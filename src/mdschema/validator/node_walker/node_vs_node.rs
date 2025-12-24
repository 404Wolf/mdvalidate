use log::trace;
use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::{ChildrenCount, SchemaViolationError, ValidationError},
    node_walker::{
        ValidationResult, code_vs_code::validate_code_vs_code, list_vs_list::validate_list_vs_list,
        text_vs_text::validate_text_vs_text,
    },
    ts_utils::{is_codeblock, is_list_node, is_textual_container, is_textual_node},
};

/// Validate two arbitrary nodes against each other.
///
/// Dispatches to the appropriate validator based on node types:
/// - Textual nodes -> `validate_text_vs_text`
/// - Code blocks -> `validate_code_vs_code`
/// - Lists -> `validate_list_vs_list`
/// - Headings/documents -> recursively validate children
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "trace", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_node_vs_node(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        input_cursor.descendant_index(),
        schema_cursor.descendant_index(),
    );

    let input_node = input_cursor.node();
    let schema_node = schema_cursor.node();

    // Make mutable copies that we can walk
    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    // 1) Both are textual nodes - use text_vs_text directly
    if both_are_textual_nodes(&input_node, &schema_node) {
        trace!("Both are textual nodes, validating text vs text");

        return validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    // 2) Both are container nodes - use container_vs_container directly
    if both_are_codeblocks(&input_node, &schema_node) {
        trace!("Both are container nodes, validating container vs container");

        return validate_code_vs_code(&input_cursor, &schema_cursor, schema_str, input_str);
    }

    // 3) Both are textual containers - check for matcher usage
    if both_are_textual_containers(&input_node, &schema_node) {
        trace!("Both are textual containers, validating text vs text");

        return validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    // 4) Both are list nodes
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

    // 5) Both are heading nodes or document nodes
    //
    // Crawl down one layer to get to the actual children
    if both_are_matching_top_level_nodes(&input_node, &schema_node)
        && input_cursor.goto_first_child()
        && schema_cursor.goto_first_child()
    {
        trace!("Both are heading nodes or document nodes, validating heading vs heading");

        let new_result = validate_node_vs_node(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        result.join_other_result(&new_result);

        result.schema_descendant_index = new_result.schema_descendant_index;
        result.input_descendant_index = new_result.input_descendant_index;

        loop {
            // TODO: handle case where one has more children than the other
            let input_had_sibling = input_cursor.goto_next_sibling();
            let schema_had_sibling = schema_cursor.goto_next_sibling();
            trace!(
                "input_cursor: {}, schema_cursor: {}",
                input_cursor.node().to_sexp(),
                schema_cursor.node().to_sexp()
            );
            trace!(
                "input_had_sibling: {}, schema_had_sibling: {}",
                input_had_sibling, schema_had_sibling
            );

            if input_had_sibling && schema_had_sibling {
                let new_result = validate_node_vs_node(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                );

                result.errors.extend(new_result.errors);
                // This is a merge for the JSON values.
                if let Some(new_obj) = new_result.value.as_object() {
                    if let Some(current_obj) = result.value.as_object_mut() {
                        for (key, value) in new_obj {
                            current_obj.insert(key.clone(), value.clone());
                        }
                    } else {
                        result.value = new_result.value;
                    }
                }
                result.schema_descendant_index = new_result.schema_descendant_index;
                result.input_descendant_index = new_result.input_descendant_index;
            } else {
                break;
            }
        }
    } else {
        trace!(
            "Both input and schema node were top level, but they didn't both have children. Trees:\n{}\n{}",
            input_node.to_sexp(),
            schema_node.to_sexp()
        );

        let schema_child_count = schema_cursor.node().child_count();
        let input_child_count = input_cursor.node().child_count();

        if schema_child_count != input_child_count {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::ChildrenLengthMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    // TODO: is there a case where we have a repeating list and this isn't true?
                    expected: ChildrenCount::from_specific(schema_child_count),
                    actual: input_child_count,
                },
            ));
        }

        return result;
    }

    result.schema_descendant_index = schema_cursor.descendant_index();
    result.input_descendant_index = input_cursor.descendant_index();

    result
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

/// Check if both nodes are top-level nodes (document or heading).
fn both_are_matching_top_level_nodes(input_node: &Node, schema_node: &Node) -> bool {
    if input_node.kind() != schema_node.kind() {
        return false;
    }

    match input_node.kind() {
        "document" => true,
        "atx_heading" => true,
        _ => false,
    }
}

/// Check if both nodes are codeblocks.
fn both_are_codeblocks(input_node: &Node, schema_node: &Node) -> bool {
    is_codeblock(&input_node) && is_codeblock(&schema_node)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::{
        errors::{ChildrenCount, SchemaViolationError, ValidationError},
        node_walker::node_vs_node::validate_node_vs_node,
        ts_utils::parse_markdown,
    };

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
    fn test_validate_two_paragraphs_with_text_vs_text() {
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
    fn test_simple_matcher() {
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
    fn test_textual_container_without_matcher() {
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
    fn test_matcher_with_prefix_and_suffix() {
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
    fn test_empty_schema_with_non_empty_input() {
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
    fn test_with_heading_and_codeblock() {
        let schema_str = "## Heading\n```\nCode\n```";
        let input_str = "## Heading\n```\nCode\n```";

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
}
