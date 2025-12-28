use log::trace;
use tracing::instrument;
use tree_sitter::Node;

use crate::mdschema::validator::{
    cursor_pair::NodeCursorPair,
    node_walker::{
        ValidationResult, code_vs_code::validate_code_vs_code, list_vs_list::validate_list_vs_list,
        text_vs_text::validate_text_vs_text, ruler_vs_ruler::validate_ruler_vs_ruler,
    },
    ts_utils::{is_marker_node, is_ruler_node},
    ts_utils::{is_codeblock, is_list_node, is_textual_container, is_textual_node},
    utils::compare_node_children_lengths,
};

/// Validate two arbitrary nodes against each other.
///
/// Dispatches to the appropriate validator based on node types:
/// - Textual nodes -> `validate_text_vs_text`
/// - Code blocks -> `validate_code_vs_code`
/// - Lists -> `validate_list_vs_list`
/// - Headings/documents -> recursively validate children
#[instrument(skip(cursor_pair, got_eof), level = "trace", fields(
    i = %cursor_pair.input_cursor.descendant_index(),
    s = %cursor_pair.schema_cursor.descendant_index(),
), ret)]
pub fn validate_node_vs_node(
    cursor_pair: &NodeCursorPair,
    got_eof: bool,
) -> ValidationResult {
    let mut result =
        ValidationResult::from_cursors(&cursor_pair.schema_cursor, &cursor_pair.input_cursor);

    let input_node = cursor_pair.input_cursor.node();
    let schema_node = cursor_pair.schema_cursor.node();

    // Make mutable copies that we can walk
    let mut input_cursor = cursor_pair.input_cursor.clone();
    let mut schema_cursor = cursor_pair.schema_cursor.clone();

    // Both are textual nodes - use text_vs_text directly
    if both_are_textual_nodes(&input_node, &schema_node) {
        trace!("Both are textual nodes, validating text vs text");

        let child_pair = cursor_pair.with_cursors(input_cursor, schema_cursor);
        return validate_text_vs_text(&child_pair, got_eof);
    }

    // Both are container nodes - use container_vs_container directly
    if both_are_codeblocks(&input_node, &schema_node) {
        trace!("Both are container nodes, validating container vs container");

        let child_pair = cursor_pair.with_cursors(input_cursor, schema_cursor);
        return validate_code_vs_code(&child_pair);
    }

    // Both are textual containers - check for matcher usage
    if both_are_textual_containers(&input_node, &schema_node) {
        trace!("Both are textual containers, validating text vs text");

        let child_pair = cursor_pair.with_cursors(input_cursor, schema_cursor);
        return validate_text_vs_text(&child_pair, got_eof);
    }

    // Both are list nodes
    if both_are_list_nodes(&input_node, &schema_node) {
        trace!("Both are list nodes, validating list vs list");

        let child_pair = cursor_pair.with_cursors(input_cursor, schema_cursor);
        return validate_list_vs_list(&child_pair, got_eof);
    }

    // Both are the same kind and have children - recurse through them.
    if input_node.kind() == schema_node.kind() {
        if let Some(error) = compare_node_children_lengths(&schema_cursor, &input_cursor, got_eof) {
            result.add_error(error);
            return result;
        }

        let mut input_child_cursor = input_cursor.clone();
        let mut schema_child_cursor = schema_cursor.clone();

        let input_has_child = input_child_cursor.goto_first_child();
        let schema_has_child = schema_child_cursor.goto_first_child();

        if !input_has_child || !schema_has_child {
            return result;
        }

        let child_pair =
            cursor_pair.with_cursors(input_child_cursor.clone(), schema_child_cursor.clone());
        let new_result = validate_node_vs_node(&child_pair, got_eof);
        result.join_other_result(&new_result);
        result.sync_cursor_pos(&schema_child_cursor, &input_child_cursor);

        loop {
            let input_had_sibling = input_child_cursor.goto_next_sibling();
            let schema_had_sibling = schema_child_cursor.goto_next_sibling();

            if input_had_sibling && schema_had_sibling {
                trace!("Both input and schema node have siblings");

                let child_pair = cursor_pair.with_cursors(
                    input_child_cursor.clone(),
                    schema_child_cursor.clone(),
                );
                let new_result = validate_node_vs_node(&child_pair, got_eof);
                result.join_other_result(&new_result);
                result.sync_cursor_pos(&schema_child_cursor, &input_child_cursor);
            } else {
                trace!("One of input or schema node does not have siblings");

                return result;
            }
        }
    }

    if both_are_rulers(&input_node, &schema_node) {
        trace!("Both are rulers, validating ruler vs ruler");
        
        let child_pair = cursor_pair.with_cursors(input_cursor, schema_cursor);
        return validate_ruler_vs_ruler(&child_pair);
    }

    if both_are_markers(&input_node, &schema_node) {
        // Nothing to do. Markers don't have children.
        debug_assert_eq!(input_node.child_count(), 0);
        debug_assert_eq!(schema_node.child_count(), 0);
        return result;
    }

    if !got_eof {
        return result;
    } else {
        result.add_error(
            crate::mdschema::validator::errors::ValidationError::InternalInvariantViolated(
                "No combination of nodes that we check for was covered.".into(),
            ),
        );
        return result;
    }
}

/// Check if both nodes are markers. For example, heading markers, or list markers.
fn both_are_markers(input_node: &Node, schema_node: &Node) -> bool {
    is_marker_node(&input_node) && is_marker_node(&schema_node)
}

/// Check if both nodes are rulers.
fn both_are_rulers(input_node: &Node, schema_node: &Node) -> bool {
    is_ruler_node(&input_node) && is_ruler_node(&schema_node)
}

/// Check if both nodes are textual nodes.
fn both_are_textual_nodes(input_node: &Node, schema_node: &Node) -> bool {
    is_textual_node(&input_node) && is_textual_node(&schema_node)
}

/// Check if both nodes are textual containers.
fn both_are_textual_containers(input_node: &Node, schema_node: &Node) -> bool {
    is_textual_container(&input_node) && is_textual_container(&schema_node)
}

/// Check if the schema node has a code_span child (indicating a matcher).

/// Check if both nodes are list nodes.
fn both_are_list_nodes(input_node: &Node, schema_node: &Node) -> bool {
    is_list_node(&input_node) && is_list_node(&schema_node)
}

/// Check if both nodes are codeblocks.
fn both_are_codeblocks(input_node: &Node, schema_node: &Node) -> bool {
    is_codeblock(&input_node) && is_codeblock(&schema_node)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tree_sitter::TreeCursor;

    use crate::mdschema::validator::{
        cursor_pair::NodeCursorPair,
        errors::{ChildrenCount, SchemaViolationError, ValidationError},
        node_walker::node_vs_node::validate_node_vs_node,
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
    fn validate_list_vs_list_with_nesting() {
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

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
        let result = validate_node_vs_node(&cursor_pair, true);

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
    fn test_validate_two_paragraphs_with_text_vs_text() {
        let schema_str = "this is **bold** text.";
        let input_str = "this is **bold** text.";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
        let result = validate_node_vs_node(&cursor_pair, false);

        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({}));

        let schema_str2 = "This is *bold* text.";
        let input_str2 = "This is **bold** text.";
        let schema2 = parse_markdown(schema_str2).unwrap();
        let input2 = parse_markdown(input_str2).unwrap();

        let schema_cursor = schema2.walk();
        let input_cursor = input2.walk();

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str2, schema_str2);
        let result = validate_node_vs_node(&cursor_pair, false);

        assert!(!result.errors.is_empty());
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_simple_matcher() {
        let schema_str = "`name:/\\w+/`";
        let input_str = "Alice";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
        let result = validate_node_vs_node(&cursor_pair, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({"name": "Alice"}));
    }

    #[test]
    fn test_textual_container_without_matcher() {
        let schema_str = "Hello **world**";
        let input_str = "Hello **world**";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
        let result = validate_node_vs_node(&cursor_pair, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_matcher_with_prefix_and_suffix() {
        let schema_str = "Hello `name:/\\w+/` world!";
        let input_str = "Hello Alice world!";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
        let result = validate_node_vs_node(&cursor_pair, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({"name": "Alice"}));
    }

    #[test]
    fn test_empty_schema_with_non_empty_input() {
        let schema_str = "";
        let input_str = "# Some content\n";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
        let result = validate_node_vs_node(&cursor_pair, true);

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
    fn test_with_heading_and_codeblock() {
        let schema_str = "## Heading\n```\nCode\n```";
        let input_str = "## Heading\n```\nCode\n```";

        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let cursor_pair = make_pair(&input_cursor, &schema_cursor, input_str, schema_str);
        let result = validate_node_vs_node(&cursor_pair, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }
}
