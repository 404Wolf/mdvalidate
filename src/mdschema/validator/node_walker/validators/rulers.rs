use log::trace;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::node_walker::validators::ValidatorImpl;
use crate::mdschema::validator::node_walker::{ValidationResult, validators::Validator};
use crate::mdschema::validator::ts_utils::is_ruler_node;

/// Validate that both nodes are rulers (thematic breaks).
///
/// This is a simple check - both nodes must be ruler nodes.
/// Rulers have no children and no content to validate.
pub fn validate_ruler_vs_ruler(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    RulerVsRulerValidator::validate(input_cursor, schema_cursor, schema_str, input_str, got_eof)
}

struct RulerVsRulerValidator;

impl ValidatorImpl for RulerVsRulerValidator {
    fn validate_impl(
        input_cursor: &TreeCursor,
        schema_cursor: &TreeCursor,
        schema_str: &str,
        input_str: &str,
        got_eof: bool,
    ) -> ValidationResult {
        let _schema_str = schema_str;
        let _input_str = input_str;
        let _got_eof = got_eof;

        validate_ruler_vs_ruler_impl(input_cursor, schema_cursor)
    }
}

fn validate_ruler_vs_ruler_impl(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
) -> ValidationResult {
    let result = ValidationResult::from_cursors(input_cursor, schema_cursor);

    let input_node = input_cursor.node();
    let schema_node = schema_cursor.node();

    // Both should be rulers - this is validated at the caller level in node_vs_node
    debug_assert!(is_ruler_node(&input_node), "Input node should be a ruler");
    debug_assert!(is_ruler_node(&schema_node), "Schema node should be a ruler");

    // Rulers have no children
    debug_assert_eq!(input_node.child_count(), 0);
    debug_assert_eq!(schema_node.child_count(), 0);

    trace!("Ruler validated successfully");

    // Return empty result - rulers don't capture any data
    result
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::node_walker::validators::rulers::RulerVsRulerValidator;
    use crate::mdschema::validator::node_walker::validators::test_utils::ValidatorTester;
    use crate::mdschema::validator::ts_utils::is_ruler_node;
    use serde_json::json;

    #[test]
    fn test_validate_ruler_vs_ruler() {
        let schema_str = "---";
        let input_str = "---";
        let result = ValidatorTester::<RulerVsRulerValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(i, s)| {
                assert!(is_ruler_node(i));
                assert!(is_ruler_node(s));
            })
            .validate(true);

        assert_eq!(result.errors, vec![], "Errors found: {:?}", result.errors);
        // Rulers don't capture matches
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_ruler_vs_ruler_different_styles() {
        // Test that different ruler styles (---, ***, ___) all work
        let test_cases = vec![
            ("---", "---"),
            ("***", "***"),
            ("___", "___"),
            ("---", "***"), // Different styles should still validate
            ("___", "---"),
        ];

        for (schema_str, input_str) in test_cases {
            let result = ValidatorTester::<RulerVsRulerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(i, s)| {
                    assert!(is_ruler_node(i));
                    assert!(is_ruler_node(s));
                })
                .validate(true);

            assert_eq!(
                result.errors,
                vec![],
                "Expected no errors for schema '{}' and input '{}', got: {:?}",
                schema_str,
                input_str,
                result.errors
            );
            assert_eq!(result.value, json!({}));
        }
    }
}
