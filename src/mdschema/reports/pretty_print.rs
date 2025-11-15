use crate::mdschema::reports::errors::{Error, SchemaViolationError};
use ariadne::{Color, Label, Report, ReportKind, Source};
use tree_sitter::Tree;

/// Pretty prints an Error using [ariadne](https://github.com/zesterer/ariadne).
pub fn pretty_print_error(
    tree: Tree,
    error: &Error,
    source_content: &str,
    filename: &str,
) -> Result<String, String> {
    let (node_index, message) = extract_error_info(&tree, source_content, error);
    let error_node = find_node_by_index(tree.root_node(), node_index);
    let range = error_node.start_byte()..error_node.end_byte();

    let mut buffer = Vec::new();
    Report::build(ReportKind::Error, (filename, range.clone()))
        .with_message(&message)
        .with_label(
            Label::new((filename, range))
                .with_message(message)
                .with_color(Color::Red),
        )
        .finish()
        .write((filename, Source::from(source_content)), &mut buffer)
        .map_err(|e| e.to_string())?;

    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn extract_error_info(tree: &Tree, source_content: &str, error: &Error) -> (usize, String) {
    match error {
        Error::SchemaViolation(schema_err) => match schema_err {
            SchemaViolationError::NodeTypeMismatch(expected, actual) => (
                *actual,
                format!(
                    "Node type mismatch: expected node {} but found node {}",
                    expected, actual
                ),
            ),
            SchemaViolationError::NodeContentMismatch(node_id, expected) => {
                let actual = node_content_by_index_or(tree.root_node(), *node_id, source_content);

                (
                    *node_id,
                    format!(
                        "Node content mismatch: expected '{}' but found '{}'",
                        expected, actual
                    ),
                )
            }
            SchemaViolationError::ChildrenLengthMismatch(expected, actual) => (
                0,
                format!(
                    "Children length mismatch: expected {} but found {} children",
                    expected, actual
                ),
            ),
        },
        Error::ParserError(_) => (0, "Parser error occurred".to_string()),
        Error::SchemaError(_) => (0, "Schema error occurred".to_string()),
    }
}

fn find_node_by_index(root: tree_sitter::Node, target_index: usize) -> tree_sitter::Node {
    let mut cursor = root.walk();
    cursor.goto_descendant(target_index);
    cursor.node()
}

fn node_content_by_index_or<'a>(
    root: tree_sitter::Node<'a>,
    target_index: usize,
    source_content: &'a str,
) -> &'a str {
    let node = find_node_by_index(root, target_index);
    node.utf8_text(source_content.as_bytes()).unwrap_or("n/a")
}
