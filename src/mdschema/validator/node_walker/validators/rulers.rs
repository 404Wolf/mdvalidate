use log::trace;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::validators::ValidatorImpl;
use crate::mdschema::validator::ts_utils::is_ruler_node;
use crate::mdschema::validator::validator_walker::ValidatorWalker;

/// Validate that both nodes are rulers (thematic breaks).
///
/// This is a simple check - both nodes must be ruler nodes.
/// Rulers have no children and no content to validate.
pub(super) struct RulerVsRulerValidator;

impl ValidatorImpl for RulerVsRulerValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let _got_eof = got_eof;
        validate_ruler_vs_ruler_impl(walker.input_cursor(), walker.schema_cursor())
    }
}

fn validate_ruler_vs_ruler_impl(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(input_cursor, schema_cursor);

    let input_node = input_cursor.node();
    let schema_node = schema_cursor.node();

    // Both should be rulers - this is validated at the caller level in node_vs_node
    if !is_ruler_node(&input_node) || !is_ruler_node(&schema_node) {
        crate::invariant_violation!(
            result,
            input_cursor,
            schema_cursor,
            "ruler validation expects thematic_break nodes"
        );
        return result;
    }

    // Rulers have no children
    if input_node.child_count() != 0 || schema_node.child_count() != 0 {
        crate::invariant_violation!(
            result,
            input_cursor,
            schema_cursor,
            "ruler nodes should not have children"
        );
        return result;
    }

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

        let (value, errors, _) =
            ValidatorTester::<RulerVsRulerValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(i, s)| {
                    assert!(is_ruler_node(i));
                    assert!(is_ruler_node(s));
                })
                .validate_complete()
                .destruct();

        assert_eq!(errors, vec![], "Errors found: {:?}", errors);
        // Rulers don't capture matches
        assert_eq!(value, json!({}));
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
            let (value, errors, _) =
                ValidatorTester::<RulerVsRulerValidator>::from_strs(schema_str, input_str)
                    .walk()
                    .goto_first_child_then_unwrap()
                    .peek_nodes(|(i, s)| {
                        assert!(is_ruler_node(i));
                        assert!(is_ruler_node(s));
                    })
                    .validate_complete()
                    .destruct();

            assert_eq!(
                errors,
                vec![],
                "Expected no errors for schema '{}' and input '{}', got: {:?}",
                schema_str,
                input_str,
                errors
            );
            assert_eq!(value, json!({}));
        }
    }
}
