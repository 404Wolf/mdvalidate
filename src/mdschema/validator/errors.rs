use ariadne::{Color, Label, Report, ReportKind, Source};
use tree_sitter::Tree;

use crate::mdschema::validator::utils::find_node_by_index;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Error {
    SchemaViolation(SchemaViolationError),
    SchemaError(SchemaError),
    ParserError(ParserError),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ParserError {
    ReadAfterGotEOF,
    InvalidUTF8,
    TreesitterError,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaError {
    MultipleMatchersInNodeChildren(usize),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaViolationError {
    /// Mismatch between schema definition and actual node
    NodeTypeMismatch(usize, usize),
    /// Text content of node does not match expected value
    NodeContentMismatch(usize, String),
    /// Nodes have different numbers of children
    ChildrenLengthMismatch(usize, usize),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum NodeContentMismatchError {
    /// A node's text content doesn't match expected literal text
    Text(String),
    /// A matcher's pattern doesn't match
    Matcher(usize),
}

/// Pretty prints an Error using [ariadne](https://github.com/zesterer/ariadne).
pub fn pretty_print_error(
    tree: &Tree,
    error: &Error,
    source_content: &str,
    filename: &str,
) -> Result<String, String> {
    let (node_index, message) = match error {
        Error::SchemaViolation(schema_err) => match schema_err {
            SchemaViolationError::NodeTypeMismatch(expected_id, actual_id) => {
                let expected = find_node_by_index(tree.root_node(), *expected_id);
                let actual = find_node_by_index(tree.root_node(), *actual_id);

                (
                    *actual_id,
                    format!(
                        "Node type mismatch: expected '{}' but found '{}'",
                        expected.kind(),
                        actual.kind()
                    ),
                )
            }
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
    };

    let error_node = find_node_by_index(tree.root_node(), node_index);
    let range = error_node.start_byte()..error_node.end_byte();

    let mut buffer = Vec::new();
    Report::build(ReportKind::Error, (filename, range.clone()))
        .with_message(&message)
        .with_label(
            Label::new((filename, range))
                .with_message(&message)
                .with_color(Color::Red),
        )
        .finish()
        .write((filename, Source::from(source_content)), &mut buffer)
        .map_err(|e| e.to_string())?;

    Ok(String::from_utf8_lossy(&buffer).to_string())
}

/// Find a node's content by its index given by a cursor's .descendant_index().
fn node_content_by_index_or<'a>(
    root: tree_sitter::Node<'a>,
    target_index: usize,
    source_content: &'a str,
) -> &'a str {
    let node = find_node_by_index(root, target_index);
    node.utf8_text(source_content.as_bytes()).unwrap_or("n/a")
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::utils::new_markdown_parser;

    use super::*;

    #[test]
    fn test_node_content_by_index_or() {
        let source = "# Heading\n\nThis is a paragraph.";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(source, None).unwrap();
        let root = tree.root_node();

        let heading_content = node_content_by_index_or(root, 3, source);
        assert_eq!(heading_content, " Heading");
    }
}
