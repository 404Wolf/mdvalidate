use log::trace;
use serde_json::json;
use tracing::{instrument, warn};
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{ChildrenCount, SchemaError, SchemaViolationError, ValidationError},
    matcher::matcher::{Matcher, MatcherError},
    node_walker::{ValidationResult, text_vs_text::validate_text_vs_text},
    ts_utils::{
        compare_node_kinds, get_siblings, has_single_code_child, has_subsequent_node_of_kind,
    },
};

/// Validate a list node against a schema list node.
///
/// For each element in the schema list, if it is a literal, match it against
/// the corresponding input list element and move on.
///
/// ```md
/// - test1^
/// - test2
/// ```
///
/// ```md
/// - test1^
/// - test2
/// ```
///
/// If the cursor is at a matcher in the schema list, check what its range of
/// allowed number of matching input nodes is. Only the last schema matcher node
/// in a list of them can have an unbounded range.
///
/// Then move on to the next element in the schema list, and repeat.
///
/// ```md
/// - test1
/// - test2^ can't match this one anymore!
/// - footest2
/// ```
///
/// ```md
/// - `id:/test\d/`{2,2}^
/// - `id:/footest\d/`{,2}
/// ```
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_list_vs_list(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        input_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    // We want to ensure that the types of lists are the same
    if let Some(error) = compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str) {
        result.add_error(error);
        return result;
    }

    ensure_at_first_list_item(&mut input_cursor);
    ensure_at_first_list_item(&mut schema_cursor);
    debug_assert_eq!(input_cursor.node().kind(), "list_item");
    debug_assert_eq!(schema_cursor.node().kind(), "list_item");

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
                        input_index: input_cursor.descendant_index(),
                    },
                ));
                return result;
            }

            let input_list_items_at_level = &get_siblings(&input_cursor);
            trace!(
                "Found {} input list items at this level",
                &input_list_items_at_level.len()
            );

            // If there aren't enough items, if we are at EOF, we can report an error right away.
            if input_list_items_at_level.len() < min_items && got_eof {
                trace!(
                    "Error: Not enough input items ({} < {}) and at EOF",
                    input_list_items_at_level.len(),
                    min_items
                );
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_cursor.descendant_index(),
                        expected: ChildrenCount::from_range(min_items, max_items),
                        actual: input_list_items_at_level.len(),
                    },
                ));
            } else {
                // Okay, there ARE enough items, or at least some items. Let's
                // try to validate up to the max. We may not have enough but
                // let's validate as far as we can.
                trace!(
                    "Proceeding to validate list items: available={}, min={}, max={:?}",
                    input_list_items_at_level.len(),
                    min_items,
                    max_items
                );

                let mut values_at_level = Vec::with_capacity(max_items.unwrap_or(1));
                let mut validate_so_far = 0;

                for input_list_item in input_list_items_at_level {
                    trace!("Validating list item #{}", validate_so_far + 1);

                    // If we've already validated the max number of items, stop.
                    if let Some(max_items) = max_items
                        && validate_so_far >= max_items
                    {
                        trace!(
                            "Reached max items limit ({}), stopping validation",
                            max_items
                        );
                        break;
                    }

                    debug_assert_eq!(input_list_item.kind(), "list_item");

                    // TODO: for now we only ever validate the item as text to save on overhead of node_vs_node
                    // Explore whether this is a bad assumption
                    let new_matches = validate_text_vs_text(
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

                    // Move the cursor so that when we call
                    // validate_node_vs_node in the next iteration it's at the
                    // right spot.
                    input_cursor.goto_next_sibling();
                }

                // If we didn't make it to the end of the input list, there
                // might be more items but that correspond to another matcher.

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
                // In these cases we are looking at a tree that looks like:
                //
                // (tight_list)
                // ├── (list_item)^
                // │   ├── (list_marker)
                // │   └── (paragraph)
                // │       ├── (code_span)
                // │       │   └── (text)
                // │       └── (text)
                // └── (list_item)
                //     ├── (list_marker)
                //     └── (paragraph)
                //         ├── (code_span)
                //         │   └── (text)
                //         └── (text)

                let more_items_are_left = input_list_items_at_level.len() > validate_so_far;
                if more_items_are_left {
                    dbg!(schema_cursor.node().to_sexp());
                    debug_assert_eq!(schema_cursor.node().kind(), "list_item");

                    // TODO: what if there is no next node in the schema? Make sure we're done validating everything?
                    if schema_cursor.goto_next_sibling() {
                        let next_result = validate_list_vs_list(
                            &input_cursor,
                            &schema_cursor,
                            schema_str,
                            input_str,
                            got_eof,
                        );
                        result.join_other_result(&next_result);
                    }
                }

                trace!("Completed validation of all {} list items", validate_so_far);

                trace!(
                    "Result so far (at level): \n{:?}\ninput_sexpr={}\nschema_sexpr={}",
                    values_at_level,
                    input_cursor.node().to_sexp(),
                    schema_cursor.node().to_sexp()
                );

                // Now, if there's another pair, recurse and validate it
                if input_cursor.goto_first_child() && schema_cursor.goto_first_child() {
                    while input_cursor.goto_next_sibling() && schema_cursor.goto_next_sibling() {}

                    // There is a deeper list!
                    if input_cursor.node().kind() == "tight_list"
                        && schema_cursor.node().kind() == "tight_list"
                    {
                        trace!(
                            "Found next sibling pairs, recursing to validate next list elements; cursors are at {:?} and {:?}",
                            input_cursor.node().kind(),
                            schema_cursor.node().kind()
                        );

                        let next_result = validate_list_vs_list(
                            &input_cursor,
                            &schema_cursor,
                            schema_str,
                            input_str,
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
        }
        // We were able to find a matcher in the schema list item, but it was invalid (we failed to parse it).
        Some(Err(e)) => {
            trace!("Error: Found invalid matcher in schema list item: {:?}", e);
            result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error: e,
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
            }));
        }
        // We didn't find a repeating matcher. In this case, just use text_vs_text and move on.
        None => {
            trace!("No repeated matcher found, using text_vs_text validation");
            let list_item_match_result = validate_text_vs_text(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                got_eof,
            );
            result.join_other_result(&list_item_match_result);

            // Recurse on next sibling if available!
            if input_cursor.goto_next_sibling() && schema_cursor.goto_next_sibling() {
                trace!("Moving to next sibling list items for continued validation");
                let new_matches = validate_list_vs_list(
                    &mut input_cursor,
                    &mut schema_cursor,
                    schema_str,
                    input_str,
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
    debug_assert_eq!(schema_cursor.node().kind(), "list_item");

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
        warn!(
            "List item does not contain a paragraph, got {}",
            list_item_cursor.node().kind()
        );
        return None;
    }

    if !has_single_code_child(&list_item_cursor) {
        warn!("List item does not contain a single code child");
        return None;
    }
    // list_item -> code_span (first item in list_item)
    list_item_cursor.goto_first_child();
    debug_assert_eq!(list_item_cursor.node().kind(), "code_span");

    match Matcher::try_from_cursor(&list_item_cursor, schema_str) {
        Ok(matcher) if matcher.is_repeated() => Some(Ok(matcher)),
        Ok(_) => None,
        Err(e @ MatcherError::MatcherInteriorRegexInvalid(_)) => Some(Err(e)),
        Err(e) => {
            warn!("Failed to extract repeated matcher from list item: {}", e);
            None
        }
    }
}

/// Ensure that the cursor is at the first list item in the list.
fn ensure_at_first_list_item(input_cursor: &mut TreeCursor) {
    if input_cursor.node().kind() == "tight_list" {
        input_cursor.goto_first_child();
        debug_assert_eq!(input_cursor.node().kind(), "list_item");
    }
}

#[cfg(test)]
mod tests {
    use std::panic;

    use serde_json::json;

    use crate::mdschema::validator::{
        errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
        matcher::matcher::MatcherType,
        node_walker::list_vs_list::{
            ensure_at_first_list_item, extract_repeated_matcher_from_list_item,
            validate_list_vs_list,
        },
        ts_utils::parse_markdown,
    };

    #[test]
    fn test_ensure_at_first_list_item() {
        // Test with - (hyphen)
        let input_str = "- test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with - (hyphen)
        let input_str = "- test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with + (plus)
        let input_str = "+ test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with * (asterisk)
        let input_str = "* test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with ordered list (1.)
        let input_str = "1. test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");
    }

    #[test]
    fn test_extract_repeated_matcher_from_list_item() {
        let input_str = "- `name:/pattern/`{1,1}";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        ensure_at_first_list_item(&mut input_cursor);

        let matcher = extract_repeated_matcher_from_list_item(&mut input_cursor, input_str)
            .unwrap()
            .unwrap();

        assert_eq!(matcher.id(), Some("name".into()));
        assert!(matches!(matcher.pattern(), MatcherType::Regex(_)));
    }

    #[test]
    fn test_extract_repeated_matcher_from_nested_list_item() {
        let schema_str = "- item1\n  - `name:/pattern/`{1,1}";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        // (document)
        // └── (tight_list)1
        //     └── (list_item)
        //         ├── (list_marker)
        //         ├── (paragraph)
        //         │   └── (text)
        //         └── (tight_list)2
        //             └── (list_item)
        //                 ├── (list_marker)
        //                 └── (paragraph)
        //                     ├── (code_span)
        //                     │   └── (text)
        //                     └── (text)

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
    fn test_validate_list_vs_list_literal_list_items() {
        let schema_str = "- test1\n- test2";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "- test1\n- test2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );

        // Test with different list items (should have errors)
        let input_str = "- test1\n- different";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        let schema_str = "- test1\n- test2";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(!result.errors.is_empty());
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                kind: NodeContentMismatchKind::Literal,
                ..
            }) => {}
            _ => panic!("Unexpected error type"),
        }
    }

    #[test]
    fn validate_list_vs_list_with_simple_matcher() {
        let schema_str = r#"- `test:/test\d/`{2,2}"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "- test1\n- test2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(result.value, json!({"test": ["test1", "test2"]}));
    }

    #[test]
    fn validate_list_vs_list_with_stacked_matcher() {
        let schema_str = r#"
- `testA:/test\d/`{2,2}
- `testB:/line2test\d/`{2,2}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
- line2test1
- line2test2
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(schema_cursor.node().kind(), "tight_list");
        assert_eq!(input_cursor.node().kind(), "tight_list");
        dbg!(schema_cursor.node().to_sexp());

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(
            result.value,
            json!({"testA": ["test1", "test2"], "testB": ["line2test1", "line2test2"]})
        );
    }

    #[test]
    fn validate_list_vs_list_with_stacked_matcher_too_many_first() {
        let schema_str = r#"
- `testA:/test\d/`{2,2}
- `testB:/line2test\d/`{2,2}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
- test3
- line2test1
- line2test2
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(schema_cursor.node().kind(), "tight_list");
        assert_eq!(input_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);
        dbg!(&result);

        assert_eq!(
            result.errors.len(),
            1,
            "Expected one error, got: {:?}",
            result.errors
        );

        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                kind: NodeContentMismatchKind::Matcher,
                schema_index,
                input_index,
                expected,
                actual,
            }) => {
                assert_eq!(*schema_index, 11);
                assert_eq!(*input_index, 12);
                assert_eq!(expected, "^line2test\\d");
                assert_eq!(actual, "test3");
            }
            _ => panic!(
                "Expected NodeContentMismatch error with Matcher kind, got: {:?}",
                result.errors[0]
            ),
        }

        assert_eq!(
            result.value,
            json!({"testA": ["test1", "test2"], "testB": [{}, "line2test1"]})
        );
    }

    #[test]
    fn validate_list_vs_list_with_nested_matcher() {
        let schema_str = r#"
- `test:/test\d/`{1,1}
    - `deep:/deep\d/`{1,1}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
    - deep1
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        dbg!(schema_cursor.node().to_sexp());

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(
            result.value,
            json!({"test": ["test1", {"deep": ["deep1"]}]})
        );
    }

    #[test]
    fn validate_list_vs_list_with_deep_nesting() {
        let schema_str = r#"
- `test:/test\d/`{2,2}
    + `deep:/deep\d/`{1,1}
        - `deeper:/deeper\d/`{2,2}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
    + deep1
        - deeper1
        - deeper2
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
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
    fn validate_list_vs_list_with_mismatched_list_kind() {
        let schema_str = r#"
- `test:/test\d/`{1,1}
    1. `deep:/deep\d/`{1,1}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        // Input uses '-' at second level instead of '+' like the schema
        let input_str = r#"
- test1
    - deep1
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            !result.errors.is_empty(),
            "Expected errors due to mismatched list kinds at second level"
        );
    }

    #[test]
    fn validate_list_vs_list_with_min_max() {
        // Positive case: within min/max bounds
        let schema_str = r#"
- `test:/test\d/`{2,5}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
- test3
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
            "Expected no errors for valid non-nested list"
        );
        assert_eq!(result.value, json!({"test": ["test1", "test2", "test3"]}));

        // Negative case: below minimum
        let schema_str = r#"
- `test:/test\d/`{2,5}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            !result.errors.is_empty(),
            "Expected errors when list has fewer items than minimum"
        );
    }

    #[test]
    fn validate_list_vs_list_with_max_only() {
        // Positive case: within max bound
        let schema_str = r#"
- `test:/test\d/`{,3}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
            "Expected no errors when list is within max bound"
        );
        assert_eq!(result.value, json!({"test": ["test1", "test2"]}));

        // Negative case: exceeds maximum (stops at max)
        let schema_str = r#"
- `test:/test\d/`{,2}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
- test3
- test4
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
            "Should not error, just stop at max"
        );
        assert_eq!(result.value, json!({"test": ["test1", "test2"]}));
    }

    #[test]
    fn validate_list_vs_list_with_min_only() {
        // Positive case: meets minimum
        let schema_str = r#"
- `test:/test\d/`{2,}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
- test3
- test4
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
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
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            !result.errors.is_empty(),
            "Expected errors when list has fewer items than minimum"
        );
    }

    #[test]
    fn validate_list_vs_list_with_unlimited() {
        // Positive case: unlimited matcher with multiple items
        let schema_str = r#"
- `test:/test\d/`{0,}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
- test3
- test4
- test5
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");
        assert_eq!(schema_cursor.node().kind(), "tight_list");

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
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
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();

        // Handle case where input might not have a list at all
        if input_cursor.goto_first_child() {
            let result =
                validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, true);

            assert!(
                result.errors.is_empty() || result.errors[0].to_string().contains("kind"),
                "Empty list should be acceptable for {{0,}} matcher or fail on kind mismatch"
            );
        }
    }
}
