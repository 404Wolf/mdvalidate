use log::trace;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::errors::ValidationError;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::validators::textual_container::TextualContainerVsTextualContainerValidator;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
use crate::mdschema::validator::ts_utils::{
    is_heading_content_node, is_heading_node, is_marker_node, is_textual_container_node,
};
use crate::mdschema::validator::utils::compare_node_kinds;
use crate::mdschema::validator::validator_walker::ValidatorWalker;

/// Validate two headings.
///
/// Checks that they are the same kind of heading, and and then delegates to
/// `TextualContainerVsTextualContainerValidator::validate`.
pub(super) struct HeadingVsHeadingValidator;

impl ValidatorImpl for HeadingVsHeadingValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut result =
            ValidationResult::from_cursors(walker.input_cursor(), walker.schema_cursor());

        let input_str = walker.input_str();
        let schema_str = walker.schema_str();

        let mut input_cursor = walker.input_cursor().clone();
        let mut schema_cursor = walker.schema_cursor().clone();

        // Both should be the start of headings
        if !is_heading_node(&input_cursor.node()) || !is_heading_node(&schema_cursor.node()) {
            crate::invariant_violation!(
                result,
                input_cursor,
                schema_cursor,
                "heading validation expects atx_heading nodes"
            );
            return result;
        }

        // This also checks the *type* of heading that they are at
        if let Some(error) =
            compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str)
        {
            trace!("Node kinds mismatched");

            result.add_error(error);

            return result;
        }

        // Go to the actual heading content
        {
            let mut failed_to_walk_to_heading = false;
            let input_had_heading_content = match ensure_at_heading_content(&mut input_cursor) {
                Ok(had_content) => had_content,
                Err(error) => {
                    result.add_error(error);
                    failed_to_walk_to_heading = true;
                    false
                }
            };
            let schema_had_heading_content = match ensure_at_heading_content(&mut schema_cursor) {
                Ok(had_content) => had_content,
                Err(error) => {
                    result.add_error(error);
                    failed_to_walk_to_heading = true;
                    false
                }
            };
            if failed_to_walk_to_heading
                || !(input_had_heading_content && schema_had_heading_content)
            {
                return result;
            }
        }
        result.sync_cursor_pos(&schema_cursor, &input_cursor); // save progress

        // Both should be at markers
        if !is_textual_container_node(&input_cursor.node())
            || !is_textual_container_node(&schema_cursor.node())
        {
            crate::invariant_violation!(
                result,
                input_cursor,
                schema_cursor,
                "heading validation expects textual container nodes"
            );
            return result;
        }

        // Now that we're at the heading content, use `validate_text_vs_text`
        TextualContainerVsTextualContainerValidator::validate(
            &walker.with_cursors(&input_cursor, &schema_cursor),
            got_eof,
        )
    }
}

fn ensure_at_heading_content(cursor: &mut TreeCursor) -> Result<bool, ValidationError> {
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
        if cursor.goto_next_sibling() {
            if !is_heading_content_node(&cursor.node()) {
                return Err(crate::invariant_violation!(
                    cursor,
                    cursor,
                    "expected heading_content node"
                ));
            }
            Ok(true)
        } else {
            Ok(false)
        }
    } else {
        Err(crate::invariant_violation!(
            cursor,
            cursor,
            format!(
                "Expected to be at heading content, but found node kind: {}",
                cursor.node().kind()
            )
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::{
        errors::SchemaViolationError, node_pos_pair::NodePosPair, node_walker::validators::test_utils::ValidatorTester, ts_utils::{is_heading_node, is_textual_node, parse_markdown}
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

        input_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // paragraph -> text
        assert!(is_textual_node(&input_cursor.node()));

        ensure_at_heading_content(&mut input_cursor).unwrap_err();
    }

    #[test]
    fn test_validate_heading_vs_heading_simple_headings() {
        let schema_str = "# Heading";
        let input_str = "# Heading";

        let (value, errors, farthest_reached_pos) =
            ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(i, s)| {
                    assert!(is_heading_node(i));
                    assert!(is_heading_node(s));
                })
                .validate_complete()
                .destruct();

        assert_eq!(value, json!({})); // No real match content
        assert_eq!(errors, vec![]);
        assert_eq!(farthest_reached_pos, NodePosPair::from_pos(4, 4));
    }

    #[test]
    fn test_validate_heading_vs_heading_wrong_heading_kind() {
        let schema_str = "# Heading";
        let input_str = "## Heading";

        let (value, errors, _) =
            ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(i, s)| {
                    assert!(is_heading_node(i));
                    assert!(is_heading_node(s));
                })
                .validate_complete()
                .destruct();

        assert_eq!(value, json!({}));
        assert_eq!(
            errors,
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: 1,
                    input_index: 1,
                    expected: "atx_heading(atx_h1_marker)".to_string(),
                    actual: "atx_heading(atx_h2_marker)".to_string(),
                }
            )]
        );
    }
    // TODO: tests for got_eof=false
}
