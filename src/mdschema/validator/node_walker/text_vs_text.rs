use tracing::instrument;
use tree_sitter::TreeCursor;
use serde_json::json;

use crate::mdschema::validator::{
    errors::{Error, NodeContentMismatchKind, SchemaViolationError},
    node_walker::ValidationResult,
};

#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_text_vs_text(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let input_node = input_cursor.node();
    let mut errors = Vec::new();

    let schema_text = &schema_str[schema_cursor.node().byte_range()];
    let input_text = &input_str[input_node.byte_range()];

    if schema_text != input_text && got_eof {
        errors.push(Error::SchemaViolation(
            SchemaViolationError::NodeContentMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_text.into(),
                actual: input_text.into(),
                kind: NodeContentMismatchKind::Literal,
            },
        ));
    }

    (json!({}), errors)
}
