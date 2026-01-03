use log::trace;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::errors::ValidationError;
use crate::mdschema::validator::node_walker::validators::textual_container::validate_textual_container_vs_textual_container;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::ts_utils::{
    is_heading_node, is_marker_node, is_textual_container_node,
};
use crate::mdschema::validator::utils::compare_node_kinds;

/// Validate two headings.
///
/// Checks that they are the same kind of heading, and and then delegates to
/// `validate_text_vs_text`.
#[instrument(skip(input_cursor, schema_cursor), level = "trace", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
pub fn validate_heading_vs_heading(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(input_cursor, schema_cursor);

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    // Both should be the start of headings
    debug_assert!(is_heading_node(&input_cursor.node()));
    debug_assert!(is_heading_node(&schema_cursor.node()));

    // This also checks the *type* of heading that they are at
    if let Some(error) = compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str) {
        trace!("Node kinds mismatched");

        result.add_error(error);

        return result;
    }

    // Go to the actual heading content
    {
        let mut failed_to_walk_to_heading = false;
        if let Err(error) = ensure_at_heading_content(&mut input_cursor) {
            result.add_error(error);
            failed_to_walk_to_heading = true;
        }
        if let Err(error) = ensure_at_heading_content(&mut schema_cursor) {
            result.add_error(error);
            failed_to_walk_to_heading = true;
        }
        if failed_to_walk_to_heading {
            return result;
        }
    }
    result.sync_cursor_pos(&schema_cursor, &input_cursor); // save progress

    // Both should be at markers
    debug_assert!(is_textual_container_node(&input_cursor.node()));
    debug_assert!(is_textual_container_node(&schema_cursor.node()));

    // Now that we're at the heading content, use `validate_text_vs_text`
    validate_textual_container_vs_textual_container(
        &input_cursor,
        &schema_cursor,
        schema_str,
        input_str,
        got_eof,
    )
}

fn ensure_at_heading_content(cursor: &mut TreeCursor) -> Result<(), ValidationError> {
    // Headings look like this:
    //
    // (atx_heading)
    // │  ├─ (atx_h2_marker)
    // │  └─ (heading_content)
    // │     └─ (text)

    if is_heading_node(&cursor.node()) {
        cursor.goto_first_child();
        ensure_at_heading_content(cursor)
    } else if is_marker_node(&cursor.node()) {
        cursor.goto_next_sibling();
        debug_assert_eq!(cursor.node().kind(), "heading_content");
        Ok(())
    } else {
        Err(ValidationError::InternalInvariantViolated(format!(
            "Expected to be at heading content, but found node kind: {}",
            cursor.node().kind()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::{
        errors::SchemaViolationError, node_walker::validators::textual::validate_textual_vs_textual, ts_utils::{is_textual_node, parse_markdown}, validator_state::NodePosPair
    };
    use serde_json::json;

    #[test]
    fn test_ensure_at_heading_content() {
        // Test starting from root of heading
        let input_str = "# test heading";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert!(is_heading_node(&input_cursor.node()));

        ensure_at_heading_content(&mut input_cursor).unwrap();
        assert_eq!(input_cursor.node().kind(), "heading_content");

        // Test starting from marker node
        let input_str = "## test heading";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert!(is_marker_node(&input_cursor.node()));

        ensure_at_heading_content(&mut input_cursor).unwrap();
        assert_eq!(input_cursor.node().kind(), "heading_content");

        // Test starting at totally wrong item
        let input_str = "test heading";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert!(is_textual_node(&input_cursor.node()));

        ensure_at_heading_content(&mut input_cursor).unwrap_err();
    }

    #[test]
    fn test_validate_heading_vs_heading_simple_headings() {
        let schema_str = "# Heading";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "# Heading";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        // (document[0])
        // └─ (atx_heading[1])
        //    ├─ (atx_h1_marker[2])
        //    └─ (heading_content[3])
        //       └─ (text[4])

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        assert_eq!(schema_cursor.node().kind(), "atx_heading");
        assert_eq!(input_cursor.node().kind(), "atx_heading");

        let got_eof = true;
        let result = validate_heading_vs_heading(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);
        let result = validate_textual_vs_textual(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        assert_eq!(result.value, json!({})); // No real match content
        assert_eq!(result.errors, vec![]);
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 5));
    }

    #[test]
    fn test_validate_heading_vs_heading_wrong_heading_kind() {
        let schema_str = "# Heading";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "## Heading";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        assert_eq!(schema_cursor.node().kind(), "atx_heading");
        assert_eq!(input_cursor.node().kind(), "atx_heading");

        let got_eof = true;
        let result = validate_heading_vs_heading(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        assert_eq!(result.value, json!({}));
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeTypeMismatch {
                schema_index,
                input_index,
                expected,
                actual,
            }) => {
                assert_eq!(*schema_index, 1);
                assert_eq!(*input_index, 1);
                assert_eq!(expected, "atx_heading(atx_h1_marker)");
                assert_eq!(actual, "atx_heading(atx_h2_marker)");
            }
            _ => panic!(
                "Expected SchemaViolation(NodeTypeMismatch), got {:?}",
                result.errors[0]
            ),
        }
    }

    // TODO: tests for got_eof=false
}
