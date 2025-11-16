use ariadne::{Color, Label, Report, ReportKind, Source};
use std::fmt;
use std::io;
use tree_sitter::Tree;

use crate::mdschema::validator::utils::find_node_by_index;

#[derive(Debug)]
pub enum ValidationError {
    Io(io::Error),
    Utf8(std::string::FromUtf8Error),
    InvalidRegex(regex::Error),
    InvalidMatcherFormat(String),
    ValidatorCreationFailed,
    ReadInputFailed(Error),
    PrettyPrintFailed(String),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::Io(e) => write!(f, "IO error: {}", e),
            ValidationError::Utf8(e) => write!(f, "UTF-8 error: {}", e),
            ValidationError::InvalidRegex(e) => write!(f, "Invalid regex: {}", e),
            ValidationError::InvalidMatcherFormat(msg) => {
                write!(f, "Invalid matcher format: {}", msg)
            }
            ValidationError::ValidatorCreationFailed => write!(f, "Failed to create validator"),
            ValidationError::ReadInputFailed(e) => write!(f, "Failed to read input: {:?}", e),
            ValidationError::PrettyPrintFailed(e) => write!(f, "Error generating report: {}", e),
        }
    }
}

impl std::error::Error for ValidationError {}

impl From<io::Error> for ValidationError {
    fn from(e: io::Error) -> Self {
        ValidationError::Io(e)
    }
}

impl From<std::str::Utf8Error> for ValidationError {
    fn from(_: std::str::Utf8Error) -> Self {
        ValidationError::Utf8(String::from_utf8(vec![]).unwrap_err())
    }
}

impl From<regex::Error> for ValidationError {
    fn from(e: regex::Error) -> Self {
        ValidationError::InvalidRegex(e)
    }
}

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
    /// When you call `validate_matcher_node_list` with a schema node whose
    /// children contain no matchers, which should never happen.
    NoMatcherInListNodeChildren(usize),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaViolationError {
    /// Mismatch between schema definition and actual node. Index of node 1 and index of node 2 that mismatch
    NodeTypeMismatch(usize, usize),
    /// Text content of node does not match expected value. Node index, text that doesn't validate
    NodeContentMismatch(usize, String),
    /// When it looks like you meant to have a repeating list node, but there is
    /// no "+" to indicate repeating. Index of the offending node.
    NonRepeatingMatcherInListContext(usize),
    /// Nodes have different numbers of children. Expected number, actual number, parent node index
    ChildrenLengthMismatch(usize, usize, usize),
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
    let mut buffer = Vec::new();

    match error {
        Error::SchemaViolation(schema_err) => match schema_err {
            SchemaViolationError::NodeTypeMismatch(expected_id, actual_id) => {
                let expected = find_node_by_index(tree.root_node(), *expected_id);
                let actual = find_node_by_index(tree.root_node(), *actual_id);
                let actual_range = actual.start_byte()..actual.end_byte();

                Report::build(ReportKind::Error, (filename, actual_range.clone()))
                    .with_message("Node type mismatch")
                    .with_label(
                        Label::new((filename, actual_range))
                            .with_message(format!(
                                "Expected '{}' but found '{}'",
                                expected.kind(),
                                actual.kind()
                            ))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::NodeContentMismatch(node_id, expected) => {
                let node = find_node_by_index(tree.root_node(), *node_id);
                let actual = node_content_by_index_or(tree.root_node(), *node_id, source_content);
                let node_range = node.start_byte()..node.end_byte();

                Report::build(ReportKind::Error, (filename, node_range.clone()))
                    .with_message("Node content mismatch")
                    .with_label(
                        Label::new((filename, node_range))
                            .with_message(format!("Expected '{}' but found '{}'", expected, actual))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::NonRepeatingMatcherInListContext(node_id) => {
                let node = find_node_by_index(tree.root_node(), *node_id);
                let node_range = node.start_byte()..node.end_byte();

                Report::build(ReportKind::Error, (filename, node_range.clone()))
                    .with_message("Non-repeating matcher in repeating context")
                    .with_label(
                        Label::new((filename, node_range))
                            .with_message(
                                "This matcher is in a list context but is not marked as repeating ('+')"
                            )
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::ChildrenLengthMismatch(expected, actual, parent_index) => {
                let parent = find_node_by_index(tree.root_node(), *parent_index);
                let parent_range = parent.start_byte()..parent.end_byte();

                Report::build(ReportKind::Error, (filename, parent_range.clone()))
                    .with_message("Children length mismatch")
                    .with_label(
                        Label::new((filename, parent_range))
                            .with_message(format!(
                                "Expected {} children but found {}",
                                expected, actual
                            ))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
        },
        Error::ParserError(parser_err) => {
            let root_range = 0..source_content.len();
            let message = match parser_err {
                ParserError::ReadAfterGotEOF => "Attempted to read after EOF",
                ParserError::InvalidUTF8 => "Invalid UTF-8 encountered",
                ParserError::TreesitterError => "Tree-sitter parsing error",
            };

            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Parser error")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(message)
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
        Error::SchemaError(schema_err) => {
            let root_range = 0..source_content.len();
            let message = match schema_err {
                SchemaError::MultipleMatchersInNodeChildren(node_id) => {
                    format!(
                        "Multiple matchers found in node children at index {}",
                        node_id
                    )
                }
                SchemaError::NoMatcherInListNodeChildren(node_id) => {
                    let actual_node = find_node_by_index(tree.root_node(), *node_id);
                    format!(
                        "No matchers found in children of list node at index {} (node kind: '{}')",
                        node_id,
                        actual_node.kind()
                    )
                }
            };
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Schema error")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(message)
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
    }

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
