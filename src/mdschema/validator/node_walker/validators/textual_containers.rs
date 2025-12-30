use log::trace;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::*,
    matcher::matcher::{Matcher, MatcherError, partition_at_special_chars},
    node_walker::ValidationResult,
    ts_utils::{
        both_are_textual_containers, get_next_node, is_code_node, is_text_node, waiting_at_end,
    },
    utils::{compare_node_kinds, compare_text_contents},
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
/// 1. If we're not a textual container, validate the contents and kind
///    directly and return.
/// 2. Count the number of top level matchers in the schema. Find the first
///    valid one. If there are more than 1, error.
/// 3. Count the number of nodes for both the input and schema using special
///    utility that takes into account literal matchers.
/// 4. Walk the input and schema cursors at the same rate, and walk down ane
///    recurse, which takes us to our base case of directly validating the contents
///    and kind of the node. If the node we are at is a code node, look at it and
///    the next node. If the two nodes correspond to a literal matcher, match the
///    inside of the matcher against the corresponding code node in the input. Then
///    if there is additional text in the subsequent text node after the code node,
///    check that there is a text node in the input, maybe error, and if there is,
///    validate that the contents of the rest of it is the same. Then move to the
///    next node pair, hopping two nodes at once for the schema node.
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

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    if both_are_textual_containers(&schema_cursor.node(), &input_cursor.node()) {
        todo!()
    } else {
        if let Some(error) =
            compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str)
        {
            trace!(
                "Node kinds do not match. Got schema_kind={} and input_kind={}",
                schema_cursor.node().kind(),
                input_cursor.node().kind()
            );
            result.add_error(error);

            return result;
        }

        let is_partial_match = waiting_at_end(got_eof, input_str, &input_cursor);
        if let Some(error) = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            is_partial_match,
            false,
        ) {
            trace!(
                "Text contents do not match. Got schema_text={} and input_text={}",
                schema_str, input_str
            );
            result.add_error(error);

            return result;
        }

        result
    }
}

/// Count the number of matchers, starting at some cursor pointing to a textual
/// container, and iterating through all of its children.
///
/// Returns the number of matchers, or a `ValidationError` that is probably a
/// `MatcherError` due to failing to construct a matcher given a code node that
/// is not marked as literal.
fn count_non_repeating_matchers_in_children(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<usize, ValidationError> {
    let mut count = 0;
    let mut cursor = schema_cursor.clone();

    cursor.goto_first_child();

    while cursor.goto_next_sibling() {
        if !is_code_node(&cursor.node()) {
            continue;
        }

        // If the following node is a text node, then it may have extras, so grab them.
        let extras_str = get_next_node(&cursor)
            .and_then(|next_node| {
                if !is_text_node(&next_node) {
                    return None;
                }

                let next_node_str = next_node.utf8_text(schema_str.as_bytes()).unwrap();
                partition_at_special_chars(next_node_str)
            })
            .map(|(_special_chars, after)| after);

        let pattern_str = cursor.node().utf8_text(schema_str.as_bytes()).unwrap();

        match Matcher::try_from_pattern_and_suffix_str(pattern_str, extras_str) {
            Ok(matcher) if matcher.is_repeated() => {
                return Err(ValidationError::SchemaError(
                    SchemaError::RepeatingMatcherInTextContainer {
                        schema_index: cursor.descendant_index(),
                    },
                ));
            }
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
    }

    Ok(count)
}

/// Ensures that the node-chunk count between the schema and input is the same. We're
/// not talking about the treesitter node count, but rather whether the number
/// of logical mdschema elements is the same.
///
/// To ensure the node count is the same, we expect the total number of nodes in
/// the input to be equal to that of the schema, minus the number of literal
/// nodes that do not have text coming directly after them in the schema ("the deduction").
///
/// If the very last node in the schema is a text node with "!" followed by
/// whitespace, and the node before it is a code node, that is also something
/// that counts for the deduction.
///
/// # Arguments
///
/// * `input_cursor`: The cursor pointing to a input textual container container
///   (like a paragraph) that has sibling nodes you want to check the count of.
/// * `schema_cursor`: The cursor pointing to a schema textual container (like a
///   paragraph) that has sibling nodes you want to check the count of.
/// * `schema_str`: The string representation of the schema.
///
/// For example, if the schema is
///
/// ```
/// `test`!*test*
/// ```
///
/// Which has exactly 3 paragraph children (`code_span`, `text`, `emphasis`).
///
/// Then we want an input like
///
/// ```
/// `test`*test*
/// ```
///
/// Which has exactly 2 paragraph children (`code_span`, `emphasis`).
///
/// But if the schema looks like
/// ```
/// `test`! foo*test*
/// ```
///
/// Which has text coming directly after the literal matcher, and still has
/// exactly 3 paragraph children (`code_span`, `text`, `emphasis`).
///
/// Then we want an input like
///
/// ```
/// `test` foo*test*
/// ```
///
/// Which has exactly 3 paragraph children (`code_span`, `text`, `emphasis`) instead of 2.
fn is_node_chunk_count_same(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> bool {
    let mut input_cursor = input_cursor.clone(); // paragraph, heading_content, or similar
    let mut schema_cursor = schema_cursor.clone(); // paragraph, heading_content, or similar

    input_cursor.goto_first_child(); // first child node, like text or emphasis
    schema_cursor.goto_first_child(); // first child node, like text or emphasis

    let mut input_count = 0;
    let mut schema_count = 0;
    let mut literal_matcher_not_followed_by_text_count = 0;

    while input_cursor.goto_next_sibling() && schema_cursor.goto_next_sibling() {
        input_count += 1;
        schema_count += 1;

        // If the `schema_cursor` is a code node followed by a `!`, check if it
        // has additional text coming after it.
        if is_code_node(&schema_cursor.node()) {
            match get_next_node(&schema_cursor) {
                Some(next_node) => {
                    // if the next node starts with a !, then if there is text following it in the same text node, update
                    // the `literal_matcher_followed_by_text_count` by 1. If there isn't text following the ! then don't.
                    let next_node_str = next_node.utf8_text(schema_str.as_bytes()).unwrap();
                    // TODO: if at very end whitespace after ! is OK
                    if is_text_node(&next_node)
                        && next_node_str.len() == 1
                        && next_node_str.starts_with('!')
                    {
                        literal_matcher_not_followed_by_text_count += 1;
                    }
                }
                None => {
                    // No point in going forward, the schema has nothing left.
                    break;
                }
            }
        }
    }

    // Finish both of them off. This is what will cause a mismatch if they are the same length.
    while input_cursor.goto_next_sibling() {
        input_count += 1;
    }
    while schema_cursor.goto_next_sibling() {
        schema_count += 1;
    }

    input_count - (schema_count - literal_matcher_not_followed_by_text_count) == 0
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::{
        errors::{SchemaError, ValidationError},
        matcher::matcher::MatcherError,
        node_walker::validators::{
            textual::validate_textual_vs_textual,
            textual_containers::{
                count_non_repeating_matchers_in_children, is_node_chunk_count_same,
                validate_textual_container_vs_textual_container,
            },
        },
        ts_utils::parse_markdown,
        validator_state::NodePosPair,
    };

    #[test]
    fn test_is_node_chunk_count_same_simple_text() {
        let schema_str = "test"; // one text node
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test test"; // two text nodes
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_same_mixed_text() {
        let schema_str = "test *test*"; // text + emphasis
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test _*test*_"; // text + emphasis inside bold (just 2 still)
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_obviously_different() {
        let schema_str = "test *test* _test_"; // text + emphasis + emphasis = 3 nodes
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test _*test*_"; // text + emphasis = 2 nodes
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(!is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_same_literal_matcher() {
        let schema_str = "test `_*test*_`!*bar*"; // text + literal matcher + emphasis = 4 nodes
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test `_*test*_`*bar*"; // text + literal matcher match + emphasis = 3 nodes
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_same_empty_string() {
        let schema_str = "";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_same_literal_matcher_without_following_text() {
        let schema_str = "test `_*test*_`!"; // text + literal matcher = 3 nodes
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test `_*test*_` test"; // text + literal matcher match + text = 3 nodes
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(!is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_same_literal_matcher_at_end() {
        let schema_str = "test `_*test*_`!"; // text + literal matcher = 3 nodes
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test `_*test*_`"; // text + literal matcher match = 2 nodes
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_same_literal_matcher_at_end_with_text_after() {
        let schema_str = "test `_*test*_`!"; // text + literal matcher = 3 nodes
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test `_*test*_` test"; // text + literal matcher match + text = 3 nodes
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(!is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_same_literal_matcher_followed_by_whitespace_at_end() {
        let schema_str = "test `_*test*_`!"; // text + literal matcher = 3 nodes
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test `_*test*_`  "; // text + literal matcher match = 3 nodes. There is a text node!
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_is_node_chunk_count_same_literal_matcher_followed_by_whitespace_at_end_with_text() {
        let schema_str = "test `_*test*_`!           \n "; // text + literal matcher = 3 nodes. There is a node that IS FOLLOWED BY TEXT at the end

        // For something like
        //
        // ```
        // test `_*test*_`!^^^
        // ^^^
        // ```
        //
        // Where ^ is whitespace, we still get something like
        //
        // (document[0]0..30)
        // └─ (paragraph[1]0..16)
        //    ├─ (text[2]0..5)
        //    ├─ (code_span[3]5..15)
        //    │  └─ (text[4]6..14)
        //    └─ (text[5]15..16)
        //
        // So treesitter strips off the whitespace for us!

        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test `_*test*_`"; // text + literal matcher match = 2 nodes
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        assert!(is_node_chunk_count_same(
            &input_cursor,
            &schema_cursor,
            schema_str
        ));
    }

    #[test]
    fn test_count_matchers_one_valid_matcher() {
        let schema_str = "test `foo:/bar/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        assert_eq!(
            count_non_repeating_matchers_in_children(&schema_cursor, schema_str).unwrap(),
            1
        );
    }

    #[test]
    fn test_count_matchers_one_valid_matcher_with_extras() {
        let schema_str = "test `foo:/bar/`{,}";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        // (document[0]0..20)
        // └─ (paragraph[1]0..19)
        //    ├─ (text[2]0..5)
        //    ├─ (code_span[3]5..16)
        //    │  └─ (text[4]6..15)
        //    └─ (text[5]16..19)

        match count_non_repeating_matchers_in_children(&schema_cursor, schema_str).unwrap_err() {
            ValidationError::SchemaError(SchemaError::RepeatingMatcherInTextContainer {
                schema_index,
            }) => {
                assert_eq!(schema_index, 3); // the index of the code_span
            }
            _ => panic!("Expected InvalidMatcher error"),
        }
    }

    #[test]
    fn test_count_matchers_invalid_matcher() {
        let schema_str = "test `_*test*_`";
        // (document[0]0..16)
        // └─ (paragraph[1]0..15)
        //    ├─ (text[2]0..5)
        //    └─ (code_span[3]5..15)
        //       └─ (text[4]6..14)
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        match count_non_repeating_matchers_in_children(&schema_cursor, schema_str).unwrap_err() {
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
    fn test_count_matchers_only_literal_matcher() {
        let schema_str = "test `_*test*_`! `test:/test/`";
        // (document[0]0..31)
        // └─ (paragraph[1]0..30)
        //    ├─ (text[2]0..5)
        //    ├─ (code_span[3]5..15)
        //    │  └─ (text[4]6..14)
        //    ├─ (text[5]15..17)
        //    └─ (code_span[6]17..30)
        //       └─ (text[7]18..29)

        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        assert_eq!(
            count_non_repeating_matchers_in_children(&schema_cursor, schema_str).unwrap(),
            1 // one is literal
        );
    }

    #[test]
    fn test_count_matchers_no_matchers() {
        let schema_str = "test *foo* _bar_";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        assert_eq!(
            count_non_repeating_matchers_in_children(&schema_cursor, schema_str).unwrap(),
            0
        );
    }

    #[test]
    fn test_count_matchers_empty_string() {
        let schema_str = "";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        assert_eq!(
            count_non_repeating_matchers_in_children(&schema_cursor, schema_str).unwrap(),
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

        let result =
            validate_textual_container_vs_textual_container(&input_cursor, &schema_cursor, schema_str, input_str, true);

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

        let result =
            validate_textual_container_vs_textual_container(&input_cursor, &schema_cursor, schema_str, input_str, true);

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"name": "Wolf"}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 5));
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
        schema_cursor.goto_first_child(); // paragraph -> text

        input_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // paragraph -> text

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
