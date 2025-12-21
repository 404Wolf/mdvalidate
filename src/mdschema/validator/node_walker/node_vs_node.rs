use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::ValidationError,
    node_walker::{
        list_vs_list::validate_list_vs_list,
        matcher_vs_text::validate_matcher_vs_text,
        text_vs_text::validate_text_vs_text,
        ValidationResult,
    },
    utils::{is_list_node, is_textual_container, is_textual_node},
};

/// Validate two arbitrary nodes against each other.
///
/// 1) If both nodes are textual nodes (schema is text, input is text), validate using `textual_vs_textual`.
/// 2) If both nodes are list nodes, validate using `matcher_vs_list`.
/// 3) If both nodes are heading nodes or document nodes, for each child of each, validate recursively using `validate_node_vs_node`.
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
        return validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    // 2) Both are textual containers - check for matcher usage
    if both_are_textual_containers(&input_node, &schema_node) {
        if has_code_child(&schema_node) {
            return validate_matcher_vs_text(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                got_eof,
            );
        } else {
            return validate_text_vs_text(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                got_eof,
            );
        }
    }

    // 3) Both are list nodes
    if both_are_list_nodes(&input_node, &schema_node) {
        return validate_list_vs_list(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    // 4) Both are heading nodes or document nodes
    if both_are_matching_top_level_nodes(&input_node, &schema_node) {
        // Crawl down one layer to get to the actual children
        if input_cursor.goto_first_child() && schema_cursor.goto_first_child() {
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
            result
                .errors
                .push(ValidationError::InternalInvariantViolated(
                    "Both input and schema node were top level, but they didn't both have children"
                        .into(),
                ));
        }
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
fn has_code_child(schema_node: &Node) -> bool {
    for i in 0..schema_node.child_count() {
        if let Some(child) = schema_node.child(i) {
            if child.kind() == "code_span" {
                return true;
            }
        }
    }
    false
}

/// Check if both nodes are list nodes.
fn both_are_list_nodes(input_node: &Node, schema_node: &Node) -> bool {
    is_list_node(&input_node) && is_list_node(&schema_node)
}

/// Check if both nodes are top-level nodes (document or heading).
fn both_are_matching_top_level_nodes(input_node: &Node, schema_node: &Node) -> bool {
    input_node.kind() == schema_node.kind()
        && (input_node.kind() == "document" || input_node.kind().starts_with("heading"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::{
        node_walker::node_vs_node::validate_node_vs_node, utils::parse_markdown,
    };

    use super::has_code_child;

    #[test]
    fn test_validate_two_paragraphs_with_text_vs_text() {
        let schema_str = "this is **bold** text.";
        let input_str = "this is **bold** text.";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let result = validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({}));

        let schema_str2 = "This is *bold* text.";
        let input_str2 = "This is **bold** text.";
        let schema2 = parse_markdown(schema_str2).unwrap();
        let input2 = parse_markdown(input_str2).unwrap();

        let schema_cursor = schema2.walk();
        let input_cursor = input2.walk();

        let result = validate_node_vs_node(&input_cursor, &schema_cursor, schema_str2, input_str2, false);

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

        let result = validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(result.errors.is_empty(), "Expected no errors, got: {:?}", result.errors);
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

        let result = validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(result.errors.is_empty(), "Expected no errors, got: {:?}", result.errors);
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

        let result = validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(result.errors.is_empty(), "Expected no errors, got: {:?}", result.errors);
        assert_eq!(result.value, json!({"name": "Alice"}));
    }

    #[test]
    fn test_has_code_child() {
        // Test with single code_span child (simple matcher)
        let schema_str = "`name:/\\w+/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(has_code_child(&schema_cursor.node()), "Expected code child for simple matcher");

        // Test with prefix, code_span, and suffix (complex matcher)
        let schema_str = "Hello `name:/\\w+/` world!";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(has_code_child(&schema_cursor.node()), "Expected code child for matcher with prefix and suffix");

        // Test with no code_span (regular text)
        let schema_str = "Hello **world**!";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(!has_code_child(&schema_cursor.node()), "Expected no code child for regular text");

        // Test with emphasis but no code_span
        let schema_str = "This is *italic* text";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(!has_code_child(&schema_cursor.node()), "Expected no code child for italic text");

        // Test with multiple code_spans
        let schema_str = "Start `first:/\\w+/` middle `second:/\\d+/` end";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph
        assert!(has_code_child(&schema_cursor.node()), "Expected code child for multiple matchers");

        // Test with list item containing code span (shouldn't be detected as matcher)
        let schema_str = "- test `test`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> list
        schema_cursor.goto_first_child(); // list -> list_item
        assert!(!has_code_child(&schema_cursor.node()), "Expected no code child for list item with code span");
    }
}
