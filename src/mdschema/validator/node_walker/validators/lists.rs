use log::trace;
use serde_json::json;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::mdschema::validator::{
    errors::{ChildrenCount, SchemaError, SchemaViolationError, ValidationError},
    matcher::matcher::{Matcher, MatcherError},
    node_walker::{
        ValidationResult,
        validators::{
            Validator, ValidatorImpl,
            textual_container::TextualContainerVsTextualContainerValidator,
        },
    },
    ts_utils::{
        count_siblings, get_node_and_next_node, has_single_code_child, has_subsequent_node_of_kind,
        is_list_item_node, is_list_marker_node, is_list_node, is_paragraph_node,
    },
    utils::compare_node_kinds,
};

/// Validate a list node against a schema list node.
///
/// Matches list items in order, supporting both literal comparison and pattern
/// matching (including recursively).
///
/// Schema list items can be:
/// - Literals: matched exactly against corresponding input items
/// - Matchers: patterns like `id:/test\d/`{2,2} that match multiple items
/// - Recursively: nested lists can be matched against nested lists, where the
///   nested lists' nodes are Literals or Matchers
///
/// # Example: Literal matching
///
/// When the schema contains literal items, they must match exactly:
///
/// **Schema:**
/// ```md
/// - test1
/// - test2
/// ```
///
/// **Input:**
/// ```md
/// - test1
/// - test2
/// ```
///
/// This validates successfully because each input item matches its corresponding schema item.
///
/// # Example: Matcher with bounded range
///
/// Matchers consume multiple input items according to their quantity constraints.
/// When a matcher reaches its maximum, the next schema matcher begins consuming items:
///
/// **Schema:**
/// ```md
/// - `id:/test\d/`{2,2}
/// - `id:/footest\d/`{1,2}
/// ```
///
/// **Input:**
/// ```md
/// - test1
/// - test2
/// - footest1
/// ```
///
/// The first matcher consumes `test1` and `test2` (reaching its max of 2).
/// Then the second matcher begins and consumes `footest1`.
///
/// Note that `test2` cannot be matched by the second matcher—once a matcher
/// reaches its limit, the cursor has moved past those items.
///
/// Note that a limitation here is that you cannot have a variable-length list
/// that is not the final list in your schema.
pub(super) struct ListVsListValidator;

impl ValidatorImpl for ListVsListValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        validate_list_vs_list_impl(walker, got_eof)
    }
}

fn validate_list_vs_list_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(walker.input_cursor(), walker.input_cursor());

    let input_str = walker.input_str();
    let schema_str = walker.schema_str();

    let mut input_cursor = walker.input_cursor().clone();
    let mut schema_cursor = walker.schema_cursor().clone();

    // We want to ensure that the types of lists are the same
    if let Some(error) = compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str) {
        result.add_error(error);
        return result;
    }

    let at_list_schema_cursor = schema_cursor.clone();
    let at_list_input_cursor = input_cursor.clone();

    if let Err(error) = ensure_at_first_list_item(&mut input_cursor) {
        result.add_error(error);
        return result;
    }
    if let Err(error) = ensure_at_first_list_item(&mut schema_cursor) {
        result.add_error(error);
        return result;
    }
    #[cfg(feature = "invariant_violations")]
    if input_cursor.node().kind() != "list_item" || schema_cursor.node().kind() != "list_item" {
        crate::invariant_violation!(
            result,
            &input_cursor,
            &schema_cursor,
            "expected list_item nodes after list traversal"
        );
    }

    match extract_repeated_matcher_from_list_item(&schema_cursor, schema_str) {
        // We were able to find a valid repeated matcher in the schema list item.
        Some(Ok(matcher)) => {
            let min_items = matcher.extras().min_items().unwrap_or(0);
            let max_items = matcher.extras().max_items();
            trace!(
                "Found repeated matcher: id={:?}, min_items={}, max_items={:?}, variable_length={}",
                matcher.id(),
                min_items,
                max_items,
                matcher.variable_length()
            );

            // We don't let you have a variable length matcher where there are more list elements in the schema.
            if matcher.variable_length() && has_subsequent_node_of_kind(&schema_cursor, "list_item")
            {
                trace!("Error: Variable length matcher found with subsequent schema list items");
                result.add_error(ValidationError::SchemaError(
                    SchemaError::RepeatingMatcherUnbounded {
                        schema_index: schema_cursor.descendant_index(),
                    },
                ));
                return result;
            }

            let mut values_at_level = Vec::with_capacity(max_items.unwrap_or(1));
            let mut validate_so_far = 0;

            loop {
                trace!("Validating list item #{}", validate_so_far + 1,);

                #[cfg(feature = "invariant_violations")]
                if input_cursor.node().kind() != "list_item"
                    || schema_cursor.node().kind() != "list_item"
                {
                    crate::invariant_violation!(
                        result,
                        &input_cursor,
                        &schema_cursor,
                        "expected list_item nodes while validating repeated matcher"
                    );
                }

                let new_matches = validate_list_item_contents_vs_list_item_contents(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                );

                validate_so_far += 1;
                values_at_level.push(new_matches.value);
                result.errors.extend(new_matches.errors);

                trace!(
                    "Completed validation of list item #{}, moving to next",
                    validate_so_far
                );

                // If we've now validated the max number of items, check if there are more
                if let Some(max_items) = max_items
                    && validate_so_far == max_items
                {
                    trace!(
                        "Reached max items limit ({}), checking if there are more items",
                        max_items
                    );

                    // Check if there are more items beyond the max
                    if input_cursor.clone().goto_next_sibling() && got_eof {
                        trace!(
                            "Error: More items than max allowed ({} > {})",
                            "at least one more", max_items
                        );
                        result.add_error(ValidationError::SchemaViolation(
                            SchemaViolationError::ChildrenLengthMismatch {
                                schema_index: schema_cursor.descendant_index(),
                                input_index: input_cursor.descendant_index(),
                                expected: ChildrenCount::from_range(min_items, Some(max_items)),
                                actual: validate_so_far + 1, // At least one more
                            },
                        ));
                    }
                    break;
                }

                // Otherwise move to the next sibling, or break if there are none left
                if !input_cursor.goto_next_sibling() {
                    break;
                }
            }

            // Check if we validated enough items
            if validate_so_far < min_items && got_eof {
                trace!(
                    "Error: Not enough items validated ({} < {}) and at EOF",
                    validate_so_far, min_items
                );
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_cursor.descendant_index(),
                        expected: ChildrenCount::from_range(min_items, max_items),
                        actual: validate_so_far,
                    },
                ));
            }

            // If we didn't make it to the end of the input list, there
            // might be more items but that correspond to another matcher.
            //
            // For example, with a schema like:
            //
            // ```md
            // - `testA:/test\d/`{2,2}
            // - `testB:/line2test\d/`{2,2}
            // ```
            //
            // And input like:
            //
            // ```md
            // - test1
            // - test2
            // - line2test1
            // - line2test2
            // ```
            //
            // We want to validate the first two, pushing them into our
            // list, and then the second two.
            //
            // { "testA": ["test1", "test2"],
            //   "testB": ["line2test1", "line2test2"] }
            //
            // In these cases we are looking at an schema tree that looks like:
            //
            // (tight_list)
            // ├── (list_item) <-- where we are now
            // │   ├── (list_marker)
            // │   └── (paragraph)
            // │       ├── (code_span)
            // │       │   └── (text)
            // │       └── (text)
            // └── (list_item) <-- where we are after .goto_next_sibling() when it returns true
            //     ├── (list_marker)
            //     └── (paragraph)
            //         ├── (code_span)
            //         │   └── (text)
            //         └── (text)
            //
            // If there are more items to validate AT THE SAME LEVEL, recurse to
            // validate them. We now use the *next* schema node too.
            if schema_cursor.goto_next_sibling() && input_cursor.goto_next_sibling() {
                let next_result = ListVsListValidator::validate(
                    &walker.with_cursors(&input_cursor, &schema_cursor),
                    got_eof,
                );
                result.join_other_result(&next_result);
            }

            trace!("Completed validation of all {} list items", validate_so_far);

            // Now, if there's another pair, recurse and validate it
            if input_cursor.goto_first_child() && schema_cursor.goto_first_child() {
                while input_cursor.goto_next_sibling() && schema_cursor.goto_next_sibling() {}

                // There is a deeper list!
                if is_list_node(&input_cursor.node()) && is_list_node(&schema_cursor.node()) {
                    trace!(
                        "Found next sibling pairs, recursing to validate next list elements; cursors are at {:?} and {:?}",
                        input_cursor.node().kind(),
                        schema_cursor.node().kind()
                    );

                    let next_result = ListVsListValidator::validate(
                        &walker.with_cursors(&input_cursor, &schema_cursor),
                        got_eof,
                    );
                    // We need to be able to capture errors that happen in the recursive call
                    result.errors.extend(next_result.errors);
                    values_at_level.push(next_result.value);
                }
            } else {
                trace!("No more sibling pairs found");
            }

            // Store the array that we just gathered
            if let Some(matcher_id) = matcher.id() {
                trace!("Storing matches for matcher id: {}", matcher_id);

                result.set_match(
                    matcher_id,
                    json!(
                        values_at_level
                            .iter()
                            .map(|value| {
                                // If we have a schema:
                                //
                                // ```md
                                // - `name:/test\d/`{2,2}
                                //   - `name:/test\d/`{1,1}
                                // ```
                                //
                                // Initially, we run this at the top level, gather something like
                                //
                                // matches_at_level = [{ "test": "test1" }, { "test": "test2" }]
                                //
                                // Then we might recurse, and end up with something like
                                //
                                // matches_at_level = [{ "test": "test1" }, { "test": "test2" }, { "deep": "test3" }]
                                //
                                // Then we iterate over the matches_at_level and unpack all the ones that have our
                                // id (we are top level), so "test," and get
                                //
                                // matches_at_level = ["test1", "test2", { "deep": "test3" }]
                                //
                                // Note that we don't unpack anything that is not our id (see below, where we
                                // "don't unpack!").

                                let mut matches_as_obj = value.as_object().unwrap().clone();

                                // TODO: can we avoid these clones?
                                if let Some(matcher_id) = matcher.id() {
                                    let match_for_same_id = matches_as_obj.remove(matcher_id);

                                    // Unwrap it to be loose in the array if we can
                                    match match_for_same_id {
                                        Some(match_for_same_id) => match_for_same_id,
                                        None => value.clone(), // don't unpack!
                                    }
                                } else {
                                    value.clone()
                                }
                            })
                            .collect::<Vec<_>>()
                    ),
                );
            }

            // Now we have validated as many as we could, let's add it to the result.
            // Update the cursors to be as far as we got, and then join the results.
            trace!("Returning validation result for repeated matcher");
            return result;
        }
        // We were able to find a matcher in the schema list item, but it was invalid (we failed to parse it).
        Some(Err(e)) => {
            trace!("Error: Found invalid matcher in schema list item: {:?}", e);
            result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error: e,
                schema_index: schema_cursor.descendant_index(),
            }));
        }
        // We didn't find a repeating matcher. In this case, just use validate the insides directly and move on.
        None => {
            trace!(
                "No repeated matcher found, using textual validation. Current node kinds: {:?} and {:?}",
                input_cursor.node().kind(),
                schema_cursor.node().kind()
            );

            // In this case we want to make sure that the children have the
            // exact same length, since they are both literal lists. Dynamic
            // lengths aren't allowed for literal lists.
            let remaining_schema_nodes = count_siblings(&schema_cursor);
            let remaining_input_nodes = count_siblings(&input_cursor);

            let literal_chunk_count = count_next_n_literal_lists(&schema_cursor, schema_str);
            if remaining_schema_nodes != remaining_input_nodes {
                let available_literal_items = remaining_input_nodes + 1;

                if available_literal_items < literal_chunk_count {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::ChildrenLengthMismatch {
                            schema_index: at_list_schema_cursor.descendant_index(),
                            input_index: at_list_input_cursor.descendant_index(),
                            // +1 because we need to include this first node that we are currently on
                            expected: ChildrenCount::from_specific(literal_chunk_count),
                            actual: available_literal_items,
                        },
                    ));
                    return result;
                }
            }

            if remaining_schema_nodes != remaining_input_nodes
                && literal_chunk_count == remaining_schema_nodes + 1
            {
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch {
                        schema_index: at_list_schema_cursor.descendant_index(),
                        input_index: at_list_input_cursor.descendant_index(),
                        // +1 because we need to include this first node that we are currently on
                        expected: ChildrenCount::from_specific(remaining_schema_nodes + 1),
                        actual: remaining_input_nodes + 1,
                    },
                ));
                return result;
            }

            let list_item_match_result = validate_list_item_contents_vs_list_item_contents(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                got_eof,
            );
            result.join_other_result(&list_item_match_result);

            {
                // Recurse down into the next list if there is one
                let mut input_cursor = input_cursor.clone();
                let mut schema_cursor = schema_cursor.clone();

                input_cursor.goto_last_child();
                schema_cursor.goto_last_child();

                if let Some(error) =
                    compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str)
                {
                    result.add_error(error);
                    return result;
                }

                if is_list_node(&input_cursor.node()) {
                    // and we know that schema is the same
                    input_cursor.goto_first_child();
                    schema_cursor.goto_first_child();

                    let deeper_result = ListVsListValidator::validate(
                        &walker.with_cursors(&input_cursor, &schema_cursor),
                        got_eof,
                    );
                    result.join_other_result(&deeper_result);
                }
            }

            // Recurse on next sibling if available!
            if input_cursor.goto_next_sibling() && schema_cursor.goto_next_sibling() {
                trace!("Moving to next sibling list items for continued validation");
                let new_matches = ListVsListValidator::validate(
                    &walker.with_cursors(&input_cursor, &schema_cursor),
                    got_eof,
                );
                result.join_other_result(&new_matches);
            } else {
                trace!("No more sibling pairs found, validation complete");
            }
        }
    }

    result
}

/// Walk forward and see how many lists after this one at the same level are literal lists.
fn count_next_n_literal_lists(schema_cursor: &TreeCursor, schema_str: &str) -> usize {
    let mut schema_cursor = schema_cursor.clone();
    let mut count = 0;
    loop {
        match extract_repeated_matcher_from_list_item(&schema_cursor, schema_str) {
            Some(Ok(_)) | Some(Err(_)) => break,
            None => {
                count += 1;
            }
        }

        if !schema_cursor.goto_next_sibling() {
            break;
        }
    }

    count
}

/// Validate the contents of a list item against the contents of a different
/// list item.
///
/// ```ansi
/// ├─ (list_item) <-- we are here
/// │  ├─ (list_marker) <-- we already validated this
/// │  └─ (paragraph) <-- we want to be here
/// │     └─ (text)
/// ```
///
/// Walks into their actual paragraphs and runs textual container validation.
fn validate_list_item_contents_vs_list_item_contents(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

    #[cfg(feature = "invariant_violations")]
    if !is_list_item_node(&schema_cursor.node()) || !is_list_item_node(&input_cursor.node()) {
        crate::invariant_violation!(
            result,
            &input_cursor,
            &schema_cursor,
            "expected list_item nodes before validating list item contents"
        );
    }

    schema_cursor.goto_first_child();
    input_cursor.goto_first_child();

    #[cfg(feature = "invariant_violations")]
    if !is_list_marker_node(&schema_cursor.node()) || !is_list_marker_node(&input_cursor.node()) {
        crate::invariant_violation!(
            result,
            &input_cursor,
            &schema_cursor,
            "expected list_marker nodes while validating list item contents"
        );
    }

    schema_cursor.goto_next_sibling();
    input_cursor.goto_next_sibling();

    #[cfg(feature = "invariant_violations")]
    if !is_paragraph_node(&schema_cursor.node()) || !is_paragraph_node(&input_cursor.node()) {
        crate::invariant_violation!(
            result,
            &input_cursor,
            &schema_cursor,
            "expected paragraph nodes while validating list item contents"
        );
    }

    let walker =
        ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
    TextualContainerVsTextualContainerValidator::validate(&walker, got_eof)
}

/// Creates a new matcher from a tree-sitter cursor pointing at code node in
/// the Markdown schema's tree.
///
/// This will attempt to grab the current node the cursor is pointing at,
/// which must be a code node, and the following one, which will be counted
/// as extras if it is a text node.
fn try_from_code_and_text_node_cursor(
    cursor: &TreeCursor,
    schema_str: &str,
) -> Result<Matcher, MatcherError> {
    let (node, next_node) = get_node_and_next_node(cursor).ok_or_else(|| {
        MatcherError::InvariantViolation(
            "Cursor has no current node to extract matcher from".to_string(),
        )
    })?;

    if node.kind() != "code_span" {
        return Err(MatcherError::InvariantViolation(
            "Cursor is not pointing at a code_span node".to_string(),
        ));
    }

    try_from_code_and_text_node(node, next_node, schema_str)
}

/// Create a new Matcher from two tree-sitter nodes and a schema string.
///
/// - The first node should be a code_span node containing the matcher pattern.
/// - The second node (optional) should be a text node containing extras.
fn try_from_code_and_text_node(
    matcher_node: tree_sitter::Node,
    suffix_node: Option<tree_sitter::Node>,
    schema_str: &str,
) -> Result<Matcher, MatcherError> {
    let matcher_text = matcher_node.utf8_text(schema_str.as_bytes()).map_err(|_| {
        MatcherError::MatcherInteriorRegexInvalid("Invalid UTF-8 in matcher node".to_string())
    })?;

    let suffix_text = suffix_node
        .map(|node| node.utf8_text(schema_str.as_bytes()).ok())
        .flatten();

    Matcher::try_from_pattern_and_suffix_str(matcher_text, suffix_text)
}

/// Walk from a list item node to the actual content, which is a paragraph node.
/// Extract a repeated matcher from a list item node.
///
/// Returns a matcher if the list item contains a repeated matcher pattern like:
///
/// ```md
/// - `name:/pattern/`{1,}
/// ```
///
/// Returns `None` if:
/// - The list item doesn't contain a matcher
/// - The matcher is not repeated
///
/// Otherwise we attempt to construct the matcher, maybe returning an error.
fn extract_repeated_matcher_from_list_item(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Option<Result<Matcher, MatcherError>> {
    #[cfg(feature = "invariant_violations")]
    if schema_cursor.node().kind() != "list_item" {
        crate::invariant_violation!(
            schema_cursor,
            schema_cursor,
            "expected list_item while extracting repeated matcher"
        );
    }

    #[cfg(not(feature = "invariant_violations"))]
    if schema_cursor.node().kind() != "list_item" {
        return Some(Err(MatcherError::InvariantViolation(
            "expected list_item while extracting repeated matcher".to_string(),
        )));
    }

    // If the first node in the list item is not a paragraph that starts with a
    // code node, we can't have a matcher.
    let list_item_node = schema_cursor.node();
    let mut list_item_cursor = list_item_node.walk();

    list_item_cursor.goto_first_child(); // Should be a paragraph

    // If it's a list_marker we can move ahead to the next sibling though
    if list_item_cursor.node().kind() == "list_marker" {
        list_item_cursor.goto_next_sibling();
    }

    if list_item_cursor.node().kind() != "paragraph" {
        trace!(
            "List item does not contain a paragraph, got {}",
            list_item_cursor.node().kind()
        );
        return None;
    }

    if !has_single_code_child(&list_item_cursor) {
        trace!("List item does not contain a single code child");
        return None;
    }
    // list_item -> code_span (first item in list_item)
    list_item_cursor.goto_first_child();
    if list_item_cursor.node().kind() != "code_span" {
        trace!("List item code_span is not the first paragraph child");
        return None;
    }

    match try_from_code_and_text_node_cursor(&list_item_cursor, schema_str) {
        Ok(matcher) if matcher.is_repeated() => Some(Ok(matcher)),
        Ok(_) => None,
        Err(e @ MatcherError::MatcherInteriorRegexInvalid(_)) => Some(Err(e)),
        Err(e) => {
            trace!("Failed to extract repeated matcher from list item: {}", e);
            None
        }
    }
}

/// Ensure that the cursor is at the first list item in the list.
fn ensure_at_first_list_item(input_cursor: &mut TreeCursor) -> Result<(), ValidationError> {
    if input_cursor.node().kind() == "tight_list" {
        input_cursor.goto_first_child();

        #[cfg(feature = "invariant_violations")]
        if input_cursor.node().kind() != "list_item" {
            crate::invariant_violation!(
                input_cursor,
                input_cursor,
                "expected list_item while walking into list"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::panic;

    use serde_json::json;

    use super::ListVsListValidator;
    use crate::mdschema::validator::{
        errors::{ChildrenCount, NodeContentMismatchKind, SchemaViolationError, ValidationError},
        node_walker::{
            ValidationResult,
            validators::{
                lists::{ensure_at_first_list_item, extract_repeated_matcher_from_list_item},
                test_utils::ValidatorTester,
            },
        },
        ts_utils::{both_are_list_nodes, parse_markdown},
    };

    fn validate_lists(schema_str: &str, input_str: &str, got_eof: bool) -> ValidationResult {
        ValidatorTester::<ListVsListValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(i, s)| assert!(both_are_list_nodes(i, s)))
            .validate(got_eof)
    }

    fn validate_list_items(schema_str: &str, input_str: &str, got_eof: bool) -> ValidationResult {
        ValidatorTester::<ListVsListValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate(got_eof)
    }

    #[test]
    fn test_ensure_at_first_list_item() {
        // Test with - (hyphen)
        let input_str = "- test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor).unwrap();
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with - (hyphen)
        let input_str = "- test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor).unwrap();
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with + (plus)
        let input_str = "+ test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor).unwrap();
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with * (asterisk)
        let input_str = "* test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor).unwrap();
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with ordered list (1.)
        let input_str = "1. test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor).unwrap();
        assert_eq!(input_cursor.node().kind(), "list_item");
    }

    #[test]
    fn test_try_from_code_and_text_node_cursor() {
        // Test successful matcher creation from cursor
        use super::try_from_code_and_text_node_cursor;
        use crate::mdschema::validator::ts_utils::new_markdown_parser;

        let schema_str = "`word:/\\w+/`{,} suffix";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(schema_str, None).unwrap();
        let root = tree.root_node();
        let paragraph = root.child(0).unwrap();

        let mut cursor = paragraph.walk();
        cursor.goto_first_child(); // go to first child (text or code_span)

        // Move cursor to the code_span node
        while cursor.node().kind() != "code_span" {
            if !cursor.goto_next_sibling() {
                panic!("No code_span node found");
            }
        }

        let matcher = try_from_code_and_text_node_cursor(&cursor, schema_str).unwrap();

        assert_eq!(matcher.id(), Some("word"));
        assert_eq!(matcher.match_str("hello"), Some("hello"));
        assert_eq!(matcher.match_str("123"), Some("123"));
        assert_eq!(matcher.match_str("!@#"), None);
        assert!(matcher.is_repeated());
    }

    #[test]
    fn test_extract_repeated_matcher_from_list_item() {
        let input_str = "- `name:/pattern/`{1,1}";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        ensure_at_first_list_item(&mut input_cursor).unwrap();

        let matcher = extract_repeated_matcher_from_list_item(&mut input_cursor, input_str)
            .unwrap()
            .unwrap();

        assert_eq!(matcher.id(), Some("name".into()));
        // MatcherType is now always a regex pattern
        assert!(!format!("{}", matcher.pattern()).is_empty());
    }

    #[test]
    fn test_extract_repeated_matcher_from_nested_list_item() {
        let schema_str = "- item1\n  - `name:/pattern/`{1,1}";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        schema_cursor.goto_first_child(); // -> tight_list-1
        assert_eq!(schema_cursor.node().kind(), "tight_list");
        schema_cursor.goto_first_child(); // -> list_item
        assert_eq!(schema_cursor.node().kind(), "list_item");
        schema_cursor.goto_first_child(); // -> list_marker

        while schema_cursor.goto_next_sibling() {} // -> tight_list-2
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        schema_cursor.goto_first_child(); // -> list_item
        assert_eq!(schema_cursor.node().kind(), "list_item");
        // schema_cursor.goto_first_child(); // -> list_marker
        // assert_eq!(schema_cursor.node().kind(), "list_marker");

        let matcher = extract_repeated_matcher_from_list_item(&schema_cursor, schema_str)
            .unwrap()
            .unwrap();
        assert_eq!(matcher.id(), "name".into());
    }

    #[test]
    fn test_validate_list_vs_list_literal_list_items_matching() {
        let schema_str = "- test1\n- test2";
        let input_str = "- test1\n- test2";
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_validate_list_vs_list_literal_list_items_different() {
        let input_str = "- test1\n- different";
        let schema_str = "- test1\n- test2";
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    kind: NodeContentMismatchKind::Literal,
                    schema_index: 9,
                    input_index: 9,
                    expected: "test2".into(),
                    actual: "different".into(),
                }
            )]
        );
    }

    #[test]
    fn test_validate_list_vs_list_literal_list_items_with_nesting_mismatch() {
        let schema_str = "- test1\n  - nested1";
        let input_str = "- test1\n  - nested_different";
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    kind: NodeContentMismatchKind::Literal,
                    schema_index: 10,
                    input_index: 10,
                    expected: "nested1".into(),
                    actual: "nested_different".into(),
                }
            )],
            "Expected NodeContentMismatch error with nested literal items"
        );
    }

    #[test]
    fn test_link_in_list_item() {
        let schema_str = "- [link]({url:/.*/})";
        let input_str = "- [link](https://404wolf.com)";

        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(result.errors, vec![],);
        assert_eq!(result.value, json!({"url": "https://404wolf.com"}));
    }

    #[test]
    fn test_validate_list_vs_list_literal_list_items_with_nesting_mismatch_and_more() {
        let schema_str = "- test1\n  - nested1\n- test2";
        let input_str = "- test1\n  - nested1\n- test3";
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    kind: NodeContentMismatchKind::Literal,
                    schema_index: 14,
                    input_index: 14,
                    expected: "test2".into(),
                    actual: "test3".into(),
                }
            )],
            "Expected NodeContentMismatch error with mismatched literal items"
        );
    }

    #[test]
    fn test_validate_list_vs_list_literal_list_items_with_nesting() {
        let schema_str = "- test1\n- test2\n  - nested1";
        let input_str = "- test1\n- test2\n  - nested1";
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors with nested literal items, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_validate_list_vs_list_literal_list_items_with_matcher() {
        let schema_str = r#"
- test1
- `id:/test\d/`
- test3

Footer: test (footer isn't validated with_list_vs_list)
"#;
        let input_str = r#"
- test1
- test2
- test3

Footer: test (footer isn't validated with_list_vs_list)
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({"id": "test2"}));
    }

    #[test]
    fn test_validate_list_vs_list_literal_then_repeated_matcher() {
        let schema_str = r#"
# Test `name:/[A-Za-z]+/`

- test
- `item:/test\d/`{,}
"#;
        let input_str = r#"
# Test Example

- test
- test1
- test2
"#;
        let result = ValidatorTester::<ListVsListValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_next_sibling_then_unwrap()
            .peek_nodes(|(i, s)| assert!(both_are_list_nodes(i, s)))
            .validate(false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({"item": ["test1", "test2"]}));
    }

    #[test]
    fn test_validate_list_vs_list_literal_literal_matcher_matcher_literal_literal_literal() {
        let schema_str = r#"
- literal1
- literal2
- `matcher1:/match1_\d/`{1,1}
- `matcher2:/match2_\d/`{1,1}
- literal3
- literal4
- literal5
"#;
        let input_str = r#"
- literal1
- literal2
- match1_1
- match2_1
- literal3
- literal4
- literal5
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(
            result.value,
            json!({"matcher1": ["match1_1"], "matcher2": ["match2_1"]})
        );
    }

    #[test]
    fn test_validate_list_vs_list_literal_chunk_mismatch_before_repeated_matcher() {
        let schema_str = r#"
- literal1
- literal2
- `item:/test\d/`{,}
"#;
        let input_str = r#"
- literal1
- test1
- test2
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    kind: NodeContentMismatchKind::Literal,
                    schema_index: 9,
                    input_index: 9,
                    expected: "literal2".into(),
                    actual: "test1".into(),
                }
            )],
            "Expected errors for literal chunk mismatch"
        );
    }

    #[test]
    fn test_validate_list_vs_list_literal_items_length_mismatch() {
        // Test case 1: Input is shorter than schema
        let schema_str = r#"
- test1
- `id:/test\d/`
- test3
- test4
- test5
- test6

Footer: test (footer isn't validated with_list_vs_list)
"#;
        let input_str = r#"
- test1
- test2
- test3

Footer: test (footer isn't validated with_list_vs_list)
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(result.value, json!({}));
        assert_eq!(
            result.errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::ChildrenLengthMismatch {
                    schema_index: 1,
                    input_index: 1,
                    expected: ChildrenCount::from_specific(6),
                    actual: 3,
                }
            )]
        );

        // Test case 2: Input is longer than schema
        let schema_str = r#"
- test1
- `id:/test\d/`
- test3

Footer: test (footer isn't validated with_list_vs_list)
"#;
        let input_str = r#"
- test1
- test2
- test3
- test4
- test5
- test6

Footer: test (footer isn't validated with_list_vs_list)
"#;
        let result = validate_lists(schema_str, input_str, true);

        assert_eq!(result.value, json!({})); // we stop early. TODO: capture as much as we can
        assert_eq!(
            result.errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::ChildrenLengthMismatch {
                    schema_index: 1,
                    input_index: 1,
                    expected: ChildrenCount::from_specific(3),
                    actual: 6,
                }
            )]
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_simple_matcher() {
        let schema_str = r#"- `test:/test\d/`{2,2}"#;
        let input_str = "- test1\n- test2";
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(result.value, json!({"test": ["test1", "test2"]}));
    }

    #[test]
    fn test_validate_list_vs_list_with_stacked_matcher() {
        let schema_str = r#"
- `testA:/test\d/`{2,2}
- `testB:/line2test\d/`{2,2}
"#;
        let input_str = r#"
- test1
- test2
- line2test1
- line2test2
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(
            result.value,
            json!({"testA": ["test1", "test2"], "testB": ["line2test1", "line2test2"]})
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_stacked_matcher_too_many_first() {
        let schema_str = r#"
- `testA:/test\d/`{1,1}
- `testB:/line2test\d/`{1,1}
"#;
        let input_str = r#"
- test1
- test2
- line2test1
"#;
        let result = validate_lists(schema_str, input_str, false);
        // even with eof=false we should know that there is an error by now

        assert_eq!(
            result.errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    kind: NodeContentMismatchKind::Matcher,
                    schema_index: 11,
                    input_index: 9,
                    expected: "^line2test\\d".into(),
                    actual: "test2".into(),
                }
            )],
            "Expected an error"
        );

        // TODO: strange that only in this case we seem to support partial outputs
        assert_eq!(result.value, json!({"testA": ["test1"], "testB": [{}]}));
    }

    #[test]
    fn test_validate_list_vs_list_with_nested_matcher() {
        let schema_str = r#"
- `test:/test\d/`{1,1}
    - `deep:/deep\d/`{1,1}
"#;
        let input_str = r#"
- test1
    - deep1
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(
            result.value,
            json!({"test": ["test1", {"deep": ["deep1"]}]})
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_deep_nesting() {
        let schema_str = r#"
- `test:/test\d/`{2,2}
    + `deep:/deep\d/`{1,1}
        - `deeper:/deeper\d/`{2,2}
"#;
        let input_str = r#"
- test1
- test2
    + deep1
        - deeper1
        - deeper2
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(
            result.value,
            json!({
                "test": [
                    "test1",
                    "test2",
                    {
                        "deep": [
                            "deep1",
                            {
                                "deeper": [ "deeper1", "deeper2" ]
                            }
                        ]
                    }
                ]
            })
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_deep_nesting_and_stacking() {
        let schema_str = r#"
- `test:/test\d/`{2,2}
- `barbar:/barbar\d/`{2,2}
    + `deep:/deep\d/`{1,1}
        - `deeper:/deeper\d/`{2,2}
        - `deepest:/deepest\d/`{2,}
"#;
        let input_str = r#"
- test1
- test2
- barbar1
- barbar2
    + deep1
        - deeper1
        - deeper2
        - deepest1
        - deepest2
        - deepest3
        - deepest4
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(
            result.value,
            json!({
                "barbar": [
                    "barbar1",
                    "barbar2",
                    {
                        "deep": [
                            "deep1",
                            {
                                "deeper": [
                                    "deeper1",
                                    "deeper2"
                                ],
                                "deepest": [
                                    "deepest1",
                                    "deepest2",
                                    "deepest3",
                                    "deepest4"
                                ]
                            }
                        ]
                    }
                ],
                "test": [
                    "test1",
                    "test2"
                ]
            })
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_mismatched_list_kind() {
        let schema_str = r#"
- `test:/test\d/`{1,1}
    1. `deep:/deep\d/`{1,1}
"#;
        // Input uses '-' at second level instead of '+' like the schema
        let input_str = r#"
- test1
    - deep1
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert!(
            !result.errors.is_empty(),
            "Expected errors due to mismatched list kinds at second level"
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_max_in_deep_list() {
        // Test case: nested list with max constraint that is exceeded
        let schema_str = r#"
- `test:/test\d/`{1,1}
    - `deep:/deep\d/`{,3}
"#;
        let input_str = r#"
- test1
    - deep1
    - deep2
    - deep3
    - deep4
"#;
        let result = validate_lists(schema_str, input_str, false);

        // Should stop at max (3) and not validate the 4th item
        assert_eq!(
            result.errors,
            vec![],
            "Should not error when max is reached, just stop validating"
        );

        // Should only capture 3 items even though 4 were provided
        assert_eq!(
            result.value,
            json!({"test": ["test1", {"deep": ["deep1", "deep2", "deep3"]}]})
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_min_max() {
        // Positive case: within min/max bounds
        let schema_str = r#"
- `test:/test\d/`{2,5}
"#;
        let input_str = r#"
- test1
- test2
- test3
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors for valid non-nested list"
        );
        assert_eq!(result.value, json!({"test": ["test1", "test2", "test3"]}));

        // Negative case: below minimum
        let schema_str = r#"
- `test:/test\d/`{2,5}
"#;
        let input_str = r#"
- test1
"#;
        let result = validate_lists(schema_str, input_str, true);

        assert!(
            !result.errors.is_empty(),
            "Expected errors when list has fewer items than minimum"
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_max_only() {
        // Positive case: within max bound
        let schema_str = r#"
- `test:/test\d/`{,3}
"#;
        let input_str = r#"
- test1
- test2
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({"test": ["test1", "test2"]}));

        // Negative case: exceeds maximum (stops at max)
        let schema_str = r#"
- `test:/test\d/`{,2}
"#;
        let input_str = r#"
- test1
- test2
- test3
- test4
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Should not error when got_eof is false, just stop at max"
        );
        assert_eq!(result.value, json!({"test": ["test1", "test2"]}));

        // Negative case with EOF: should error when exceeding max
        let result = validate_lists(schema_str, input_str, true);

        assert_eq!(
            result.errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::ChildrenLengthMismatch {
                    schema_index: 2,
                    input_index: 6,
                    expected: ChildrenCount::from_range(0, Some(2)),
                    actual: 3,
                }
            )],
            "Expected error when list exceeds maximum with EOF"
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_min_only() {
        // Positive case: meets minimum
        let schema_str = r#"
- `test:/test\d/`{2,}
"#;
        let input_str = r#"
- test1
- test2
- test3
- test4
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors when list meets minimum"
        );
        assert_eq!(
            result.value,
            json!({"test": ["test1", "test2", "test3", "test4"]})
        );

        // Negative case: below minimum
        let schema_str = r#"
- `test:/test\d/`{3,}
"#;
        let input_str = r#"
- test1
- test2
"#;
        let result = validate_lists(schema_str, input_str, true);

        assert!(
            !result.errors.is_empty(),
            "Expected errors when list has fewer items than minimum"
        );
    }

    #[test]
    fn test_validate_list_vs_list_with_unlimited() {
        // Positive case: unlimited matcher with multiple items
        let schema_str = r#"
- `test:/test\d/`{0,}
"#;
        let input_str = r#"
- test1
- test2
- test3
- test4
- test5
"#;
        let result = validate_lists(schema_str, input_str, false);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors for unlimited matcher"
        );
        assert_eq!(
            result.value,
            json!({"test": ["test1", "test2", "test3", "test4", "test5"]})
        );

        // Positive case: unlimited matcher with zero items
        let schema_str = r#"
- `test:/test\d/`{0,}
"#;
        let input_str = r#"
"#;
        let tester = ValidatorTester::<ListVsListValidator>::from_strs(schema_str, input_str);
        let mut tester_walker = tester.walk();
        if tester_walker.goto_first_child_then().is_ok() {
            let result = tester_walker.validate(true);

            assert!(
                result.errors.is_empty() || result.errors[0].to_string().contains("kind"),
                "Empty list should be acceptable for {{0,}} matcher or fail on kind mismatch"
            );
        }
    }

    #[test]
    fn test_list_vs_list_one_item_different_contents() {
        let schema_str = r#"
- Item 1
- Item 2
"#;
        let input_str = r#"
- Item 1
- Item 3
"#;
        let result = validate_list_items(schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        assert_eq!(
            result.errors[0],
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                // TODO: make sure these indexes are correct
                schema_index: 9,
                input_index: 9,
                expected: "Item 2".into(),
                actual: "Item 3".into(),
                kind: NodeContentMismatchKind::Literal,
            })
        );
    }

    #[test]
    fn test_list_vs_list_one_item_same_contents() {
        let schema_str = "# List\n- Item 1\n- Item 2\n";
        let input_str = "# List\n- Item 1\n- Item 2\n";
        let result = ValidatorTester::<ListVsListValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_next_sibling_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate(true);

        assert_eq!(result.errors, vec![]);
    }

    #[test]
    fn test_validate_list_vs_list_with_nesting_lists() {
        let schema_str = r#"
- `test:/\w+/`{2,2}
  - `test2:/\w+/`{1,1}
"#;
        let input_str = r#"
- test1
- test2
  - deepy
"#;
        let result = validate_lists(schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);

        assert_eq!(
            result.value,
            json!({
                "test": [
                    "test1",
                    "test2",
                    { "test2": [ "deepy" ] }
                ]
            })
        );
    }
}
