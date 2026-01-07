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
        if let Some(error) = crate::mdschema::validator::utils::compare_node_kinds(
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
