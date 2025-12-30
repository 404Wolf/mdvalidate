use log::trace;
use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::helpers::node_print::PrettyPrint;
use crate::mdschema::validator::errors::ValidationError;
use crate::mdschema::validator::node_walker::validators::code::validate_code_vs_code;
use crate::mdschema::validator::node_walker::validators::headings::validate_heading_vs_heading;
use crate::mdschema::validator::node_walker::validators::lists::validate_list_vs_list;
use crate::mdschema::validator::node_walker::validators::rulers::validate_ruler_vs_ruler;
use crate::mdschema::validator::node_walker::validators::textual::validate_textual_container_vs_textual_container;
use crate::mdschema::validator::ts_utils::{
    both_are_codeblocks, both_are_list_nodes, both_are_matching_top_level_nodes, both_are_rulers,
    both_are_textual_containers, both_are_textual_nodes, is_heading_node, is_ruler_node,
};
use crate::mdschema::validator::{
    node_walker::ValidationResult,
    ts_utils::{is_codeblock_node, is_list_node, is_textual_container_node, is_textual_node},
    utils::compare_node_children_lengths,
};

/// Validate two arbitrary nodes against each other.
///
/// Dispatches to the appropriate validator based on node types:
/// - Textual nodes -> `validate_text_vs_text`
/// - Code blocks -> `validate_code_vs_code`
/// - Lists -> `validate_list_vs_list`
/// - Headings/documents -> recursively validate children
///   #[track_caller]
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "trace", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
pub fn validate_node_vs_node(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(input_cursor, schema_cursor);

    let input_node = input_cursor.node();
    let schema_node = schema_cursor.node();

    // Make mutable copies that we can walk
    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    // Both are textual nodes - use text_vs_text directly
    if both_are_textual_nodes(&input_node, &schema_node) {
        trace!("Both are textual nodes, validating text vs text");

        return validate_textual_container_vs_textual_container(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    // Both are container nodes - use container_vs_container directly
    if both_are_codeblocks(&input_node, &schema_node) {
        trace!("Both are container nodes, validating container vs container");

        return validate_code_vs_code(&input_cursor, &schema_cursor, schema_str, input_str);
    }

    // Both are textual containers - check for matcher usage
    if both_are_textual_containers(&input_node, &schema_node) {
        trace!("Both are textual containers, validating text vs text");

        return validate_textual_container_vs_textual_container(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    // Both are list nodes
    if both_are_list_nodes(&input_node, &schema_node) {
        trace!("Both are list nodes, validating list vs list");

        return validate_list_vs_list(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    // Both are ruler nodes
    if both_are_rulers(&input_node, &schema_node) {
        trace!("Both are rulers, validating ruler vs ruler");

        return validate_ruler_vs_ruler(&input_cursor, &schema_cursor);
    }

    // Both are heading nodes or document nodes
    //
    // Crawl down one layer to get to the actual children
    if both_are_matching_top_level_nodes(&input_node, &schema_node)
        && input_cursor.clone().goto_first_child() // don't actually do the walking down just yet
        && schema_cursor.clone().goto_first_child()
    {
        // First, if they are headings, validate the headings themselves.
        if is_heading_node(&input_node) && is_heading_node(&schema_node) {
            trace!("Both are heading nodes, validating heading vs heading");

            let heading_result = validate_heading_vs_heading(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                got_eof,
            );
            result.join_other_result(&heading_result);
            result.sync_cursor_pos(&schema_cursor, &input_cursor);
        }

        trace!("Both are heading nodes or document nodes. Recursing into sibling pairs.");

        // Since we're dealing with top level nodes it is our responsibility to ensure that they have the same number of children.
        if let Some(error) = compare_node_children_lengths(&schema_cursor, &input_cursor, got_eof) {
            result.add_error(error);

            return result;
        }

        // Now actually go down to the children
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        let new_result = validate_node_vs_node(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        result.join_other_result(&new_result);
        result.sync_cursor_pos(&schema_cursor, &input_cursor);

        loop {
            // TODO: handle case where one has more children than the other
            let input_had_sibling = input_cursor.goto_next_sibling();
            let schema_had_sibling = schema_cursor.goto_next_sibling();
            trace!(
                "input_had_sibling: {}, schema_had_sibling: {}, input_kind: {}, schema_kind: {}",
                input_had_sibling,
                schema_had_sibling,
                input_cursor.node().kind(),
                schema_cursor.node().kind()
            );

            if input_had_sibling && schema_had_sibling {
                trace!("Both input and schema node have siblings");

                let new_result = validate_node_vs_node(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                );
                result.join_other_result(&new_result);
                result.sync_cursor_pos(&schema_cursor, &input_cursor);
            } else {
                trace!("One of input or schema node does not have siblings");

                break;
            }
        }

        return result;
    }

    if !got_eof {
        return result;
    } else {
        #[cfg(debug_assertions)]
        {
            eprintln!("{}", input_node.pretty_print());
            eprintln!("{}", schema_node.pretty_print());
        }

        result.add_error(ValidationError::InternalInvariantViolated(format!(
            "No combination of nodes that we check for was covered. \
                     Attempting to compare a {}[{}] node with a {}[{}] node",
            input_node.kind(),
            input_cursor.descendant_index(),
            schema_node.kind(),
            schema_cursor.descendant_index()
        )));

        return result;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        helpers::node_print::PrettyPrint,
        mdschema::validator::{
            errors::{ChildrenCount, SchemaViolationError, ValidationError},
            node_walker::node_vs_node::validate_node_vs_node,
            ts_utils::parse_markdown,
        },
    };

    #[test]
    fn test_validate_node_vs_node_with_with_nesting_lists() {
        let schema_str = r#"
- `test:/\w+/`{2,2}
  - `test2:/\w+/`{1,1}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
  - deepy
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let input_cursor = input_tree.walk();
        assert_eq!(input_cursor.node().kind(), "document");

        let result =
            validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );

        assert_eq!(
            result.value,
            json!({
                "test": [
                    "test1",
                    "test2",
                    { "test2": [ "deepy" ] }
                ]
            })
        );
    }

    #[test]
    fn test_validate_node_vs_node_with_two_mixed_paragraphs() {
        let schema_str = "this is **bold** text.";
        let input_str = "this is **bold** text.";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let result =
            validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({}));

        let schema_str2 = "This is *bold* text.";
        let input_str2 = "This is **bold** text.";
        let schema2 = parse_markdown(schema_str2).unwrap();
        let input2 = parse_markdown(input_str2).unwrap();

        let schema_cursor = schema2.walk();
        let input_cursor = input2.walk();

        let result = validate_node_vs_node(
            &input_cursor,
            &schema_cursor,
            schema_str2,
            input_str2,
            false,
        );

        assert!(!result.errors.is_empty());
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_simple_text_matcher() {
        let schema_str = "`name:/\\w+/`";
        let input_str = "Alice";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let result =
            validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({"name": "Alice"}));
    }

    #[test]
    fn test_validate_node_vs_node_with_textual_container_without_matcher() {
        let schema_str = "Hello **world**";
        let input_str = "Hello **world**";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let result =
            validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_matcher_with_prefix_and_suffix() {
        let schema_str = "Hello `name:/\\w+/` world!";
        let input_str = "Hello Alice world!";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let result =
            validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({"name": "Alice"}));
    }

    #[test]
    fn test_validate_node_vs_node_with_empty_schema_with_non_empty_input() {
        let schema_str = "";
        let input_str = "# Some content\n";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let result =
            validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            !result.errors.is_empty(),
            "Expected validation errors when schema is empty but input is not"
        );

        match result.errors.first() {
            Some(error) => {
                match error {
                    ValidationError::SchemaViolation(
                        SchemaViolationError::ChildrenLengthMismatch {
                            schema_index,
                            input_index,
                            expected,
                            actual,
                        },
                    ) => {
                        // Expected this specific error type
                        assert!(schema_index >= &0, "schema_index should be non-negative");
                        assert!(input_index >= &0, "input_index should be non-negative");
                        assert_eq!(
                            *expected,
                            ChildrenCount::SpecificCount(0),
                            "expected should be 0 for empty schema"
                        );
                        assert!(
                            actual > &0,
                            "actual should be greater than 0 for non-empty input"
                        );
                    }
                    _ => panic!("Expected ChildrenLengthMismatch error, got: {:?}", error),
                }
            }
            None => panic!("Expected error"),
        }
    }

    #[test]
    fn test_validate_node_vs_node_with_heading_and_codeblock() {
        let schema_str = "## Heading\n```\nCode\n```";
        let input_str = "## Heading\n```\nCode\n```";

        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();
        eprintln!("{}", input_cursor.node().pretty_print());
        eprintln!("{}", schema_cursor.node().pretty_print());

        let result =
            validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }
}
