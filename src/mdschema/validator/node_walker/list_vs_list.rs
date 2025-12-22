use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
    node_walker::ValidationResult,
};

/// Validate a list node against a schema list node.
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_list_vs_list(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    todo!()
}
