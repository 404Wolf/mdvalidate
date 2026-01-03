use crate::mdschema::validator::{matcher::matcher::*, validator::Validator};
use ariadne::{Color, Label, Report, ReportKind, Source};
use std::fmt;

use crate::mdschema::validator::ts_utils::find_node_by_index;

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ValidationError::IoError(e) => write!(f, "IO error: {}", e),
            ValidationError::InvalidUTF8 => write!(f, "Invalid UTF-8 encoding in input"),
            ValidationError::SchemaViolation(e) => write!(f, "Schema violation: {}", e),
            ValidationError::SchemaError(e) => write!(f, "Schema error: {}", e),
            ValidationError::InternalInvariantViolated(msg) => {
                write!(f, "Internal invariant violated: {} (this is a bug)", msg)
            }
            ValidationError::ParserError(e) => write!(f, "Parser error: {}", e),
            ValidationError::ValidatorCreationFailed => write!(f, "Failed to create validator"),
        }
    }
}

/// Top-level error type for all validation operations.
///
/// This enum represents all possible errors that can occur during markdown validation,
/// from IO issues to schema violations to parser errors.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ValidationError {
    /// IO error occurred while reading input.
    IoError(String),

    /// Input contains invalid UTF-8 encoding.
    InvalidUTF8,

    /// Input violates the schema definition.
    SchemaViolation(SchemaViolationError),

    /// Schema definition itself is invalid or malformed.
    SchemaError(SchemaError),

    /// Internal invariant was violated (indicates a bug in the validator).
    InternalInvariantViolated(String),

    /// Parser failed to process input or schema.
    ParserError(ParserError),

    /// Failed to create or initialize the validator.
    ValidatorCreationFailed,
}

/// Errors that occur during parsing of input or schema.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ParserError {
    /// Attempted to read after already reaching end of file.
    ///
    /// This is an internal error and should never happen in normal operation.
    ReadAfterEOF,

    /// Failed to read input data.
    ReadInputFailed(String),

    /// Tree-sitter parser encountered an error.
    TreesitterError,

    /// Failed to create a validator instance.
    ValidatorCreationFailed,

    /// Failed to format error message for display.
    PrettyPrintFailed(String),
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParserError::ReadAfterEOF => write!(f, "Attempted to read after EOF"),
            ParserError::ReadInputFailed(msg) => write!(f, "Failed to read input: {}", msg),
            ParserError::TreesitterError => write!(f, "Tree-sitter parser error"),
            ParserError::ValidatorCreationFailed => write!(f, "Failed to create validator"),
            ParserError::PrettyPrintFailed(msg) => write!(f, "Failed to format error: {}", msg),
        }
    }
}

/// Errors in the schema definition itself.
///
/// These errors indicate problems with the schema document, not the input being validated.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaError {
    /// Node has multiple matchers in its children (only one is allowed).
    MultipleMatchersInNodeChildren {
        schema_index: usize,
        /// Number of matchers found.
        received: usize,
    },

    /// A repeating matcher in a textual container
    RepeatingMatcherInTextContainer { schema_index: usize },

    /// List node uses a non-repeating matcher.
    ///
    /// List nodes must use matchers with repetition syntax like `{1,}`.
    BadListMatcher { schema_index: usize },

    /// Matcher has invalid extras syntax.
    ///
    /// For example, `test:/1/`!{1,2} is invalid.
    InvalidMatcherExtras {
        schema_index: usize,
        error: MatcherExtrasError,
    },

    /// Matcher was not properly closed.
    UnclosedMatcher { schema_index: usize },

    /// Error occurred while constructing a matcher.
    MatcherError {
        error: MatcherError,
        schema_index: usize,
    },
    /// Unbounded repeating matcher is followed by another repeating matcher.
    ///
    /// When multiple repeating matchers appear in sequence, all but the last
    /// must have a specific upper bound.
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
    RepeatingMatcherUnbounded { schema_index: usize },

    /// Schema text contains invalid UTF-8 encoding.
    UTF8Error { schema_index: usize },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SchemaError::MultipleMatchersInNodeChildren { received, .. } => {
                write!(
                    f,
                    "Found {} matchers in node children (only 1 allowed)",
                    received
                )
            }
            SchemaError::RepeatingMatcherInTextContainer { .. } => {
                write!(f, "Repeating matcher cannot be used in text container")
            }
            SchemaError::BadListMatcher { .. } => {
                write!(f, "List node requires repeating matcher syntax")
            }
            SchemaError::InvalidMatcherExtras { error, .. } => {
                write!(f, "Invalid matcher extras: {}", error)
            }
            SchemaError::UnclosedMatcher { .. } => write!(f, "Matcher not properly closed"),
            SchemaError::MatcherError { error, .. } => write!(f, "Matcher error: {}", error),
            SchemaError::RepeatingMatcherUnbounded { .. } => {
                write!(f, "Unbounded repeating matcher must be last in sequence")
            }
            SchemaError::UTF8Error { .. } => write!(f, "Invalid UTF-8 in schema"),
        }
    }
}

/// Represents the kind of mismatch that occurred between expected and actual content in a node.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum NodeContentMismatchKind {
    /// The suffix following a matcher doesn't match.
    Suffix,
    /// The actual matcher pattern doesn't match.
    Matcher,
    /// The prefix following a matcher doesn't match.
    Prefix,
    /// A literal piece of content doesn't match.
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

/// Violations where input doesn't match a valid schema.
///
/// These errors indicate that the input document doesn't conform to the schema definition.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaViolationError {
    /// Node type doesn't match expected type from schema.
    NodeTypeMismatch {
        schema_index: usize,
        input_index: usize,
        /// Expected node type from schema.
        expected: String,
        /// Actual node type found in input.
        actual: String,
    },

    /// Node text content doesn't match expected pattern or literal.
    NodeContentMismatch {
        schema_index: usize,
        input_index: usize,
        /// Expected text or pattern from schema.
        expected: String,
        /// Actual content found in input.
        actual: String,
        /// Type of content mismatch (prefix, suffix, matcher, or literal).
        kind: NodeContentMismatchKind,
    },

    /// Matcher appears in list context without repetition syntax.
    ///
    /// List nodes require matchers to use `{min,max}` syntax.
    NonRepeatingMatcherInListContext {
        schema_index: usize,
        input_index: usize,
    },

    /// Number of children doesn't match expected count.
    ChildrenLengthMismatch {
        schema_index: usize,
        input_index: usize,
        /// Expected number of children from schema.
        expected: ChildrenCount,
        /// Actual number of children in input.
        actual: usize,
    },

    /// Nested list depth exceeds maximum allowed.
    NodeListTooDeep {
        schema_index: usize,
        input_index: usize,
        /// Maximum allowed nesting depth.
        max_depth: usize,
    },

    /// Number of list items is outside allowed range.
    WrongListCount {
        schema_index: usize,
        input_index: usize,
        /// Minimum number of items allowed (None means no minimum).
        min: Option<usize>,
        /// Maximum number of items allowed (None means no maximum).
        max: Option<usize>,
        /// Actual number of items in input.
        actual: usize,
    },
}

impl fmt::Display for SchemaViolationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SchemaViolationError::NodeTypeMismatch {
                expected, actual, ..
            } => {
                write!(f, "Expected node type '{}', found '{}'", expected, actual)
            }
            SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                kind,
                ..
            } => {
                write!(f, "Expected {} '{}', found '{}'", kind, expected, actual)
            }
            SchemaViolationError::NonRepeatingMatcherInListContext { .. } => {
                write!(f, "Non-repeating matcher used in list context")
            }
            SchemaViolationError::ChildrenLengthMismatch {
                expected, actual, ..
            } => {
                write!(f, "Expected {} children, found {}", expected, actual)
            }
            SchemaViolationError::NodeListTooDeep { max_depth, .. } => {
                write!(f, "List nesting exceeds maximum depth of {}", max_depth)
            }
            SchemaViolationError::WrongListCount {
                min, max, actual, ..
            } => {
                let range_desc = match (min, max) {
                    (Some(min_val), Some(max_val)) => format!("{}-{}", min_val, max_val),
                    (Some(min_val), None) => format!("at least {}", min_val),
                    (None, Some(max_val)) => format!("at most {}", max_val),
                    (None, None) => "any number of".to_string(),
                };
                write!(f, "Expected {} items, found {}", range_desc, actual)
            }
        }
    }
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

/// Errors that occur during pretty-printing of validation errors.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PrettyPrintError {
    /// Failed to format error message for display.
    FailedToPrettyPrint(String),

    /// UTF-8 encoding error while formatting.
    UTF8Error(String),
}

impl fmt::Display for PrettyPrintError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PrettyPrintError::FailedToPrettyPrint(msg) => {
                write!(f, "Failed to format error: {}", msg)
            }
            PrettyPrintError::UTF8Error(msg) => write!(f, "UTF-8 error during formatting: {}", msg),
        }
    }
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

/// Prints error using simple Debug formatting without pretty-printing.
///
/// This is for debugging and development when you want to see the raw error
/// structure without ariadne formatting.
pub fn debug_print_error(error: &ValidationError) -> String {
    format!("{:#?}", error)
}

/// Convert validation errors to an ariadne report, and then print them to a
/// buffer.
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
                let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                let schema_content =
                    node_content_by_index(tree.root_node(), *schema_index, source_content)?;
                let input_range = input_node.start_byte()..input_node.end_byte();
                let schema_range = schema_node.start_byte()..schema_node.end_byte();

                Report::build(ReportKind::Error, (filename, input_range.clone()))
                    .with_message("Non-repeating matcher in repeating context")
                    .with_label(
                        Label::new((filename, input_range))
                            .with_message(
                                "This input corresponds to a list node in the schema"
                            )
                            .with_color(Color::Blue),
                    )
                    .with_label(
                        Label::new((filename, schema_range))
                            .with_message(format!(
                                "This matcher is in a list context but is not marked as repeating: '{}'",
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
                schema_index: _,
                input_index,
                expected,
                actual,
            } => {
                let parent = find_node_by_index(tree.root_node(), *input_index);
                let parent_range = parent.start_byte()..parent.end_byte();

                let mut report = Report::build(ReportKind::Error, (filename, parent_range.clone()))
                    .with_message("Children length mismatch")
                    .with_label(
                        Label::new((filename, parent_range))
                            .with_message(format!(
                                "Expected {} children but found {}.",
                                expected, actual
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
                schema_index: _,
                input_index,
                max_depth,
            } => {
                let node = find_node_by_index(tree.root_node(), *input_index);
                let node_range = node.start_byte()..node.end_byte();

                Report::build(ReportKind::Error, (filename, node_range.clone()))
                    .with_message("Nested list exceeds maximum depth")
                    .with_label(
                        Label::new((filename, node_range))
                            .with_message(format!(
                                "List nesting exceeds maximum depth of {} level(s).",
                                max_depth,
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
                    schema_index,
                    received: received_count,
                } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_range = schema_node.start_byte()..schema_node.end_byte();

                    Report::build(ReportKind::Error, (filename, schema_range.clone()))
                        .with_message("Multiple matchers in node children")
                        .with_label(
                            Label::new((filename, schema_range))
                                .with_message(format!(
                                    "{} matchers found in node children (only 1 allowed)",
                                    received_count
                                ))
                                .with_color(Color::Red),
                        )
                        .with_help("Only one matcher is allowed per node's children.")
                        .finish()
                }
                SchemaError::RepeatingMatcherInTextContainer { schema_index } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_range = schema_node.start_byte()..schema_node.end_byte();

                    Report::build(ReportKind::Error, (filename, schema_range.clone()))
                        .with_message("Repeating matcher in text container")
                        .with_label(
                            Label::new((filename, schema_range))
                                .with_message("Repeating matcher cannot be used in a textual container")
                                .with_color(Color::Red),
                        )
                        .with_help("Text containers like paragraphs and headings cannot contain repeating matchers. Use repetition syntax only with list items.")
                        .finish()
                }
                SchemaError::BadListMatcher { schema_index } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_content =
                        node_content_by_index(tree.root_node(), *schema_index, source_content)?;
                    let schema_range = schema_node.start_byte()..schema_node.end_byte();

                    Report::build(ReportKind::Error, (filename, schema_range.clone()))
                        .with_message("Bad list matcher")
                        .with_label(
                            Label::new((filename, schema_range))
                                .with_message(format!(
                                    "No matchers found in children of list node: '{}'",
                                    schema_content
                                ))
                                .with_color(Color::Red),
                        )
                        .with_help("List nodes require repeating matcher syntax like `label:/pattern/`{1,}")
                        .finish()
                }
                SchemaError::UnclosedMatcher { schema_index } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_range = schema_node.start_byte()..schema_node.end_byte();

                    Report::build(ReportKind::Error, (filename, schema_range.clone()))
                        .with_message("Unclosed matcher")
                        .with_label(
                            Label::new((filename, schema_range))
                                .with_message("Matcher is not properly closed")
                                .with_color(Color::Red),
                        )
                        .with_help("Matchers must be properly closed with a backtick, e.g., `label:/pattern/`")
                        .finish()
                }
                SchemaError::MatcherError {
                    error,
                    schema_index,
                } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_range = schema_node.start_byte()..schema_node.end_byte();

                    Report::build(ReportKind::Error, (filename, schema_range.clone()))
                        .with_message("Matcher error")
                        .with_label(
                            Label::new((filename, schema_range))
                                .with_message(format!("Matcher error: {}", error))
                                .with_color(Color::Red),
                        )
                        .finish()
                }
                SchemaError::UTF8Error { schema_index } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_range = schema_node.start_byte()..schema_node.end_byte();

                    Report::build(ReportKind::Error, (filename, schema_range.clone()))
                        .with_message("UTF-8 error in schema")
                        .with_label(
                            Label::new((filename, schema_range))
                                .with_message("Schema text at this position is not valid UTF-8")
                                .with_color(Color::Red),
                        )
                        .finish()
                }
                SchemaError::InvalidMatcherExtras {
                    schema_index,
                    error,
                } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_range = schema_node.start_byte()..schema_node.end_byte();

                    Report::build(ReportKind::Error, (filename, schema_range.clone()))
                        .with_message("Invalid matcher extras")
                        .with_label(
                            Label::new((filename, schema_range))
                                .with_message(format!("Invalid matcher extras: {}", error))
                                .with_color(Color::Red),
                        )
                        .finish()
                }
                SchemaError::RepeatingMatcherUnbounded { schema_index } => {
                    let schema_node = find_node_by_index(tree.root_node(), *schema_index);
                    let schema_range = schema_node.start_byte()..schema_node.end_byte();

                    Report::build(ReportKind::Error, (filename, schema_range.clone()))
                        .with_message("Unbounded repeating matcher must be last")
                        .with_label(
                            Label::new((filename, schema_range))
                                .with_message(
                                    "This unbounded repeating matcher is followed by other repeating matchers.",
                               )
                                .with_color(Color::Red),
                        )
                        .with_help(
                            r#"When you have multiple repeating matchers in a row, all but the last one must have a specific upper bound. For example:
- `name1:/test/`{1,3}
- `name2:/bar/`{,}

The first matcher has a specific upper bound (3), while the last one can be unbounded."#
                        )
                        .finish()
                }
            }
        }
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
