use tree_sitter::Parser;

#[allow(dead_code)]
pub fn node_to_str(node: tree_sitter::Node, input_str: &str) -> String {
    node_to_str_rec(node, input_str, 0)
}

#[allow(dead_code)]
fn node_to_str_rec(node: tree_sitter::Node, input_str: &str, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let mut result = format!(
        "{}{}[{}..{}]",
        indent,
        node.kind(),
        node.byte_range().start,
        node.byte_range().end
    );

    if node.child_count() == 0 {
        let text = &input_str[node.byte_range()];
        result.push_str(&format!(": {:?}", text));
    }

    result.push('\n');

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            result.push_str(&node_to_str_rec(cursor.node(), input_str, depth + 1));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
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
