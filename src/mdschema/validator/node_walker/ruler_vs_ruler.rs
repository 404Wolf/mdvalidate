use log::trace;
use tracing::instrument;
use crate::mdschema::validator::{
    cursor_pair::NodeCursorPair, node_walker::ValidationResult, ts_utils::is_ruler_node,
};

/// Validate that both nodes are rulers (thematic breaks).
///
/// This is a simple check - both nodes must be ruler nodes.
/// Rulers have no children and no content to validate.
#[instrument(skip(cursor_pair), level = "trace", fields(
    i = %cursor_pair.input_cursor.descendant_index(),
    s = %cursor_pair.schema_cursor.descendant_index(),
), ret)]
pub fn validate_ruler_vs_ruler(
    cursor_pair: &NodeCursorPair,
) -> ValidationResult {
    let result =
        ValidationResult::from_cursors(&cursor_pair.schema_cursor, &cursor_pair.input_cursor);

    let input_node = cursor_pair.input_cursor.node();
    let schema_node = cursor_pair.schema_cursor.node();

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
    use serde_json::json;
    use tree_sitter::TreeCursor;
    use crate::mdschema::validator::{
        cursor_pair::NodeCursorPair, node_walker::ruler_vs_ruler::validate_ruler_vs_ruler,
        ts_utils::parse_markdown,
    };

    fn make_pair<'a>(
        input_cursor: &TreeCursor<'a>,
        schema_cursor: &TreeCursor<'a>,
        input_str: &'a str,
        schema_str: &'a str,
    ) -> NodeCursorPair<'a> {
        NodeCursorPair::new(input_cursor.clone(), schema_cursor.clone(), input_str, schema_str)
    }

    #[test]
    fn test_validate_ruler_vs_ruler() {
        let schema_str = "---";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "---";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> thematic_break
        input_cursor.goto_first_child(); // document -> thematic_break

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
        let result = validate_ruler_vs_ruler(&cursor_pair);

        assert!(
            result.errors.is_empty(),
            "Errors found: {:?}",
            result.errors
        );
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
            let schema_tree = parse_markdown(schema_str).unwrap();
            let input_tree = parse_markdown(input_str).unwrap();

            let mut schema_cursor = schema_tree.walk();
            let mut input_cursor = input_tree.walk();

            schema_cursor.goto_first_child();
            input_cursor.goto_first_child();

            let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
            let result = validate_ruler_vs_ruler(&cursor_pair);

            assert!(
                result.errors.is_empty(),
                "Expected no errors for schema '{}' and input '{}', got: {:?}",
                schema_str,
                input_str,
                result.errors
            );
            assert_eq!(result.value, json!({}));
        }
    }
}
