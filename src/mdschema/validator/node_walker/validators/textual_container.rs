use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::*,
    matcher::{
        matcher::{Matcher, MatcherError},
        matcher_extras::get_all_extras,
    },
    node_walker::{
        ValidationResult, helpers::expected_input_nodes::expected_input_nodes,
        validators::textual::validate_textual_vs_textual,
    },
    ts_utils::{
        both_are_textual_containers, count_siblings, get_next_node, is_code_node, is_text_node,
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
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
pub fn validate_textual_container_vs_textual_container(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    debug_assert!(both_are_textual_containers(
        &schema_cursor.node(),
        &input_cursor.node()
    ));

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

    let expected_input_node_count = match expected_input_nodes(&schema_cursor, schema_str) {
        Ok(expected_input_node_count) => expected_input_node_count,
        Err(error) => {
            result.add_error(error);
            return result;
        }
    };

    let actual_input_node_count = count_siblings(&input_cursor) + 1; // including the node we are currently at
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

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    // Go from the container to the first child in the container, and then
    // iterate over the siblings at the same rate.
    input_cursor.goto_first_child();
    schema_cursor.goto_first_child();

    loop {
        let has_next_pair =
            input_cursor.clone().goto_next_sibling() && schema_cursor.clone().goto_next_sibling();

        let pair_result = validate_textual_vs_textual(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            has_next_pair || got_eof, // if we have more pairs, then eof=true. Otherwise, eof = got_eof
        );

        result.join_other_result(&pair_result);
        result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);

        if !input_cursor.goto_next_sibling() || !schema_cursor.goto_next_sibling() {
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
        if !is_code_node(&cursor.node()) {
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
                let next_node_str = next_node.utf8_text(schema_str.as_bytes()).unwrap();
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

        let pattern_str = cursor.node().utf8_text(schema_str.as_bytes()).unwrap();

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

    use crate::mdschema::validator::{
        errors::{SchemaError, ValidationError},
        matcher::matcher::MatcherError,
        node_walker::validators::{
            textual::validate_textual_vs_textual,
            textual_container::{
                count_non_literal_matchers_in_children,
                validate_textual_container_vs_textual_container,
            },
        },
        ts_utils::parse_markdown,
        validator_state::NodePosPair,
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
    fn test_validate_text_vs_text_on_text() {
        let schema_str = "test";
        // (document[0]0..5)
        // └─ (paragraph[1]0..4)
        //    └─ (text[2]0..4)

        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test";
        // (document[0]0..5)
        // └─ (paragraph[1]0..4)
        //    └─ (text[2]0..4)

        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // paragraph -> text
        schema_cursor.goto_first_child(); // paragraph -> text

        let result = validate_textual_vs_textual(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false,
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(2, 2));
    }

    #[test]
    fn test_validate_text_vs_text_header_content() {
        let schema_str = "# Test Wolf";
        // (document[0]0..12)
        // └─ (atx_heading[1]0..11)
        //    ├─ (atx_h1_marker[2]0..1)
        //    └─ (heading_content[3]1..11)
        //       └─ (text[4]1..11)
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "# Test Wolf";
        // (document[0]0..12)
        // └─ (atx_heading[1]0..11)
        //    ├─ (atx_h1_marker[2]0..1)
        //    └─ (heading_content[3]1..11)
        //       └─ (text[4]1..11)
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();
        assert_eq!(input_cursor.node().kind(), "heading_content");
        assert_eq!(schema_cursor.node().kind(), "heading_content");

        let result = validate_textual_container_vs_textual_container(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_text_vs_text_header_content_and_matcher() {
        let schema_str = "# Test `name:/[a-zA-Z]+/`";
        // (document[0]0..26)
        // └─ (atx_heading[1]0..25)
        //    ├─ (atx_h1_marker[2]0..1)
        //    └─ (heading_content[3]1..25)
        //       ├─ (text[4]1..7)
        //       └─ (code_span[5]7..25)
        //          └─ (text[6]8..24)

        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "# Test Wolf";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        // (document[0])
        // └─ (atx_heading[1])
        //    ├─ (atx_h1_marker[2])
        //    └─ (heading_content[3])
        //       └─ (text[4])

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();
        assert_eq!(input_cursor.node().kind(), "heading_content");
        assert_eq!(schema_cursor.node().kind(), "heading_content");

        let result = validate_textual_container_vs_textual_container(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"name": "Wolf"}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(6, 4));
    }

    #[test]
    fn test_validate_text_vs_text_with_incomplete_matcher() {
        let schema_str = "prefix `test:/test/`";
        // (document[0]0..21)
        // └─ (paragraph[1]0..20)
        //    ├─ (text[2]0..7)
        //    └─ (code_span[3]7..20)
        //       └─ (text[4]8..19)

        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "prefix `test:/te";
        // (document[0]0..17)
        // └─ (paragraph[1]0..16)
        //    └─ (text[2]0..16)

        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph

        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_textual_container_vs_textual_container(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // we are allowed to have a broken matcher if it is the last com
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(2, 2)) // no movement yet
    }
}
