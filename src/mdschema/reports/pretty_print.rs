use crate::mdschema::reports::errors::{Error, SchemaViolationError};
use ariadne::{Color, Label, Report, ReportKind, Source};
use tree_sitter::Tree;

/// Pretty prints an Error using
/// [ariadne](https://github.com/zesterer/ariadne) for nice formatting.
///
/// Returns a String containing the formatted report, or an error message if
/// formatting fails with a message.
pub fn pretty_print_error<'a>(
    tree: Tree,
    error: &Error,
    source_content: &str,
    filename: &str,
) -> Result<String, String> {
    // Extract node index from the error
    let node_index = match error {
        Error::SchemaViolation(schema_err) => match schema_err {
            SchemaViolationError::NodeTypeMismatch(_, actual_id) => *actual_id,
            SchemaViolationError::NodeContentMismatch(node_id, _) => *node_id,
            SchemaViolationError::MultipleMatchers(_) => 0, // Use root node for this case
            SchemaViolationError::ChildrenLengthMismatch(_, _) => 0, // Use root node for this case
        },
        Error::ParserError(_) => 0, // Use root node for parser errors
    };

    // Get the node at the error position using the node index
    let error_node = {
        let root = tree.root_node();
        // Find the node by index through tree traversal
        fn find_node_by_index<'a>(node: tree_sitter::Node<'a>, target_index: usize, current_index: &mut usize) -> Option<tree_sitter::Node<'a>> {
            if *current_index == target_index {
                return Some(node);
            }
            *current_index += 1;
            
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if let Some(found) = find_node_by_index(child, target_index, current_index) {
                    return Some(found);
                }
            }
            None
        }
        
        let mut index = 0;
        find_node_by_index(root, node_index, &mut index).unwrap_or(root)
    };

    let (message, report_kind, color, byte_start, byte_end) = match error {
        Error::SchemaViolation(schema_err) => match schema_err {
            SchemaViolationError::NodeTypeMismatch(expected_id, actual_id) => (
                format!(
                    "Node type mismatch: expected node {} but found node {}",
                    expected_id, actual_id
                ),
                ReportKind::Error,
                Color::Red,
                error_node.start_byte(),
                error_node.end_byte(),
            ),
            SchemaViolationError::NodeContentMismatch(_node_id, expected) => (
                format!(
                    "Node content mismatch: expected '{}' but found different content",
                    expected
                ),
                ReportKind::Error,
                Color::Red,
                error_node.start_byte(),
                error_node.end_byte(),
            ),
            SchemaViolationError::MultipleMatchers(count) => (
                format!("Multiple matchers found ({} matchers)", count),
                ReportKind::Error,
                Color::Red,
                error_node.start_byte(),
                error_node.end_byte(),
            ),
            SchemaViolationError::ChildrenLengthMismatch(expected, actual) => (
                format!(
                    "Children length mismatch: expected {} children but found {} children",
                    expected, actual
                ),
                ReportKind::Error,
                Color::Red,
                error_node.start_byte(),
                error_node.end_byte(),
            ),
        },
        Error::ParserError(_) => (
            "Parser error occurred".to_string(),
            ReportKind::Error,
            Color::Red,
            0,
            0,
        ),
    };

    let mut buffer = Vec::new();
    Report::build(report_kind, (filename, byte_start..byte_end))
        .with_message(message.clone())
        .with_label(
            Label::new((filename, byte_start..byte_end))
                .with_message(message)
                .with_color(color),
        )
        .finish()
        .write((filename, Source::from(source_content)), &mut buffer)
        .map_err(|e| e.to_string())?;

    Ok(String::from_utf8_lossy(&buffer).to_string())
}
