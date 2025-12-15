use tree_sitter::{Node, Parser, TreeCursor};

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
    parser
        .set_language(&tree_sitter_markdown::language())
        .unwrap();
    parser
}

/// Determine whether a given node is the last node in the tree.
pub fn is_last_node(input_str: &str, node: &tree_sitter::Node) -> bool {
    input_str.trim().len() == node.byte_range().end
}

/// Find a node by its index given by a cursor's .descendant_index().
pub fn find_node_by_index(root: tree_sitter::Node, target_index: usize) -> tree_sitter::Node {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_last_node() {
        let input = "Hello, world!";
        let mut parser = new_markdown_parser();

        let tree = parser.parse(input, None).unwrap();
        let root_node = tree.root_node();
        let last_child = root_node.child(root_node.named_child_count() - 1).unwrap();

        assert!(is_last_node(input, &last_child));
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
