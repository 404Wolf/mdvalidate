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
    /// When a node has multiple matchers in its children, which is not allowed.
    /// Has node index and number of matchers.
    MultipleMatchersInNodeChildren(usize, usize),
    /// When you call `validate_matcher_node_list` with a schema node whose
    /// children contain no matchers, which should never happen.
    NoMatcherInListNodeChildren(usize),
    /// When you create a matcher and don't close it.
    UnclosedMatcher(usize),
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
    /// Nested list exceeds the maximum allowed depth. Max depth allowed, node index of deepest list
    NodeListTooDeep(usize, usize),
    /// List item count is outside the expected range. (min, max, actual, node_index)
    /// min and max are Option<usize> where None means no limit
    WrongListCount(Option<usize>, Option<usize>, usize, usize),
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

                let mut report = Report::build(ReportKind::Error, (filename, parent_range.clone()))
                    .with_message("Children length mismatch")
                    .with_label(
                        Label::new((filename, parent_range))
                            .with_message(format!(
                                "Expected {} children but found {}",
                                expected, actual
                            ))
                            .with_color(Color::Red),
                    );

                if parent.kind() == "list_item" {
                    report = report.with_help(
                        "If you want to allow any number of list items, add a '+' after the matcher \
                         (e.g., `item:/pattern/`+)",
                    );
                }

                report
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::NodeListTooDeep(max_depth, node_index) => {
                let node = find_node_by_index(tree.root_node(), *node_index);
                let node_range = node.start_byte()..node.end_byte();

                Report::build(ReportKind::Error, (filename, node_range.clone()))
                    .with_message("Nested list exceeds maximum depth")
                    .with_label(
                        Label::new((filename, node_range))
                            .with_message(format!(
                                "List nesting exceeds maximum depth of {} level(s)",
                                max_depth
                            ))
                            .with_color(Color::Red),
                    )
                    .with_help(
                        "For schemas like:\n\
                         - `num1:/\\d/`+\n\
                         \u{20} - `num2:/\\d/`++\n\
                         \n\
                         You may need to adjust the number of '+' signs for the first matcher\n\
                         to allow for the depth of the following ones. For example, you could\n\
                         make that `num1:/\\d/`+++ to allow for three levels of nesting (the one
                         below it, and the two allowed below that).",
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::WrongListCount(min, max, actual, node_index) => {
                let node = find_node_by_index(tree.root_node(), *node_index);
                let node_range = node.start_byte()..node.end_byte();

                let range_desc = match (min, max) {
                    (Some(min_val), Some(max_val)) => {
                        format!("between {} and {}", min_val, max_val)
                    }
                    (Some(min_val), None) => format!("at least {}", min_val),
                    (None, Some(max_val)) => format!("at most {}", max_val),
                    (None, None) => "any number of".to_string(),
                };

                let message = format!("Expected {} item(s) but found {}", range_desc, actual);

                Report::build(ReportKind::Error, (filename, node_range.clone()))
                    .with_message("List item count mismatch")
                    .with_label(
                        Label::new((filename, node_range))
                            .with_message(message)
                            .with_color(Color::Red),
                    )
                    .with_help(
                        "The number of items in `matcher`{1,2} syntax refers to the number of \
                         entries at the level of that matcher (deeper items are not included in \
                         that count).",
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
                SchemaError::MultipleMatchersInNodeChildren(node_id, count) => {
                    format!(
                        "Multiple matchers ({}) found in node children at index {}",
                        count, node_id
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
                SchemaError::UnclosedMatcher(node_id) => {
                    format!("Unclosed matcher at index {}", node_id)
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
