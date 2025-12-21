use serde_json::Value;
use tree_sitter::{Node, Parser, Tree, TreeCursor};
use tree_sitter_markdown::language;

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

/// Create a new Tree-sitter parser for Markdown.
pub fn new_markdown_parser() -> Parser {
    let mut parser = Parser::new();
    parser.set_language(&language()).unwrap();
    parser
}

/// Parse a markdown string into a Tree-sitter tree.
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

/// Chekc if a node is a heading node.
pub fn is_heading_node(node: &Node) -> bool {
    dbg!(&node.kind());
    node.kind().starts_with("atx_heading")
}

/// Determine whether the input is incomplete based on EOF status and last node.
///
/// The input is incomplete if we haven't reached the EOF and the cursor is at
/// the last node. Otherwise we're in the middle, we're not "incomplete."
pub fn waiting_at_end(got_eof: bool, last_input_str: &str, input_cursor: &TreeCursor) -> bool {
    !got_eof && is_last_node(last_input_str, &input_cursor.node())
}

/// Join two values together in-place.
pub fn join_values(a: &mut Value, b: Value) {
    match (a, b) {
        (Value::Object(ref mut existing_map), Value::Object(new_map)) => {
            for (key, value) in new_map {
                existing_map.insert(key, value);
            }
        }
        (Value::Array(ref mut existing_array), Value::Array(new_array)) => {
            existing_array.extend(new_array);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_markdown_and_get_tree(input: &str) -> Tree {
        let mut parser = new_markdown_parser();
        parser.parse(input, None).unwrap()
    }

    #[test]
    fn test_join_values_objects() {
        let mut a = serde_json::json!({ "key1": "value1" });
        let b = serde_json::json!({ "key2": "value2" });

        join_values(&mut a, b);

        if let Value::Object(ref map) = a {
            assert_eq!(map.get("key1"), Some(&Value::String("value1".to_string())));
            assert_eq!(map.get("key2"), Some(&Value::String("value2".to_string())));
        } else {
            panic!("a is not an object");
        }
    }

    #[test]
    fn test_join_values_arrays() {
        let mut a = Value::Array(vec![Value::String("value1".to_string())]);
        let b = Value::Array(vec![
            Value::String("value2".to_string()),
            Value::String("value3".to_string()),
        ]);

        join_values(&mut a, b);

        if let Value::Array(ref array) = a {
            assert_eq!(array.len(), 3);
            assert_eq!(array[0], Value::String("value1".to_string()));
            assert_eq!(array[1], Value::String("value2".to_string()));
            assert_eq!(array[2], Value::String("value3".to_string()));
        } else {
            panic!("a is not an array");
        }
    }

    #[test]
    fn test_is_heading_node() {
        // Test atx heading node
        let tree = parse_markdown_and_get_tree("# Heading");
        let root = tree.root_node();
        let heading_node = root.child(0).unwrap();
        assert!(is_heading_node(&heading_node));

        // Test non-heading node
        let tree = parse_markdown_and_get_tree("Paragraph text");
        let root = tree.root_node();
        let paragraph_node = root.child(0).unwrap();
        assert!(!is_heading_node(&paragraph_node));
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
}
