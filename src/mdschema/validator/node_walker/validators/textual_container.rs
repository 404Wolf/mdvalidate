use tree_sitter::TreeCursor;

use crate::invariant_violation;
use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::mdschema::validator::{
    errors::*,
    matcher::{
        matcher::{Matcher, MatcherError},
        matcher_extras::get_all_extras,
    },
    node_walker::{
        ValidationResult,
        helpers::expected_input_nodes::expected_input_nodes,
        validators::{
            Validator, ValidatorImpl, links::LinkVsLinkValidator,
            textual::TextualVsTextualValidator,
        },
    },
    ts_utils::{
        both_are_image_nodes, both_are_link_nodes, both_are_textual_containers, count_siblings,
        get_next_node, get_node_text, is_inline_code_node, is_text_node,
    },
};

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
pub(super) struct TextualContainerVsTextualContainerValidator;

impl ValidatorImpl for TextualContainerVsTextualContainerValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        validate_textual_container_vs_textual_container_impl(walker, got_eof)
    }
}

fn validate_textual_container_vs_textual_container_impl(
    walker: &ValidatorWalker,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());

    let schema_str = walker.schema_str();

    let mut schema_cursor = walker.schema_cursor().clone();
    let mut input_cursor = walker.input_cursor().clone();

    #[cfg(feature = "invariant_violations")]
    if !both_are_textual_containers(&schema_cursor.node(), &input_cursor.node()) {
        invariant_violation!(
            result,
            &schema_cursor,
            &input_cursor,
            "expected textual container nodes"
        );
    }

    match count_non_literal_matchers_in_children(&schema_cursor, schema_str) {
        Ok(non_repeating_matchers_count) if non_repeating_matchers_count > 1 && got_eof => result
            .add_error(ValidationError::SchemaError(
                SchemaError::MultipleMatchersInNodeChildren {
                    schema_index: schema_cursor.descendant_index(),
                    received: non_repeating_matchers_count,
                },
            )),
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

        let expected_input_node_count = match expected_input_nodes(&schema_cursor, schema_str) {
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
                expected: ChildrenCount::from_specific(expected_input_node_count),
                actual: actual_input_node_count,
            },
        ));
    }

    // Go from the container to the first child in the container, and then
    // iterate over the siblings at the same rate.
    input_cursor.goto_first_child();
    schema_cursor.goto_first_child();

    loop {
        let pair_result = if both_are_link_nodes(&schema_cursor.node(), &input_cursor.node())
            || both_are_image_nodes(&schema_cursor.node(), &input_cursor.node())
        {
            LinkVsLinkValidator::validate(
                &walker.with_cursors(&schema_cursor, &input_cursor),
                got_eof,
            )
        } else {
            let new_result = TextualVsTextualValidator::validate(
                &walker.with_cursors(&schema_cursor, &input_cursor),
                got_eof,
            );
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

/// Count the number of matchers, starting at some cursor pointing to a textual
/// container, and iterating through all of its children.
///
/// Returns the number of matchers, or a `ValidationError` that is probably a
/// `MatcherError` due to failing to construct a matcher given a code node that
/// is not marked as literal.
fn count_non_literal_matchers_in_children(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<usize, ValidationError> {
    let mut count = 0;
    let mut cursor = schema_cursor.clone();

    cursor.goto_first_child();

    loop {
        if !is_inline_code_node(&cursor.node()) {
            if !cursor.goto_next_sibling() {
                break;
            } else {
                continue;
            }
        }

        // If the following node is a text node, then it may have extras, so grab them.
        let extras_str = match get_next_node(&cursor)
            .filter(|n| is_text_node(n))
            .map(|next_node| {
                let next_node_str = get_node_text(&next_node, schema_str);
                get_all_extras(next_node_str)
            }) {
            Some(Ok(extras)) => Some(extras),
            Some(Err(error)) => {
                return Err(ValidationError::SchemaError(SchemaError::MatcherError {
                    error: error.into(),
                    schema_index: schema_cursor.descendant_index(),
                }));
            }
            None => None,
        };

        let pattern_str = get_node_text(&cursor.node(), schema_str);

        match Matcher::try_from_pattern_and_suffix_str(pattern_str, extras_str) {
            Ok(_) => count += 1,
            Err(MatcherError::WasLiteralCode) => {
                // Don't count it, but this is an OK error
            }
            Err(err) => {
                return Err(ValidationError::SchemaError(SchemaError::MatcherError {
                    error: err,
                    schema_index: cursor.descendant_index(),
                }));
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::TextualContainerVsTextualContainerValidator;
    use crate::mdschema::validator::{
        errors::{SchemaError, ValidationError},
        matcher::matcher::MatcherError,
        node_pos_pair::NodePosPair,
        node_walker::validators::{
            test_utils::ValidatorTester, textual_container::count_non_literal_matchers_in_children,
        },
        ts_utils::{both_are_textual_containers, is_heading_content_node, parse_markdown},
    };

    #[test]
    fn test_count_non_literal_matchers_in_children_invalid_matcher() {
        let schema_str = "test `_*test*_`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        match count_non_literal_matchers_in_children(&schema_cursor, schema_str).unwrap_err() {
            ValidationError::SchemaError(SchemaError::MatcherError {
                error,
                schema_index,
            }) => {
                assert_eq!(schema_index, 3); // the index of the code_span
                match error {
                    MatcherError::MatcherInteriorRegexInvalid(_) => {}
                    _ => panic!("Expected MatcherInteriorRegexInvalid error"),
                }
            }
            _ => panic!("Expected InvalidMatcher error"),
        }
    }

    #[test]
    fn test_count_non_literal_matchers_in_children_only_literal_matcher() {
        let schema_str = "test `_*test*_`! `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        assert_eq!(
            count_non_literal_matchers_in_children(&schema_cursor, schema_str).unwrap(),
            1 // one is literal
        );
    }

    #[test]
    fn test_count_non_literal_matchers_in_children_no_matchers() {
        let schema_str = "test *foo* _bar_";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();

        assert_eq!(
            count_non_literal_matchers_in_children(&schema_cursor, schema_str).unwrap(),
            0
        );
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_with_content_and_link() {
        let schema_str = "# Test Wolf [hi](https://example.com)";
        let input_str = "# Test Wolf [hi](https://foobar.com)";

        let result = ValidatorTester::<TextualContainerVsTextualContainerValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .goto_first_child_then_unwrap()
        .goto_next_sibling_then_unwrap()
        .peek_nodes(|(s, i)| assert!(is_heading_content_node(s) && is_heading_content_node(i)))
        .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(9, 9));
        assert!(!result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_header_content() {
        let schema_str = "# Test Wolf";
        let input_str = "# Test Wolf";

        let result = ValidatorTester::<TextualContainerVsTextualContainerValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .goto_first_child_then_unwrap()
        .goto_next_sibling_then_unwrap()
        .peek_nodes(|(s, i)| assert!(is_heading_content_node(s) && is_heading_content_node(i)))
        .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_header_content_and_matcher() {
        let schema_str = "# Test `name:/[a-zA-Z]+/`";
        let input_str = "# Test Wolf";

        let result = ValidatorTester::<TextualContainerVsTextualContainerValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .goto_first_child_then_unwrap()
        .goto_next_sibling_then_unwrap()
        .peek_nodes(|(s, i)| assert!(is_heading_content_node(s) && is_heading_content_node(i)))
        .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(6, 4));
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({"name": "Wolf"}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_link_then_bad_node() {
        let schema_str = "# Heading [test]({a:/a/}) `b:/b/`";
        let input_str = "# Heading [test](a) b";

        let result = ValidatorTester::<TextualContainerVsTextualContainerValidator>::from_strs(
            schema_str, input_str,
        )
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
}
