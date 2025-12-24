use tree_sitter::{Node, Parser, Tree, TreeCursor};
use tree_sitter_markdown::language;

use crate::mdschema::validator::errors::ValidationError;

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

/// Check whether a node is a codeblock.
pub fn is_codeblock(node: &Node) -> bool {
    match node.kind() {
        "fenced_code_block" => true,
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

/// Extract the language and body of a codeblock.
///
/// # Arguments
///
/// * `cursor`: The cursor pointing to the codeblock node.
/// * `src`: The source text of the document.
///
/// # Returns
///
/// An `Option` containing:
/// - The optional language tuple: `(language_string, descendant_index)` if the language text is present
/// - The body tuple: `(body_string, descendant_index)` of the code content
/// Where `descendant_index` is the index of the descendant node that contains the language or body text.
///
/// Returns `None` if the codeblock is invalid or it isn't a codeblock to begin with.
pub fn extract_codeblock_contents(
    cursor: &TreeCursor,
    src: &str,
) -> Option<(Option<(String, usize)>, (String, usize))> {
    // A codeblock looks like this:
    //
    // └── (fenced_code_block)
    //     ├── (info_string)?   // only present when there is a language
    //     │   └── (text)
    //     └── (code_fence_content)
    //         └── (text)
    //
    // └── (fenced_code_block)
    //     └── (code_fence_content)
    //         └── (text)

    let mut cursor = cursor.clone();
    if cursor.node().kind() != "fenced_code_block" {
        return None;
    }

    // Move to the first child and determine if it's an info_string or the content
    if !cursor.goto_first_child() {
        return None;
    }

    let mut language: Option<(String, usize)> = None;

    if cursor.node().kind() == "info_string" {
        // Extract language from info_string -> text
        if !cursor.goto_first_child() || cursor.node().kind() != "text" {
            return None;
        }
        language = Some((
            cursor.node().utf8_text(src.as_bytes()).ok()?.to_string(),
            cursor.descendant_index(),
        ));

        // Go back to info_string, then to its sibling: code_fence_content
        if !cursor.goto_parent() || !cursor.goto_next_sibling() {
            return None;
        }
    } else if cursor.node().kind() != "code_fence_content" {
        // First child is neither info_string nor code_fence_content -> invalid layout
        return None;
    }

    // At this point, cursor must be at code_fence_content
    debug_assert_eq!(cursor.node().kind(), "code_fence_content");

    // Get the full text from code_fence_content node itself, not just the first child
    let code_fence_node = cursor.node();
    let text = code_fence_node.utf8_text(src.as_bytes()).ok()?;

    // Navigate to first text child to get its descendant_index
    if !cursor.goto_first_child() || cursor.node().kind() != "text" {
        return None;
    }

    let body = (text.to_string(), cursor.descendant_index());

    Some((language, body))
}

/// Walk from a list_item node to its content paragraph.
///
/// Moves the cursor from a list_item through the list_marker to the paragraph node.
///
/// # Tree Structure
///
/// ```ansi
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

/// Count the number of siblings a node has.
pub fn count_siblings(cursor: &TreeCursor) -> usize {
    let mut cursor = cursor.clone();
    let mut count = 0;
    while cursor.goto_next_sibling() {
        count += 1;
    }
    count
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
    use crate::mdschema::validator::utils::parse_markdown_and_get_tree;

    use super::*;

    #[test]
    fn test_is_codeblock() {
        // Without language, 3 backticks
        let input = "```\ncode\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert!(is_codeblock(&cursor.node()));

        // With language, 3 backticks
        let input = "```rust\ncode\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert!(is_codeblock(&cursor.node()));

        // Without language, 4 backticks
        let input = "````\ncode\n````\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert!(is_codeblock(&cursor.node()));

        // With language, 4 backticks
        let input = "````rust\ncode\n````\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert!(is_codeblock(&cursor.node()));
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
}
