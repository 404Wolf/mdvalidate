use crate::mdschema::validator::validator::Validator;
use ariadne::{Color, Label, Report, ReportKind, Source};
use std::fmt;
use std::io;

use crate::mdschema::validator::matcher::Error as MatcherError;
use crate::mdschema::validator::utils::find_node_by_index;

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {}", e),
            Error::Utf8(e) => write!(f, "UTF-8 error: {}", e),
            Error::InvalidRegex(e) => write!(f, "Invalid regex: {}", e),
            Error::SchemaViolation(e) => write!(f, "Schema violation: {:?}", e),
            Error::SchemaError(e) => write!(f, "Schema error: {:?}", e),
            Error::ParserError(e) => write!(f, "Parser error: {:?}", e),
            Error::InvalidMatcherFormat(s) => write!(f, "Invalid matcher format: {}", s),
            Error::ValidatorCreationFailed => write!(f, "Validator creation failed"),
            Error::ReadInputFailed(s) => write!(f, "Failed to read input: {}", s),
            Error::PrettyPrintFailed(s) => write!(f, "Failed to pretty print: {}", s),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(_: std::str::Utf8Error) -> Self {
        Error::Utf8(String::from_utf8(vec![]).unwrap_err().to_string())
    }
}

impl From<regex::Error> for Error {
    fn from(e: regex::Error) -> Self {
        Error::InvalidRegex(e.to_string())
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Error {
    Io(String),
    Utf8(String),
    InvalidRegex(String),
    SchemaViolation(SchemaViolationError),
    SchemaError(SchemaError),
    ParserError(ParserError),
    InvalidMatcherFormat(String),
    ValidatorCreationFailed,
    ReadInputFailed(String),
    PrettyPrintFailed(String),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ParserError {
    ReadAfterGotEOF,
    InvalidUTF8,
    TreesitterError,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaError {
    /// Missing a matcher in a matcher group
    MissingMatcher {
        schema_index: usize,
        input_index: usize,
    },
    /// When a node has multiple matchers in its children, which is not allowed.
    MultipleMatchersInNodeChildren {
        schema_index: usize,
        input_index: usize,
        /// Number of matchers received
        received: usize,
    },
    /// When you attempt to validate a list node, but the schema has a non
    /// repeated matcher.
    BadListMatcher {
        schema_index: usize,
        input_index: usize,
    },
    /// When you attempt to make a matcher but the interior contents are
    /// invalid. For example, `////foobar/bad matcher!`.
    InvalidMatcherContents {
        schema_index: usize,
        input_index: usize,
    },
    /// When you create a matcher and don't close it.
    UnclosedMatcher {
        schema_index: usize,
        input_index: usize,
    },
    /// When we construct a matcher and encounter an error.
    MatcherError {
        error: MatcherError,
        schema_index: usize,
        input_index: usize,
    },
    /// When the corresponding part of the schema text is not valid UTF-8.
    UTF8Error {
        schema_index: usize,
        input_index: usize,
    },
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum NodeContentMismatchKind {
    Suffix,
    Matcher,
    Prefix,
    Literal,
}

impl fmt::Display for NodeContentMismatchKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NodeContentMismatchKind::Suffix => write!(f, "suffix"),
            NodeContentMismatchKind::Matcher => write!(f, "matcher"),
            NodeContentMismatchKind::Prefix => write!(f, "prefix"),
            NodeContentMismatchKind::Literal => write!(f, "literal"),
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaViolationError {
    /// Mismatch between schema definition and actual node.
    NodeTypeMismatch {
        schema_index: usize,
        input_index: usize,
        /// The expected node type that doesn't validate
        ///
        /// TODO: Make this a enum value rather than the raw treesitter node
        /// type string.
        expected: String,
        /// Actual node type is obtainable from the input tree
        ///
        /// TODO: Make this a enum value rather than the raw treesitter node
        /// type string.
        actual: String,
    },
    /// Text content of node does not match expected value.
    NodeContentMismatch {
        schema_index: usize,
        input_index: usize,
        /// The expected text that doesn't validate
        expected: String,
        /// Actual content is obtainable from the input tree
        actual: String,
        /// The type of node content mismatch
        kind: NodeContentMismatchKind,
    },
    /// When it looks like you meant to have a repeating list node, but there is
    /// no {} to indicate repeating.
    NonRepeatingMatcherInListContext {
        schema_index: usize,
        input_index: usize,
    },
    /// Nodes have different numbers of children.
    ChildrenLengthMismatch {
        schema_index: usize,
        input_index: usize,
        /// Expected number of children
        expected: usize,
        /// Actual number of children
        actual: usize,
    },
    /// Nested list exceeds the maximum allowed depth.
    NodeListTooDeep {
        schema_index: usize,
        input_index: usize,
        /// Maximum depth allowed
        max_depth: usize,
    },
    /// List item count is outside the expected range.
    WrongListCount {
        schema_index: usize,
        input_index: usize,
        /// Minimum number of items (None means no limit)
        min: Option<usize>,
        /// Maximum number of items (None means no limit)
        max: Option<usize>,
        /// Actual number of items
        actual: usize,
    },
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
    error: &Error,
    validator: &Validator,
    filename: &str,
) -> Result<String, String> {
    let source_content = validator.last_input_str();
    let tree = &validator.input_tree;

    let mut buffer = Vec::new();

    match error {
        Error::Io(e) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("IO error")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!("IO error: {}", e))
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
        Error::Utf8(e) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("UTF-8 error")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!("UTF-8 error: {}", e))
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
        Error::InvalidRegex(e) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Invalid regex")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!("Invalid regex: {}", e))
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
        Error::InvalidMatcherFormat(msg) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Invalid matcher format")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!("Invalid matcher format: {}", msg))
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
        Error::ValidatorCreationFailed => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Validator creation failed")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message("Failed to create validator")
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
        Error::ReadInputFailed(msg) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Failed to read input")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!("Failed to read input: {}", msg))
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
        Error::PrettyPrintFailed(msg) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Failed to pretty print")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!("Failed to pretty print: {}", msg))
                        .with_color(Color::Red),
                )
                .finish()
                .write((filename, Source::from(source_content)), &mut buffer)
                .map_err(|e| e.to_string())?;
        }
        Error::SchemaViolation(schema_err) => match schema_err {
            SchemaViolationError::NodeTypeMismatch {
                schema_index: _,
                input_index,
                expected,
                actual,
            } => {
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let input_range = input_node.start_byte()..input_node.end_byte();

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("Node type mismatch")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(format!(
                                "Expected '{}' but found '{}'",
                                expected,
                                actual,
                            ))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::NodeContentMismatch {
                schema_index: _,
                input_index,
                expected,
                actual,
                kind,
            } => {
                let node = find_node_by_index(tree.root_node(), *input_index);
                let node_range = node.start_byte()..node.end_byte();

                Report::build(ReportKind::Error, (filename, node_range.clone()))
                    .with_message(format!("Node {} mismatch", kind))
                    .with_label(
                        Label::new((filename, node_range))
                            .with_message(format!(
                                "Expected {} '{}' but found '{}'",
                                kind, expected, actual
                            ))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::NonRepeatingMatcherInListContext {
                schema_index,
                input_index,
            } => {
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);
                let input_range = input_node.start_byte()..input_node.end_byte();

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("Non-repeating matcher in repeating context")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(format!(
                                "This matcher is in a list context but is not marked as repeating. Schema: '{}'",
                                schema_content
                            ))
                            .with_color(Color::Red),
                    )
                    .with_help(r#"
You can mark a list node as repeating by adding a '{<min_count>,<max_count>} directly after the matcher, like
- `myLabel:/foo/`{1,12}
"#)
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::ChildrenLengthMismatch {
                schema_index,
                input_index,
                expected,
                actual,
            } => {
                let parent = find_node_by_index(tree.root_node(), *input_index);
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);
                let parent_range = parent.start_byte()..parent.end_byte();

                let mut report = Report::build(ReportKind::Error, (filename, parent_range.clone()))
                    .with_message("Children length mismatch")
                    .with_label(
                        Label::new((filename, parent_range))
                            .with_message(format!(
                                "Expected {} children but found {}. Schema: '{}'",
                                expected, actual, schema_content
                            ))
                            .with_color(Color::Red),
                    );

                if parent.kind() == "list_item" {
                    report = report.with_help(
                        "If you want to allow any number of list items, use the {min,max} syntax \
                         (e.g., `item:/pattern/`{1,} or `item:/pattern/`{0,})",
                    );
                }

                report
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::NodeListTooDeep {
                schema_index,
                input_index,
                max_depth,
            } => {
                let node = find_node_by_index(tree.root_node(), *input_index);
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);
                let node_range = node.start_byte()..node.end_byte();

                Report::build(ReportKind::Error, (filename, node_range.clone()))
                    .with_message("Nested list exceeds maximum depth")
                    .with_label(
                        Label::new((filename, node_range))
                            .with_message(format!(
                                "List nesting exceeds maximum depth of {} level(s). Schema: '{}'",
                                max_depth, schema_content
                            ))
                            .with_color(Color::Red),
                    )
                    .with_help(
                        "For schemas like:\n\
                         - `num1:/\\d/`{1,}\n\
                         \u{20} - `num2:/\\d/`{1,}{1,}\n\
                         \n\
                         You may need to adjust the repetition for the first matcher\n\
                         to allow for the depth of the following ones. For example, you could\n\
                         make that `num1:/\\d/`{1,}{1,}{1,} to allow for three levels of nesting (the one \
                         below it, and the two allowed below that).",
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaViolationError::WrongListCount {
                schema_index,
                input_index,
                min,
                max,
                actual,
            } => {
                let node = find_node_by_index(tree.root_node(), *input_index);
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);
                let node_range = node.start_byte()..node.end_byte();

                let range_desc = match (min, max) {
                    (Some(min_val), Some(max_val)) => {
                        format!("between {} and {}", min_val, max_val)
                    }
                    (Some(min_val), None) => format!("at least {}", min_val),
                    (None, Some(max_val)) => format!("at most {}", max_val),
                    (None, None) => "any number of".to_string(),
                };

                let message = format!(
                    "Expected {} item(s) but found {}. Schema: '{}'",
                    range_desc, actual, schema_content
                );

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
        Error::SchemaError(schema_err) => match schema_err {
            SchemaError::MultipleMatchersInNodeChildren {
                schema_index: _,
                input_index,
                received: received_count,
            } => {
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let input_range = input_node.start_byte()..input_node.end_byte();

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("Multiple matchers in node children")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(format!(
                                "{} matchers found in node children",
                                received_count
                            ))
                            .with_color(Color::Red),
                    )
                    .with_help("Only one matcher is allowed per node's children.")
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaError::BadListMatcher {
                schema_index,
                input_index,
            } => {
                let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let input_range = input_node.start_byte()..input_node.end_byte();

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("Bad list matcher")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(format!(
                                "No matchers found in children of list node (node kind: '{}'). Schema: '{}'",
                                schema_node.kind(),
                                schema_content
                            ))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaError::InvalidMatcherContents {
                schema_index,
                input_index,
            } => {
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let input_range = input_node.start_byte()..input_node.end_byte();

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("Invalid matcher contents")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(format!(
                                "Invalid matcher contents. Schema: '{}'",
                                schema_content
                            ))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaError::UnclosedMatcher {
                schema_index,
                input_index,
            } => {
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let input_range = input_node.start_byte()..input_node.end_byte();

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("Unclosed matcher")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(format!("Unclosed matcher. Schema: '{}'", schema_content))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaError::MatcherError {
                error,
                schema_index,
                input_index,
            } => {
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let input_range = input_node.start_byte()..input_node.end_byte();
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("Matcher error")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(format!(
                                "Matcher error: {:?}. Schema: '{}'",
                                error, schema_content
                            ))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaError::UTF8Error {
                schema_index,
                input_index,
            } => {
                let schema_content =
                    node_content_by_index_or(tree.root_node(), *schema_index, source_content);
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let input_range = input_node.start_byte()..input_node.end_byte();

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("UTF-8 error in schema")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(format!(
                                "Schema text at this position is not valid UTF-8. Schema: '{}'",
                                schema_content
                            ))
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((filename, Source::from(source_content)), &mut buffer)
                    .map_err(|e| e.to_string())?;
            }
            SchemaError::MissingMatcher { schema_index, input_index } => todo!(),
        },
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
