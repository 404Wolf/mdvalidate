use tree_sitter::TreeCursor;

use crate::mdschema::validator::errors::{
    ChildrenLengthRange, SchemaViolationError, ValidationError,
};

/// Compare the number of children between schema and input nodes.
///
/// Returns an error if:
/// - At EOF: child counts don't match exactly
/// - Not at EOF: input has more children than schema
///
/// # Arguments
/// * `schema_cursor`: Cursor at schema node
/// * `input_cursor`: Cursor at input node
/// * `got_eof`: Whether we have received the full input document.
pub fn compare_node_children_lengths(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    got_eof: bool,
) -> Option<ValidationError> {
    // First, count the children to check for length mismatches
    let schema_child_count = schema_cursor.node().child_count();
    let input_child_count = input_cursor.node().child_count();

    // Handle node mismatches
    // If we have reached the EOF:
    //   No difference in the number of children
    // else:
    //   We can have less input children
    //
    let children_len_mismatch_err =
        ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
            expected: ChildrenLengthRange(schema_child_count, schema_child_count),
            actual: input_child_count,
        });

    if got_eof {
        // At EOF, children count must match exactly
        if input_child_count != schema_child_count {
            return Some(children_len_mismatch_err);
        }
    } else {
        // Not at EOF: input can have fewer children, but not more
        if input_child_count > schema_child_count {
            return Some(children_len_mismatch_err);
        }
    }

    None
}

/// Macro for checking if node children lengths match and adding error to result.
///
/// This macro encapsulates the common pattern of checking if two nodes have
/// the same number of children, adding an error to the result if they don't, and returning early.
///
/// # Example
///
/// ```rs
/// compare_node_children_lengths_check!(
///     schema_cursor,
///     input_cursor,
///     got_eof,
///     result
/// );
/// ```
#[macro_export]
macro_rules! compare_node_children_lengths_check {
    (
        $schema_cursor:expr,
        $input_cursor:expr,
        $got_eof:expr,
        $result:expr
    ) => {
        if let Some(error) = crate::mdschema::validator::node_walker::helpers::node_children_lengths::compare_node_children_lengths(
            &$schema_cursor,
            &$input_cursor,
            $got_eof,
        ) {
            $result.add_error(error);
            return $result;
        }
    };
}
