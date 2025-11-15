use tree_sitter::Parser;

#[allow(dead_code)]
pub fn node_to_str(node: tree_sitter::Node, input_str: &str) -> String {
    let mut cursor = node.walk();
    node_to_str_rec(&mut cursor, input_str, 0)
}

#[allow(dead_code)]
fn node_to_str_rec(cursor: &mut tree_sitter::TreeCursor, input_str: &str, depth: usize) -> String {
    let node = cursor.node();
    let indent = "  ".repeat(depth);
    let mut result = format!(
        "{}{}[{}..{}]({})",
        indent,
        node.kind(),
        node.byte_range().start,
        node.byte_range().end,
        cursor.descendant_index()
    );

    if node.child_count() == 0 {
        let text = &input_str[node.byte_range()];
        result.push_str(&format!(": {:?}", text));
    }

    result.push('\n');

    if cursor.goto_first_child() {
        loop {
            result.push_str(&node_to_str_rec(cursor, input_str, depth + 1));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }

    result
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
