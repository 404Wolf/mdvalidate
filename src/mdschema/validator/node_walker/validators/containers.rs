//! Textual container validator for node-walker comparisons.
//!
//! Types:
//! - `TextualContainerVsTextualContainerValidator`: walks inline children in
//!   paragraphs/emphasis and validates them with matcher support and link-aware
//!   handling.
//! - `RepeatedMatcherParagraphVsParagraphValidator`: handles paragraphs that
//!   contain a single repeating matcher, collecting matches across repeated
//!   paragraphs before delegating to nested validation.
use crate::mdschema::validator::matcher::matcher::MatcherKind;
use crate::mdschema::validator::node_walker::helpers::check_repeating_matchers::check_repeating_matchers;
use crate::mdschema::validator::node_walker::helpers::count_non_literal_matchers_in_children::count_non_literal_matchers_in_children;
use crate::mdschema::validator::ts_utils::{get_node_text, waiting_at_end};
use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::mdschema::validator::{
    errors::*,
    matcher::matcher::Matcher,
    node_walker::{
        ValidationResult,
        helpers::expected_input_nodes::expected_input_nodes,
        validators::{
            Validator, ValidatorImpl, links::LinkVsLinkValidator,
            textual::TextualVsTextualValidator,
        },
    },
    ts_types::*,
    ts_utils::count_siblings,
};
use crate::{compare_node_kinds_check, invariant_violation};
use derive_builder::Builder;
use log::trace;
use serde_json::Value;
use tree_sitter::TreeCursor;

/// Validate a textual region of input against a textual region of schema.
///
/// Takes two cursors pointing at text containers in the schema and input, and
/// validates them. The input text container may have a single matcher, and
/// potentially many other types of nodes. For example:
///
/// Schema:
/// ```md
/// **Test** _*test*_ `test///`! `match:/test/` *foo*.
/// ```
///
/// Input:
/// ```md
/// **Test** _*test*_ `test///`! test *foo*.
///
/// # Algorithm
///
/// This works by:
///
/// 1. Count the number of top level matchers in the schema. Find the first
///    valid one. Then keep going, but if there are more than 1, error.
/// 2. Count the number of nodes for both the input and schema using special
///    utility that takes into account literal matchers.
/// 3. Walk the input and schema cursors at the same rate, and walk down ane
///    recurse, which takes us to our base case of directly validating the contents
///    and kind of the node. If the node we are at is a code node, look at it and
///    the next node. If the two nodes correspond to a literal matcher:
///    - Match the inside of the matcher against the corresponding code node in the input.
///    - Then if there is additional text in the subsequent text node after the code node,
///      check that there is a text node in the input, maybe error, and if there is,
///      validate that the contents of the rest of it is the same.
///    - Then move to the next node pair, hopping two nodes at once for the schema node.
#[derive(Default, Builder)]
pub(super) struct ContainerVsContainerValidator {
    allow_repeating: bool,
}

impl ValidatorImpl for ContainerVsContainerValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut result =
            ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());
        let need_to_restart_result = result.clone();

        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        #[cfg(feature = "invariant_violations")]
        if !both_are_textual_containers(&schema_cursor.node(), &input_cursor.node()) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "expected textual container nodes, got {:?} and {:?}",
                schema_cursor.node().kind(),
                input_cursor.node().kind()
            );
        }

        compare_node_kinds_check!(
            schema_cursor,
            input_cursor,
            walker.schema_str(),
            walker.input_str(),
            result
        );

        if is_repeated_matcher_paragraph(&schema_cursor, walker.schema_str()) {
            return RepeatedMatcherParagraphVsParagraphValidator::default()
                .validate(walker, got_eof);
        }

        if !self.allow_repeating {
            if let Some(repeating_matcher_index) =
                check_repeating_matchers(&schema_cursor, walker.schema_str())
            {
                result.add_error(ValidationError::SchemaError(
                    SchemaError::RepeatingMatcherInTextContainer {
                        schema_index: repeating_matcher_index,
                    },
                ));
                return result;
            }
        }

        match count_non_literal_matchers_in_children(&schema_cursor, walker.schema_str()) {
            Ok(non_literal_matchers_in_children)
                if non_literal_matchers_in_children > 1 && got_eof =>
            {
                result.add_error(ValidationError::SchemaError(
                    SchemaError::MultipleMatchersInNodeChildren {
                        schema_index: schema_cursor.descendant_index(),
                        received: non_literal_matchers_in_children,
                    },
                ))
            }
            Ok(_) => {
                // Exactly one non repeating matcher is OK!
            }
            Err(err) => {
                result.add_error(err);

                return result;
            }
        }

        let (expected_input_node_count, actual_input_node_count) = {
            let mut schema_cursor = schema_cursor.clone();
            schema_cursor.goto_first_child();

            let mut input_cursor = input_cursor.clone();
            input_cursor.goto_first_child();

            let expected_input_node_count =
                match expected_input_nodes(&schema_cursor, walker.schema_str()) {
                    Ok(expected_input_node_count) => expected_input_node_count,
                    Err(error) => {
                        result.add_error(error);
                        return result;
                    }
                };

            let actual_input_node_count = count_siblings(&input_cursor) + 1; // including the node we are currently at

            (expected_input_node_count, actual_input_node_count)
        };

        if (actual_input_node_count != expected_input_node_count) && got_eof {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::ChildrenLengthMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: expected_input_node_count.into(),
                    actual: actual_input_node_count,
                },
            ));
        }

        // Go from the container to the first child in the container, and then
        // iterate over the siblings at the same rate.
        match (
            input_cursor.goto_first_child(),
            schema_cursor.goto_first_child(),
        ) {
            (true, true) => {} // nothing to do
            (false, false) => {
                return result;
            }
            (false, true) => {
                if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                    // okay, we'll just wait!
                    return need_to_restart_result;
                } else {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::MalformedNodeStructure {
                            schema_index: schema_cursor.descendant_index(),
                            input_index: input_cursor.descendant_index(),
                            kind: MalformedStructureKind::InputHasChildSchemaDoesnt,
                        },
                    ));
                }
            }
            (true, false) => {
                if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                    // okay, we'll just wait!
                    return need_to_restart_result;
                } else {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::MalformedNodeStructure {
                            schema_index: schema_cursor.descendant_index(),
                            input_index: input_cursor.descendant_index(),
                            kind: MalformedStructureKind::SchemaHasChildInputDoesnt,
                        },
                    ));
                }
                return result;
            }
        }

        loop {
            let pair_result = if both_are_link_nodes(&schema_cursor.node(), &input_cursor.node())
                || both_are_image_nodes(&schema_cursor.node(), &input_cursor.node())
            {
                LinkVsLinkValidator::default()
                    .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof)
            } else {
                let new_result = TextualVsTextualValidator::default()
                    .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
                new_result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);
                new_result
            };

            result.join_other_result(&pair_result);

            if !schema_cursor.goto_next_sibling() || !input_cursor.goto_next_sibling() {
                break;
            }
        }

        result
    }
}

/// We special case paragraphs that are just a single code node that is a
/// repeated matcher. This function attempts to match what we call a repeated
/// matcher paragraph.
///
/// If we see a paragraph that looks like:
///
/// ```md
/// # Hi there
///
/// `paragraphs:/.*/`{2,2}
/// ```
///
/// (Where the `` `test:/.*/`{,} `` is the paragraph)
///
/// Then we expect an input that has that many paragraphs, and we accumulate them into an array:
///
/// ```md
/// # Hi there
///
/// This is the first paragraph
///
/// This is the second paragraph
/// ```
///
/// And the output is
///
/// ```json
/// {
///     "paragraphs": [
///         "This is the first paragraph",
///         "This is the second paragraph"
///     ]
/// }
/// ```
#[derive(Default)]
pub(super) struct RepeatedMatcherParagraphVsParagraphValidator;

impl ValidatorImpl for RepeatedMatcherParagraphVsParagraphValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut result =
            ValidationResult::from_cursors(walker.schema_cursor(), &walker.input_cursor());

        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        #[cfg(feature = "invariant_violations")]
        if !both_are_paragraphs(
            &walker.schema_cursor().node(),
            &walker.input_cursor().node(),
        ) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "repeated containers are only possible for paragraphs"
            );
        }

        let next_schema_cursor = {
            let mut schema_cursor = schema_cursor.clone();
            schema_cursor.goto_next_sibling();
            schema_cursor
        };

        if !schema_cursor.goto_first_child() {
            #[cfg(feature = "invariant_violations")]
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "for repeating matchers we should always have a first child in the schema"
            );
        }

        match Matcher::try_from_schema_cursor(&schema_cursor, walker.schema_str()) {
            Ok(matcher) if matcher.is_repeated() => {
                let mut matches = vec![];

                let extras = matcher.extras();

                let max_matches = extras.max_items_or(usize::MAX);
                for _ in 0..max_matches {
                    // compare the ENTIRE text of the paragraph
                    let input_paragraph_text =
                        get_node_text(&input_cursor.node(), walker.input_str());

                    match matcher.match_str(input_paragraph_text) {
                        Some(matched) => matches.push(matched),
                        None => {}
                    }

                    let prev_sibling = input_cursor.clone();
                    if input_cursor.goto_next_sibling() && is_paragraph_node(&input_cursor.node()) {
                        // continue
                    } else {
                        input_cursor.reset_to(&prev_sibling);
                        break;
                    }
                }

                if matches.len() < extras.min_items_or(0) {
                    if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                        // That's ok. We may get them later.
                        return result;
                    } else {
                        result.add_error(ValidationError::SchemaViolation(
                            SchemaViolationError::WrongListCount {
                                schema_index: schema_cursor.descendant_index(),
                                input_index: input_cursor.descendant_index(),
                                min: extras.min_items(),
                                max: extras.max_items(),
                                actual: matches.len(),
                            },
                        ));
                        return result;
                    }
                }

                input_cursor.goto_next_sibling();
                result.sync_cursor_pos(&next_schema_cursor, &input_cursor);

                if let Some(id) = matcher.id() {
                    result.set_match(
                        id,
                        serde_json::Value::Array(
                            matches
                                .iter()
                                .map(|s| Value::String(s.to_string()))
                                .collect(),
                        ),
                    );
                }

                result
            }
            _ => {
                #[cfg(feature = "invariant_violations")]
                invariant_violation!(
                    &schema_cursor,
                    &input_cursor,
                    "we should be at a repeated matcher"
                )
            }
        }
    }
}

/// Check whether a paragraph is a repeated paragraph matcher.
///
/// A paragraph is a repeated paragraph matcher if it has a single child, which
/// is a a repeated matcher.
///
/// For example,
///
/// ```md
/// `test:/test/`{,}
/// ```
///
/// Contains a document with one child, which is a repeated paragraph matcher,
/// whereas
///
/// ```md
/// `test:/test/` test
/// ```
///
/// Contains a document with one child, which is just a normal paragraph with a
/// matcher in it.
///
/// # Arguments
///
/// * `schema_cursor`: The cursor pointing to a paragraph that might be a repeated matcher paragraph.
/// * `schema_str`: The full input document (so far).
fn is_repeated_matcher_paragraph(schema_cursor: &TreeCursor, schema_str: &str) -> bool {
    // We must be at a paragraph node
    if !is_paragraph_node(&schema_cursor.node()) {
        trace!("is_repeated_matcher_paragraph: not a paragraph node, returning false");
        return false;
    }

    // All repeating matchers have a code span followed by text. This is a nonstarter.
    if schema_cursor.node().child_count() != 2 {
        trace!("is_repeated_matcher_paragraph: child count is not 2, returning false");
        return false;
    }

    let mut schema_cursor = schema_cursor.clone();
    schema_cursor.goto_first_child(); // note we know there is one because we checked above

    match Matcher::try_from_schema_cursor(&schema_cursor, schema_str) {
        Ok(matcher) if matcher.is_repeated() && matches!(matcher.kind(), MatcherKind::All) => true,
        Ok(_) => false,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ContainerVsContainerValidator, is_repeated_matcher_paragraph};
    use crate::mdschema::validator::{
        errors::{SchemaViolationError, ValidationError},
        node_pos_pair::NodePosPair,
        node_walker::validators::{
            containers::RepeatedMatcherParagraphVsParagraphValidator, test_utils::ValidatorTester,
        },
        ts_types::*,
        ts_utils::parse_markdown,
    };

    #[test]
    fn test_is_repeated_matcher_paragraph_simple_non_paragraph() {
        let schema_str = "`test`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        assert!(is_inline_code_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_simple_non_repeating() {
        let schema_str = "this is just a normal paragraph";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_simple_repeating_matcher() {
        let schema_str = "`test`{,}";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_matcher_non_repeating() {
        let schema_str = "`test` test";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_matcher_invalid_matcher() {
        let schema_str = "`fjeiaofjioweajf` test";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_matcher_valid_literal_matcher() {
        let schema_str = "`this is invalid`! test";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_with_content_and_link() {
        let schema_str = "# Test Wolf [hi](https://example.com)";
        let input_str = "# Test Wolf [hi](https://foobar.com)";

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .goto_next_sibling_then_unwrap()
                .peek_nodes(|(s, i)| {
                    assert!(is_heading_content_node(s) && is_heading_content_node(i))
                })
                .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(9, 9));
        assert!(!result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_header_content() {
        let schema_str = "# Test Wolf";
        let input_str = "# Test Wolf";

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .goto_next_sibling_then_unwrap()
                .peek_nodes(|(s, i)| {
                    assert!(is_heading_content_node(s) && is_heading_content_node(i))
                })
                .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_header_content_and_matcher() {
        let schema_str = "# Test `name:/[a-zA-Z]+/`";
        let input_str = "# Test Wolf";

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .goto_next_sibling_then_unwrap()
                .peek_nodes(|(s, i)| {
                    assert!(is_heading_content_node(s) && is_heading_content_node(i))
                })
                .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(6, 4));
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({"name": "Wolf"}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_link_then_bad_node() {
        let schema_str = "# Heading [test]({a:/a/}) `b:/b/`";
        let input_str = "# Heading [test](a) b";

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .goto_next_sibling_then_unwrap()
                .peek_nodes(|(s, i)| assert!(both_are_textual_containers(s, i)))
                .validate_complete();

        let errors = result.errors().to_vec();
        let value = result.value().clone();

        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(12, 10)
        );
        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"a": "a", "b": "b"}));
    }

    #[test]
    fn test_paragraph_vs_repeated_matcher_paragraph_simple() {
        let schema_str = r#"
`items`{,}
"#;
        let input_str = r#"
foo

bar

buzz
"#;

        let result = ValidatorTester::<RepeatedMatcherParagraphVsParagraphValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
        .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(1, 5));
        assert_eq!(result.errors(), vec![]);
        assert_eq!(*result.value(), json!({"items": ["foo", "bar", "buzz"]}));
    }

    #[test]
    fn test_paragraph_vs_repeated_matcher_paragraph_simple_with_stuff_after() {
        let schema_str = r#"
`items`{,}

# Test
"#;
        let input_str = r#"
foo

bar

buzz

# Test
"#;

        let result = ValidatorTester::<RepeatedMatcherParagraphVsParagraphValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
        .validate_complete();

        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(5, 7) // at the subsequent heading
        );
        assert_eq!(result.errors(), vec![]);
        assert_eq!(*result.value(), json!({"items": ["foo", "bar", "buzz"]}));
    }

    #[test]
    fn test_paragraph_vs_repeated_matcher_paragraph_with_italic() {
        let schema_str = r#"
`items`{,}
"#;
        let input_str = r#"
foo

bar *italic*

buzz
"#;

        let result = ValidatorTester::<RepeatedMatcherParagraphVsParagraphValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
        .validate_complete();

        let _errors = result.errors().to_vec();
        let value = result.value().clone();

        assert_eq!(value, json!({"items": ["foo", "bar *italic*", "buzz"]}));
    }

    #[test]
    fn test_paragraph_vs_paragraph_with_normal_matcher() {
        let schema_str = r#"
`data:/test/`
"#;
        let input_str = r#"
test
"#;

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
                .validate_complete();

        // Should have no errors since "test" matches the pattern "^test"
        assert_eq!(result.errors(), vec![]);
        assert_eq!(*result.value(), json!({"data": "test"}));
    }

    #[test]
    fn test_paragraph_vs_paragraph_with_normal_matcher_mismatch() {
        let schema_str = r#"
`data:/test/`
"#;
        let input_str = r#"
foo
"#;

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
                .validate_complete();

        // Should have an error since "foo" doesn't match the pattern "^test"
        assert!(!result.errors().is_empty());
    }

    #[test]
    fn test_paragraph_vs_paragraph_with_min() {
        let schema_str = r#"
`data`{2,}
"#;
        let input_str = r#"
test
"#;

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
                .validate_complete();

        assert_eq!(
            result.errors(),
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::WrongListCount {
                    schema_index: 2,
                    input_index: 1,
                    min: Some(2),
                    max: None,
                    actual: 1
                }
            )]
        );
        assert_eq!(*result.value(), json!({}));
    }

    #[test]
    fn test_paragraph_vs_paragraph_with_min_incomplete() {
        let schema_str = r#"
`data`{2,}
"#;
        let input_str = r#"
test
"#;

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
                .validate_incomplete();

        // no errors yet since incomplete. we don't have too many, we have too
        // few, so we may get them later
        assert_eq!(result.errors(), vec![]);
        assert_eq!(*result.value(), json!({})); // no matches yet
    }

    #[test]
    fn test_paragraph_vs_paragraph_with_max() {
        let schema_str = r#"
`data`{,2}
"#;
        let input_str = r#"
test

foo

bar
"#;

        let result =
            ValidatorTester::<ContainerVsContainerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
                .validate_complete();

        assert_eq!(result.errors(), vec![]); // stops yoinking after the max
        assert_eq!(*result.value(), json!({"data": ["test", "foo"]}));
    }
}
