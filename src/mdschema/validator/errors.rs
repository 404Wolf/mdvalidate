use crate::mdschema::validator::{matcher::matcher::*, validator::Validator};
use ariadne::{Color, Label, Report, ReportKind, Source};
use std::fmt;

use crate::mdschema::validator::ts_utils::find_node_by_index;

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ValidationError::SchemaViolation(e) => write!(f, "Schema violation: {:?}", e),
            ValidationError::SchemaError(e) => write!(f, "Schema error: {:?}", e),
            ValidationError::InternalInvariantViolated(msg) => {
                write!(f, "Internal invariant violated: {}. This is a bug.", msg)
            }
            ValidationError::IoError(e) => write!(f, "IO error: {}", e),
            ValidationError::InvalidUTF8 => write!(f, "Invalid UTF-8"),
            ValidationError::ParserError(e) => write!(f, "Parser error: {:?}", e),
            ValidationError::ValidatorCreationFailed => write!(f, "Validator creation failed"),
        }
    }
}

/// Error that happens during input parsing or processing.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ValidationError {
    /// Error when reading input.
    IoError(String),

    /// When we attempt to read a byte from the input, but the input is not valid UTF-8.
    InvalidUTF8,

    /// When we find a violation when validating the input against the schema.
    SchemaViolation(SchemaViolationError),

    /// When the actual schema is invalid.
    SchemaError(SchemaError),

    /// Some internal invariant was violated.
    InternalInvariantViolated(String),

    /// Error that happens during input parsing or processing.
    ParserError(ParserError),

    /// Failed to create validator.
    ValidatorCreationFailed,
}

/// Error that happens during input parsing or processing.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ParserError {
    /// When we attempt to read again after we already reached EOF.
    ///
    /// This is an internal error and should never happen.
    ReadAfterGotEOF,

    /// Failed to read input.
    ///
    /// TODO: do we really need this?
    ReadInputFailed(String),

    /// Error given to us from the treesitter parser.
    TreesitterError,

    /// Internal error when we fail to create a validator.
    ///
    /// TODO: add a nested enum so we get more context here.
    ValidatorCreationFailed,

    /// Failed to pretty print the error message.
    PrettyPrintFailed(String),
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
    /// When you attempt to make a matcher but the extras are
    /// invalid. For example, `test:/1/`!{1,2}.
    InvalidMatcherExtras {
        schema_index: usize,
        input_index: usize,
        error: MatcherExtrasError,
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
    /// When you have a repeating matcher, followed by another repeating
    /// matcher, and the first repeating matcher is not bounded to a specific
    /// number of nodes.
    ///
    /// In cases where you have two or more repeating matchers in a row, the
    /// first ones must have a specific number of nodes.
    ///
    /// For example, this is fine:
    ///
    /// Input:
    ///
    /// ```md
    /// - test
    /// - bar
    /// - bar
    /// ```
    ///
    /// Schema:
    ///
    /// ```md
    /// - `name1:/test/`{1,1}
    /// - `name2:/bar/`{,}
    /// ```
    ///
    /// But this is not, and will result in this error:
    ///
    /// Input:
    ///
    /// ```md
    /// - test
    /// - test
    /// - test
    /// - test
    /// - bar
    /// - bar
    /// ```
    ///
    /// Schema:
    ///
    /// ```md
    /// - `name1:/test/`{,}
    /// - `name2:/bar/`{,2}
    /// ```
    RepeatingMatcherUnbounded {
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
        expected: ChildrenCount,
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
pub enum ChildrenCount {
    SpecificCount(usize),
    Range { min: usize, max: Option<usize> },
}

impl fmt::Display for ChildrenCount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ChildrenCount::SpecificCount(count) => write!(f, "{}", count),
            ChildrenCount::Range { min, max } => match max {
                Some(max_val) => write!(f, "between {} and {}", min, max_val),
                None => write!(f, "at least {}", min),
            },
        }
    }
}

impl ChildrenCount {
    pub fn from_specific(count: usize) -> Self {
        ChildrenCount::SpecificCount(count)
    }

    pub fn from_range(min: usize, max: Option<usize>) -> Self {
        ChildrenCount::Range { min, max }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum NodeContentMismatchError {
    /// A node's text content doesn't match expected literal text
    Text(String),
    /// A matcher's pattern doesn't match
    Matcher(usize),
}

#[derive(Debug)]
pub enum PrettyPrintError {
    FailedToPrettyPrint(String),
    UTF8Error(String),
}

impl From<std::str::Utf8Error> for PrettyPrintError {
    fn from(err: std::str::Utf8Error) -> Self {
        PrettyPrintError::UTF8Error(err.to_string())
    }
}

impl From<String> for PrettyPrintError {
    fn from(err: String) -> Self {
        PrettyPrintError::FailedToPrettyPrint(err)
    }
}

impl From<&str> for PrettyPrintError {
    fn from(err: &str) -> Self {
        PrettyPrintError::FailedToPrettyPrint(err.to_string())
    }
}

/// Pretty prints an Error using [ariadne](https://github.com/zesterer/ariadne).
pub fn pretty_print_error(
    error: &ValidationError,
    validator: &Validator,
    filename: &str,
) -> Result<String, PrettyPrintError> {
    let mut buffer = Vec::new();
    validation_error_to_ariadne(error, validator, filename, &mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn validation_error_to_ariadne(
    error: &ValidationError,
    validator: &Validator,
    filename: &str,
    buffer: &mut Vec<u8>,
) -> Result<(), PrettyPrintError> {
    let source_content = validator.last_input_str();
    let tree = &validator.input_tree;

    let report = match error {
        ValidationError::SchemaViolation(schema_err) => match schema_err {
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
                            .with_message(
                                format!("Expected '{}' but found '{}'", expected, actual,),
                            )
                            .with_color(Color::Red),
                    )
                    .finish()
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
            }
            SchemaViolationError::NonRepeatingMatcherInListContext {
                schema_index,
                input_index,
            } => {
                let input_node = find_node_by_index(tree.root_node(), *input_index);
                let schema_content =
                    node_content_by_index(tree.root_node(), *schema_index, source_content)?;
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
            }
            SchemaViolationError::ChildrenLengthMismatch {
                schema_index,
                input_index,
                expected,
                actual,
            } => {
                let parent = find_node_by_index(tree.root_node(), *input_index);
                let schema_content =
                    node_content_by_index(tree.root_node(), *schema_index, source_content)?;
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

                report.finish()
            }
            SchemaViolationError::NodeListTooDeep {
                schema_index,
                input_index,
                max_depth,
            } => {
                let node = find_node_by_index(tree.root_node(), *input_index);
                let schema_content =
                    node_content_by_index(tree.root_node(), *schema_index, source_content)?;
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
                    node_content_by_index(tree.root_node(), *schema_index, source_content)?;
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
            }
        },
        ValidationError::SchemaError(schema_err) => {
            match schema_err {
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
                }
                SchemaError::BadListMatcher {
                    schema_index,
                    input_index,
                } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_content =
                        node_content_by_index(tree.root_node(), *schema_index, source_content)?;
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
                }
                SchemaError::UnclosedMatcher {
                    schema_index,
                    input_index,
                } => {
                    let schema_content =
                        node_content_by_index(tree.root_node(), *schema_index, source_content)?;
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
                }
                SchemaError::MatcherError {
                    error,
                    schema_index,
                    input_index,
                } => {
                    let input_node = find_node_by_index(tree.root_node(), *input_index);
                    let input_range = input_node.start_byte()..input_node.end_byte();
                    let schema_content =
                        node_content_by_index(tree.root_node(), *schema_index, source_content)?;

                    Report::build(ReportKind::Error, (filename, input_range.clone()))
                        .with_message("Matcher error")
                        .with_label(
                            Label::new((filename, input_range))
                                .with_message(format!(
                                    "Matcher error: {}. Schema: '{}'",
                                    error, schema_content
                                ))
                                .with_color(Color::Red),
                        )
                        .finish()
                }
                SchemaError::UTF8Error {
                    schema_index,
                    input_index,
                } => {
                    let schema_content =
                        node_content_by_index(tree.root_node(), *schema_index, source_content)?;
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
                }
                SchemaError::MissingMatcher {
                    schema_index,
                    input_index,
                } => {
                    let input_node = find_node_by_index(tree.root_node(), *input_index);
                    let input_range = input_node.start_byte()..input_node.end_byte();
                    let schema_content =
                        node_content_by_index(tree.root_node(), *schema_index, source_content)?;

                    Report::build(ReportKind::Error, (filename, input_range.clone()))
                        .with_message("Missing matcher")
                        .with_label(
                            Label::new((filename, input_range))
                                .with_message(format!(
                                    "Missing matcher in matcher group. Schema: '{}'",
                                    schema_content
                                ))
                                .with_color(Color::Red),
                        )
                        .finish()
                }
                SchemaError::InvalidMatcherExtras {
                    schema_index,
                    input_index,
                    error,
                } => {
                    let schema_content =
                        node_content_by_index(tree.root_node(), *schema_index, source_content)?;
                    let input_node = find_node_by_index(tree.root_node(), *input_index);
                    let input_range = input_node.start_byte()..input_node.end_byte();

                    Report::build(ReportKind::Error, (filename, input_range.clone()))
                        .with_message("Invalid matcher extras")
                        .with_label(
                            Label::new((filename, input_range))
                                .with_message(format!(
                                    "Invalid matcher extras: {}. Schema: '{}'",
                                    error, schema_content
                                ))
                                .with_color(Color::Red),
                        )
                        .finish()
                }
                SchemaError::RepeatingMatcherUnbounded {
                    schema_index,
                    input_index,
                } => {
                    let schema_content =
                        node_content_by_index(tree.root_node(), *schema_index, source_content)?;
                    let input_node = find_node_by_index(tree.root_node(), *input_index);
                    let input_range = input_node.start_byte()..input_node.end_byte();

                    Report::build(ReportKind::Error, (filename, input_range.clone()))
                        .with_message("Unbounded repeating matcher must be last")
                        .with_label(
                            Label::new((filename, input_range))
                                .with_message(format!(
                                    "This unbounded repeating matcher is followed by other repeating matchers. Schema: '{}'",
                                    schema_content
                                ))
                                .with_color(Color::Red),
                        )
                        .with_help(
                            "When you have multiple repeating matchers in a row, all but the last one \
                         must have a specific upper bound. For example:\n\
                         - `name1:/test/`{{1,3}}\n\
                         - `name2:/bar/`{{,}}\n\
                         \n\
                         The first matcher has a specific upper bound (3), while the last one can be unbounded.",
                        )
                        .finish()
                }
            }
        },
        ValidationError::InternalInvariantViolated(msg) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Internal invariant violated")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!(
                            "Internal invariant violated: {}. This is a bug.",
                            msg
                        ))
                        .with_color(Color::Red),
                )
                .finish()
        }
        ValidationError::IoError(msg) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("IO error")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!("IO error: {}", msg))
                        .with_color(Color::Red),
                )
                .finish()
        }
        ValidationError::InvalidUTF8 => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Invalid UTF-8")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message("Input contains invalid UTF-8")
                        .with_color(Color::Red),
                )
                .finish()
        }
        ValidationError::ParserError(parser_err) => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Parser error")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message(format!("Parser error: {:?}", parser_err))
                        .with_color(Color::Red),
                )
                .finish()
        }
        ValidationError::ValidatorCreationFailed => {
            let root_range = 0..source_content.len();
            Report::build(ReportKind::Error, (filename, root_range.clone()))
                .with_message("Validator creation failed")
                .with_label(
                    Label::new((filename, root_range))
                        .with_message("Failed to create validator")
                        .with_color(Color::Red),
                )
                .finish()
        }
    };

    report
        .write((filename, Source::from(source_content)), buffer)
        .map_err(|e| PrettyPrintError::from(e.to_string()))?;

    Ok(())
}

/// Find a node's content by its index given by a cursor's .descendant_index().
fn node_content_by_index<'a>(
    root: tree_sitter::Node<'a>,
    target_index: usize,
    source_content: &'a str,
) -> Result<&'a str, std::str::Utf8Error> {
    let node = find_node_by_index(root, target_index);
    node.utf8_text(source_content.as_bytes())
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::ts_utils::new_markdown_parser;

    use super::*;

    #[test]
    fn test_node_content_by_index() {
        let source = "# Heading\n\nThis is a paragraph.";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(source, None).unwrap();
        let root = tree.root_node();

        let heading_content = node_content_by_index(root, 3, source);
        assert_eq!(heading_content.unwrap(), " Heading");
    }
}
