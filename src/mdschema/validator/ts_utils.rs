use tree_sitter::{Node, Parser, Tree, TreeCursor};
use tree_sitter_markdown::language;

use crate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaViolationError, ValidationError,
};

use regex::Regex;
use std::sync::LazyLock;

/// Ordered lists use numbers followed by period . or right paren )
static ORDERED_LIST_MARKER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d+[.)]").unwrap());

/// Check whether a list marker is an ordered list marker.
///
/// https://commonmark.org/help/tutorial/06-lists.html
pub fn is_ordered_list_marker(marker: &str) -> bool {
    ORDERED_LIST_MARKER_REGEX.is_match(marker)
}

/// Unordered lists can use either asterisks *, plus +, or hyphens - as list markers.
///
/// https://commonmark.org/help/tutorial/06-lists.html
static UNORDERED_LIST_MARKER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[*+\-]").unwrap());

/// Check whether a list marker is an unordered list marker.
pub fn is_unordered_list_marker(marker: &str) -> bool {
    UNORDERED_LIST_MARKER_REGEX.is_match(marker)
}

/// Get the current treesitter node for a cursor, and the subsequent sibling.
pub fn get_node_and_next_node<'a>(cursor: &TreeCursor<'a>) -> Option<(Node<'a>, Option<Node<'a>>)> {
    let mut input_cursor = cursor.clone();

    let first_node = input_cursor.node();

    let next_node = if input_cursor.goto_next_sibling() {
        Some(input_cursor.node())
    } else {
        None
    };

    Some((first_node, next_node))
}

/// Whether the cursor has a node sibling following it.
pub fn has_subsequent_node_of_kind(cursor: &TreeCursor, kind: &str) -> bool {
    cursor.clone().goto_next_sibling() && cursor.node().kind() == kind
}

/// Create a new Tree-sitter parser for Markdown.
pub fn new_markdown_parser() -> Parser {
    let mut parser = Parser::new();
    parser.set_language(&language()).unwrap();
    parser
}

/// Parse a markdown string into a Tree-sitter tree.
#[allow(dead_code)]
pub fn parse_markdown(text: &str) -> Option<Tree> {
    let mut parser = new_markdown_parser();
    parser.parse(text, None)
}

/// Determine whether a given node is the last node in the tree.
///
/// It is the last node if it is the deepest and right most node that ends at
/// the end of the input.
pub fn is_last_node(input_str: &str, node: &Node) -> bool {
    input_str.trim().len() == node.byte_range().end
        && node.next_sibling().is_none()
        && node.child_count() == 0
}

/// Find a node by its index given by a cursor's .descendant_index().
pub fn find_node_by_index(root: Node, target_index: usize) -> Node {
    let mut cursor = root.walk();
    cursor.goto_descendant(target_index);
    cursor.node()
}

/// Check if a node is a list.
pub fn is_list_node(node: &Node) -> bool {
    match node.kind() {
        "tight_list" | "loose_list" => true,
        _ => false,
    }
}

/// Check if a node is "textual" (i.e., a text node, bold node, code node, or similar).
pub fn is_textual_node(node: &Node) -> bool {
    match node.kind() {
        "text" | "emphasis" | "strong_emphasis" | "code_span" => true,
        _ => false,
    }
}

/// Check if a node is a "textual container" (i.e., a paragraph node, list item node, or similar).
pub fn is_textual_container(node: &Node) -> bool {
    match node.kind() {
        "paragraph" | "heading_content" | "list_item" => true,
        _ => false,
    }
}

/// Determine whether the input is incomplete based on EOF status and last node.
///
/// The input is incomplete if we haven't reached the EOF and the cursor is at
/// the last node. Otherwise we're in the middle, we're not "incomplete."
pub fn waiting_at_end(got_eof: bool, last_input_str: &str, input_cursor: &TreeCursor) -> bool {
    !got_eof && is_last_node(last_input_str, &input_cursor.node())
}

/// Compare node kinds and return an error if they don't match
///
/// # Arguments
/// * `schema_cursor` - The schema cursor, pointed at any node
/// * `input_cursor` - The input cursor, pointed at any node
/// * `input_str` - The input string
/// * `schema_str` - The schema string
///
/// # Returns
/// An optional validation error if the node kinds don't match
pub fn compare_node_kinds(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    input_str: &str,
    schema_str: &str,
) -> Option<ValidationError> {
    let schema_node = schema_cursor.node();
    let input_node = input_cursor.node();

    let schema_kind = schema_node.kind();
    let input_kind = input_node.kind();

    // Special case! If they are both tight lists, check the first children of
    // each of them, which are list markers. This will indicate whether they are
    // the same type of list.
    if schema_cursor.node().kind() == "tight_list" && input_cursor.node().kind() == "tight_list" {
        let schema_list_marker = extract_list_marker(schema_cursor, schema_str);
        let input_list_marker = extract_list_marker(input_cursor, input_str);

        // They must both be unordered, both be ordered, or both have the same marker
        if schema_list_marker == input_list_marker {
            // They can be the same list symbol!
        } else if is_ordered_list_marker(schema_list_marker)
            && is_ordered_list_marker(input_list_marker)
        {
            // Or both ordered
        } else if is_unordered_list_marker(schema_list_marker)
            && is_unordered_list_marker(input_list_marker)
        {
            // Or both unordered
        } else {
            // But anything else is a mismatch

            return Some(ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    // TODO: find a better way to represent the *kind* of list in this error
                    expected: format!("{}({})", input_cursor.node().kind(), schema_list_marker),
                    actual: format!("{}({})", input_cursor.node().kind(), input_list_marker),
                },
            ));
        }
    }

    if schema_kind != input_kind {
        Some(ValidationError::SchemaViolation(
            SchemaViolationError::NodeTypeMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_node.kind().into(),
                actual: input_node.kind().into(),
            },
        ))
    } else {
        None
    }
}

/// Compare node children lengths and return an error if they don't match
///
/// # Arguments
/// * `schema_cursor` - The schema cursor, pointed at any node
/// * `input_cursor` - The input cursor, pointed at any node
/// * `got_eof` - Whether we have reached the end of file
///
/// # Returns
/// An optional validation error if the children lengths don't match
pub fn compare_node_children_lengths(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    got_eof: bool,
) -> Option<ValidationError> {
    use crate::mdschema::validator::errors::{ChildrenCount, SchemaViolationError};

    // First, count the children to check for length mismatches
    let input_child_count = input_cursor.node().child_count();
    let schema_child_count = schema_cursor.node().child_count();

    // Handle node mismatches
    // If we have reached the EOF:
    //   No difference in the number of children
    // else:
    //   We can have less input children
    //
    let children_len_mismatch_err =
        ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
            expected: ChildrenCount::from_specific(schema_child_count),
            actual: input_child_count,
        });
    if got_eof {
        // At EOF, children count must match exactly
        if input_child_count != schema_child_count {
            return Some(children_len_mismatch_err);
        }
    } else {
        // Not at EOF: input can have fewer children, but not more
        if input_child_count > schema_child_count {
            return Some(children_len_mismatch_err);
        }
    }

    None
}

/// Extract the list marker from a tight_list node
///
/// TODO: Handle UTF8 errors properly instead of unwrapping
pub fn extract_list_marker<'a>(cursor: &TreeCursor<'a>, schema_str: &'a str) -> &'a str {
    let mut cursor = cursor.clone();
    cursor.goto_first_child(); // Go to first list_item
    cursor.goto_first_child(); // Go to list_marker
    let marker = cursor.node();
    marker.utf8_text(schema_str.as_bytes()).unwrap()
}

/// Compare text contents and return an error if they don't match
///
/// # Arguments
/// * `schema_node` - The schema node to compare against
/// * `input_node` - The input node to compare
/// * `schema_str` - The full schema string
/// * `input_str` - The full input string
/// * `schema_cursor` - The schema cursor, pointed at any node that has text contents
/// * `input_cursor` - The input cursor, pointed at any node that has text contents
/// * `is_partial_match` - Whether the match is partial
///
/// # Returns
/// An optional validation error if the text contents don't match
pub fn compare_text_contents(
    schema_node: &Node,
    input_node: &Node,
    schema_str: &str,
    input_str: &str,
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    is_partial_match: bool,
) -> Option<ValidationError> {
    let (mut schema_text, input_text) = match (
        schema_node.utf8_text(schema_str.as_bytes()),
        input_node.utf8_text(input_str.as_bytes()),
    ) {
        (Ok(schema), Ok(input)) => (schema, input),
        (Err(_), _) | (_, Err(_)) => return None, // Can't compare invalid UTF-8
    };

    // If we're doing a partial match (not at EOF), adjust schema text length
    if is_partial_match {
        // If we got more input than expected, it's an error
        if input_text.len() > schema_text.len() {
            return Some(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_text.into(),
                    actual: input_text.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
        } else {
            // The schema might be longer than the input, so crop the schema to the input we've got
            schema_text = &schema_text[..input_text.len()];
        }
    }

    if schema_text != input_text {
        Some(ValidationError::SchemaViolation(
            SchemaViolationError::NodeContentMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_text.into(),
                actual: input_text.into(),
                kind: NodeContentMismatchKind::Literal,
            },
        ))
    } else {
        None
    }
}

/// Check if the treesitter schema node has a single code_span child (indicating
/// a matcher).
pub fn has_single_code_child(schema_cursor: &TreeCursor) -> bool {
    let mut code_child_count = 0;
    let cursor = schema_cursor.node().walk();
    for child in schema_cursor.node().children(&mut cursor.clone()) {
        if child.kind() == "code_span" {
            code_child_count += 1;
            if code_child_count > 1 {
                return false;
            }
        }
    }
    code_child_count == 1
}

/// Walk from a list_item node to its content paragraph.
///
/// Moves the cursor from a list_item through the list_marker to the paragraph node.
///
/// # Tree Structure
/// ```text
/// list_item
/// ├── list_marker
/// └── paragraph
///     └── text
/// ```
pub fn walk_to_list_item_content(cursor: &mut TreeCursor) {
    // list_item -> list_marker
    cursor.goto_first_child();
    debug_assert_eq!(cursor.node().kind(), "list_marker");
    // list_marker -> paragraph
    cursor.goto_next_sibling();
    debug_assert_eq!(cursor.node().kind(), "paragraph");
}

#[allow(dead_code)]
pub fn validate_str(schema: &str, input: &str) -> (serde_json::Value, Vec<ValidationError>) {
    use crate::mdschema::validator::validator_state::ValidatorState;

    let mut state = ValidatorState::new(schema.to_string(), input.to_string(), true);

    let mut parser = new_markdown_parser();
    let schema_tree = parser.parse(schema, None).unwrap();
    let input_tree = parser.parse(input, None).unwrap();

    {
        use crate::mdschema::validator::node_walker::NodeWalker;

        let mut node_validator = NodeWalker::new(&mut state, input_tree.walk(), schema_tree.walk());

        node_validator.validate();
    }

    let errors = state
        .errors_so_far()
        .into_iter()
        .cloned()
        .collect::<Vec<ValidationError>>();
    let matches = state.matches_so_far().clone();

    (matches, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_markdown_and_get_tree(input: &str) -> Tree {
        let mut parser = new_markdown_parser();
        parser.parse(input, None).unwrap()
    }

    #[test]
    fn test_is_ordered_list_marker() {
        assert!(is_ordered_list_marker("1."));
        assert!(is_ordered_list_marker("2."));
        assert!(is_ordered_list_marker("3."));
        assert!(!is_ordered_list_marker("a."));
        assert!(!is_ordered_list_marker("b."));
        assert!(!is_ordered_list_marker("c."));
    }

    #[test]
    fn test_is_unordered_list_marker() {
        assert!(is_unordered_list_marker("*"));
        assert!(is_unordered_list_marker("-"));
        assert!(is_unordered_list_marker("+"));
        assert!(!is_unordered_list_marker("1."));
        assert!(!is_unordered_list_marker("2."));
        assert!(!is_unordered_list_marker("3."));
    }

    #[test]
    fn test_has_subsequent_node_of_kind() {
        let input = "- test1\n- test2\n- test3";

        let mut parser = new_markdown_parser();
        let tree = parser.parse(input, None).unwrap();
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        cursor.goto_first_child(); // first list item
        assert_eq!(cursor.node().kind(), "list_item");

        let has_subsequent_list_item = has_subsequent_node_of_kind(&cursor, "list_item");
        assert!(has_subsequent_list_item);
    }

    #[test]
    fn test_extract_list_markers() {
        let input = "- test1\n- test2\n- test3\n# Irrelevant Heading";

        let mut parser = new_markdown_parser();
        let tree = parser.parse(input, None).unwrap();
        let mut cursor = tree.walk();
        cursor.goto_first_child(); // Go to tight_list
        assert_eq!(cursor.node().kind(), "tight_list");

        let list_marker = extract_list_marker(&cursor, input);
        assert_eq!(list_marker, "-");
    }

    #[test]
    fn test_is_textual_node() {
        // Test text node
        let tree = parse_markdown_and_get_tree("text");
        let root = tree.root_node();
        let paragraph = root.child(0).unwrap();
        let text_node = paragraph.child(0).unwrap();
        assert!(is_textual_node(&text_node));

        // Test emphasis node
        let tree = parse_markdown_and_get_tree("*emphasis*");
        let root = tree.root_node();
        let paragraph = root.child(0).unwrap();
        let emphasis_node = paragraph.child(0).unwrap();
        assert!(is_textual_node(&emphasis_node));

        // Test strong emphasis node
        let tree = parse_markdown_and_get_tree("**strong emphasis**");
        let root = tree.root_node();
        let paragraph = root.child(0).unwrap();
        let strong_emphasis_node = paragraph.child(0).unwrap();
        assert!(is_textual_node(&strong_emphasis_node));

        // Test code span node
        let tree = parse_markdown_and_get_tree("`code`");
        let root = tree.root_node();
        let paragraph = root.child(0).unwrap();
        let code_span_node = paragraph.child(0).unwrap();
        assert!(is_textual_node(&code_span_node));

        // Test code fence node
        let tree = parse_markdown_and_get_tree("```\ncode\n```");
        let root = tree.root_node();
        let code_fence_node = root.child(0).unwrap();
        assert!(!is_textual_node(&code_fence_node));

        // Test paragraph node (should not be textual)
        let tree = parse_markdown_and_get_tree("paragraph");
        let root = tree.root_node();
        let paragraph_node = root.child(0).unwrap();
        assert!(!is_textual_node(&paragraph_node));
    }

    #[test]
    fn test_waiting_at_end() {
        let input = "# First\nHello, world!";
        let mut parser = new_markdown_parser();

        let tree = parser.parse(input, None).unwrap();
        let root_node = tree.root_node();
        let mut cursor = root_node.walk();

        // At root node, not the last node, so not waiting at end
        assert_eq!(waiting_at_end(false, input, &cursor), false);

        // Got EOF at root node, so not waiting at end
        assert_eq!(waiting_at_end(true, input, &cursor), false);

        // Navigate to the actual last node (deepest, rightmost node)
        cursor.goto_first_child(); // atx_heading
        cursor.goto_next_sibling(); // paragraph
        cursor.goto_first_child(); // text node (last node)

        assert_eq!(is_last_node(input, &cursor.node()), true);

        // At the last node and haven't got EOF, so waiting at end
        assert_eq!(waiting_at_end(false, input, &cursor), true);

        // At the last node but got EOF, so not waiting at end
        assert_eq!(waiting_at_end(true, input, &cursor), false);
    }

    #[test]
    fn test_is_last_node() {
        let input = "# First\nHello, world!";
        let mut parser = new_markdown_parser();

        let tree = parser.parse(input, None).unwrap();
        let root_node = tree.root_node();

        // Root node should be a document and should not be the last node
        assert_eq!(root_node.kind(), "document");
        assert_eq!(is_last_node(input, &root_node), false);

        // First child is the heading, which is not the last node
        let first_child = root_node.child(0).unwrap();
        assert_eq!(first_child.kind(), "atx_heading");
        assert_eq!(is_last_node(input, &first_child), false);

        // Last child is the paragraph, but it's not the deepest node
        let last_child = root_node.child(root_node.named_child_count() - 1).unwrap();
        assert_eq!(last_child.kind(), "paragraph");
        assert_eq!(is_last_node(input, &last_child), false);

        // Text node is the deepest, rightmost node that ends at the input end
        let text_node = last_child.child(0).unwrap();
        assert_eq!(text_node.kind(), "text");
        assert_eq!(is_last_node(input, &text_node), true);
    }

    #[test]
    fn test_find_node_by_index() {
        let input = "# Heading\n\nThis is a paragraph.";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(input, None).unwrap();
        let root_node = tree.root_node();

        let node = find_node_by_index(root_node, 0);
        assert_eq!(node.kind(), "document");

        let node = find_node_by_index(root_node, 1);
        assert_eq!(node.kind(), "atx_heading");

        let node = find_node_by_index(root_node, 2);
        assert_eq!(node.kind(), "atx_h1_marker");

        let node = find_node_by_index(root_node, 3);
        assert_eq!(node.kind(), "heading_content");

        let node = find_node_by_index(root_node, 4);
        assert_eq!(node.kind(), "text");

        let node = find_node_by_index(root_node, 5);
        assert_eq!(node.kind(), "paragraph");

        let node = find_node_by_index(root_node, 6);
        assert_eq!(node.kind(), "text");
    }

    #[test]
    fn test_get_node_and_next_node_with_both() {
        let input = "# Heading\n\nThis is a paragraph.";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(input, None).unwrap();
        let root_node = tree.root_node();
        let mut cursor = root_node.walk();
        cursor.goto_first_child(); // Move to the first child (the heading)

        let (node, next_node) = get_node_and_next_node(&cursor).unwrap();
        assert_eq!(node.kind(), "atx_heading");
        assert!(next_node.is_some());
        assert_eq!(next_node.unwrap().kind(), "paragraph");
    }

    #[test]
    fn test_get_node_and_next_node_without_next() {
        let input = "# Heading";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(input, None).unwrap();
        let root_node = tree.root_node();
        let mut cursor = root_node.walk();
        cursor.goto_first_child(); // Move to the first child (the heading)
        cursor.goto_next_sibling(); // Move to the next sibling (which doesn't exist)
        let (node, next_node) = get_node_and_next_node(&cursor).unwrap();
        assert_eq!(node.kind(), "atx_heading");
        assert!(next_node.is_none());
    }

    #[test]
    fn test_is_list_node() {
        let input = "- Item 1\n- Item 2";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(input, None).unwrap();
        let root_node = tree.root_node();
        let list_node = root_node.child(0).unwrap();
        assert!(is_list_node(&list_node));
    }

    #[test]
    fn test_has_single_code_child() {
        // Test with single code_span child (simple matcher)
        let schema_str = "`name:/\\w+/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(
            has_single_code_child(&schema_cursor),
            "Expected code child for simple matcher"
        );

        // Test with prefix, code_span, and suffix (complex matcher)
        let schema_str = "Hello `name:/\\w+/` world!";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(
            has_single_code_child(&schema_cursor),
            "Expected code child for matcher with prefix and suffix"
        );

        // Test with no code_span (regular text)
        let schema_str = "Hello **world**!";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(
            !has_single_code_child(&schema_cursor),
            "Expected no code child for regular text"
        );

        // Test with emphasis but no code_span
        let schema_str = "This is *italic* text";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(
            !has_single_code_child(&schema_cursor),
            "Expected no code child for italic text"
        );

        // Test with multiple code_spans
        let schema_str = "Start `first:/\\w+/` middle `second:/\\d+/` end";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(
            !has_single_code_child(&schema_cursor),
            "Expected no single code child for multiple matchers"
        );

        // Test with list item containing code span (shouldn't be detected as matcher)
        let schema_str = "- test `test`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> list
        schema_cursor.goto_first_child(); // list -> list_item
        assert!(
            !has_single_code_child(&schema_cursor),
            "Expected no code child for list item with code span"
        );
    }

    #[test]
    fn test_walk_to_list_item_content() {
        let input_str = "- test content";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut cursor = input_tree.walk();

        // Navigate to the list
        cursor.goto_first_child();
        assert_eq!(cursor.node().kind(), "tight_list");

        // Navigate to the first list item
        cursor.goto_first_child();
        assert_eq!(cursor.node().kind(), "list_item");

        // Now call walk_to_list_item_content
        walk_to_list_item_content(&mut cursor);

        // Should now be at the paragraph node
        assert_eq!(cursor.node().kind(), "paragraph");
    }

    #[test]
    fn test_compare_node_kinds_list() {
        let input_1 = " - test1";
        let input_1_tree = parse_markdown(input_1).unwrap();
        let mut input_1_cursor = input_1_tree.walk();

        let input_2 = " * test1";
        let input_2_tree = parse_markdown(input_2).unwrap();
        let mut input_2_cursor = input_2_tree.walk();

        input_1_cursor.goto_first_child();
        input_2_cursor.goto_first_child();
        assert_eq!(input_2_cursor.node().kind(), "tight_list");
        assert_eq!(input_1_cursor.node().kind(), "tight_list");

        let result = compare_node_kinds(&input_2_cursor, &input_1_cursor, input_1, input_2);
        assert!(result.is_none());
    }
}
