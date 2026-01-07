use tree_sitter::TreeCursor;

use crate::mdschema::validator::errors::{SchemaViolationError, ValidationError};
use crate::mdschema::validator::ts_utils::{
    extract_list_marker, get_heading_kind, is_ordered_list_marker, is_unordered_list_marker,
};

/// Compare the kinds (types) of two nodes and return an error if they don't match.
///
/// Special handling for:
/// - Tight lists: checks list marker type (ordered vs unordered)
/// - Headings: checks heading level
/// - Other nodes: checks exact kind match
///
/// # Arguments
/// - `schema_cursor`: Cursor at schema node
/// - `input_cursor`: Cursor at input node
/// - `input_str`: The input markdown string
/// - `schema_str`: The schema markdown string
pub fn compare_node_kinds(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    input_str: &str,
    schema_str: &str,
) -> Option<ValidationError> {
    let schema_node = schema_cursor.node();
    let input_node = input_cursor.node();

    let schema_kind = schema_node.kind();
    let input_kind = input_node.kind();

    // If they are both tight lists, check the first children of each of them,
    // which are list markers. This will indicate whether they are the same type
    // of list.
    if schema_cursor.node().kind() == "tight_list" && input_cursor.node().kind() == "tight_list" {
        let schema_list_marker = extract_list_marker(schema_cursor, schema_str);
        let input_list_marker = extract_list_marker(input_cursor, input_str);

        // They must both be unordered, both be ordered, or both have the same marker
        if schema_list_marker == input_list_marker {
            // They can be the same list symbol!
        } else if is_ordered_list_marker(schema_list_marker)
            && is_ordered_list_marker(input_list_marker)
        {
            // Or both ordered
        } else if is_unordered_list_marker(schema_list_marker)
            && is_unordered_list_marker(input_list_marker)
        {
            // Or both unordered
        } else {
            // But anything else is a mismatch

            return Some(ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    // TODO: find a better way to represent the *kind* of list in this error
                    expected: format!("{}({})", input_cursor.node().kind(), schema_list_marker),
                    actual: format!("{}({})", input_cursor.node().kind(), input_list_marker),
                },
            ));
        }
    }

    if schema_cursor.node().kind() == "atx_heading" && input_cursor.node().kind() == "atx_heading" {
        let schema_heading_kind = match get_heading_kind(&schema_cursor) {
            Ok(kind) => kind,
            Err(error) => return Some(error),
        };
        let input_heading_kind = match get_heading_kind(&input_cursor) {
            Ok(kind) => kind,
            Err(error) => return Some(error),
        };

        if schema_heading_kind != input_heading_kind {
            return Some(ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: format!("{}({})", input_cursor.node().kind(), schema_heading_kind),
                    actual: format!("{}({})", input_cursor.node().kind(), input_heading_kind),
                },
            ));
        }
    }

    if schema_kind != input_kind {
        Some(ValidationError::SchemaViolation(
            SchemaViolationError::NodeTypeMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_node.kind().into(),
                actual: input_node.kind().into(),
            },
        ))
    } else {
        None
    }
}

/// Macro for checking if node kinds match and adding error to result.
///
/// This macro encapsulates the common pattern of checking if two nodes have
/// matching kinds, adding an error to the result if they don't, and returning early.
///
/// # Example
///
/// ```ignore
/// compare_node_kinds_check!(
///     schema_cursor,
///     input_cursor,
///     input_str,
///     schema_str,
///     result
/// );
/// ```
#[macro_export]
macro_rules! compare_node_kinds_check {
    (
        $schema_cursor:expr,
        $input_cursor:expr,
        $input_str:expr,
        $schema_str:expr,
        $result:expr
    ) => {
        if let Some(error) = crate::mdschema::validator::node_walker::helpers::node_kinds::compare_node_kinds(
            &$schema_cursor,
            &$input_cursor,
            $input_str,
            $schema_str,
        ) {
            $result.add_error(error);
            return $result;
        }
    };
}
