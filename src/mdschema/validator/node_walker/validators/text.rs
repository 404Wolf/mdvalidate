use log::trace;
use serde_json::json;
use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::*,
    matcher::matcher::{Matcher, MatcherError, get_everything_after_special_chars},
    node_walker::{ValidationResult, node_vs_node::validate_node_vs_node},
    ts_utils::{
        both_are_text_nodes, get_next_node, get_node_and_next_node, is_code_node, is_last_node,
        is_textual_node, waiting_at_end,
    },
    utils::{compare_node_children_lengths, compare_node_kinds, compare_text_contents},
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
///    valid one. If there are more than 1, error.
/// 2. Count the number of nodes for both the input and schema.
/// 3. Iterate up until the matcher that we found in both the schema and input.
///    For each node:
///    - Check that the kind of the node in the input and schema matches
/// - Walk down into the the node and recurse. TODO: make a helper to avoid the
///   unneeded matcher checks here.
///
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
pub fn validate_text_vs_text(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    todo!()
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
                    if next_node_str.len() == 1 && next_node_str.starts_with('!') {
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
    use crate::mdschema::validator::{
        node_walker::validators::text::is_node_chunk_count_same, ts_utils::parse_markdown,
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
}
