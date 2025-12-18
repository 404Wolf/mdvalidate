use log::debug;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{Error, SchemaError, SchemaViolationError},
    node_walker::{
        matcher_vs_list::validate_matcher_vs_list, matcher_vs_text::validate_matcher_vs_text,
        text_vs_text::validate_text_vs_text, ValidationResult,
    },
    utils::is_list_node,
};

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
    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    debug!("Input sexpr: {}", input_cursor.node().to_sexp());
    debug!("Schema sexpr: {}", schema_cursor.node().to_sexp());

    let schema_node = schema_cursor.node();
    let input_node = input_cursor.node();

    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    let input_is_text_node = input_cursor.node().kind() == "text";

    // It's a paragraph and it has a single text child
    // TODO: support all types, including bold etc
    let input_has_single_text_child = input_cursor.node().child_count() == 1
        && input_cursor
            .node()
            .child(0)
            .map(|c| c.kind() == "text")
            .unwrap_or(false);

    let input_is_text_only = input_is_text_node || input_has_single_text_child;
    let schema_direct_children_code_node_count = schema_cursor
        .node()
        .children(&mut schema_cursor.clone())
        .filter(|c| c.kind() == "code_span")
        .count();

    if schema_direct_children_code_node_count > 1 {
        result.add_error(Error::SchemaError(
            SchemaError::MultipleMatchersInNodeChildren {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                received: schema_direct_children_code_node_count,
            },
        ));
        result.schema_descendant_index = schema_cursor.descendant_index();
        result.input_descendant_index = input_cursor.descendant_index();
        return result;
    }

    if schema_direct_children_code_node_count == 1 && input_is_text_only {
        let new_result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, got_eof);

        result.errors.extend(new_result.errors);
        result.value = new_result.value;
        result.schema_descendant_index = new_result.schema_descendant_index;
        result.input_descendant_index = new_result.input_descendant_index;
        return result;
    } else if is_list_node(&schema_node) && is_list_node(&input_node) {
        let new_result =
            validate_matcher_vs_list(&input_cursor, &schema_cursor, schema_str, input_str);

        result.errors.extend(new_result.errors);
        result.value = new_result.value;
        result.schema_descendant_index = new_result.schema_descendant_index;
        result.input_descendant_index = new_result.input_descendant_index;
        return result;
    } else if schema_cursor.node().kind() == "text" {
        let new_result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, got_eof);
        result.errors.extend(new_result.errors);
        result.schema_descendant_index = new_result.schema_descendant_index;
        result.input_descendant_index = new_result.input_descendant_index;
    }

    if input_cursor.node().child_count() != schema_cursor.node().child_count() {
        if is_list_node(&schema_cursor.node()) && is_list_node(&input_cursor.node()) {
            // If both nodes are list nodes, don't handle them here
        } else if got_eof {
            // TODO: this feels wrong, we should check to make sure that when eof is false we detect nested incomplete nodes too
            result.add_error(Error::SchemaViolation(
                SchemaViolationError::ChildrenLengthMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_cursor.node().child_count(),
                    actual: input_cursor.node().child_count(),
                },
            ));
        }
    }

    // TODO: what if one node has children and the other doesn't?
    if input_cursor.goto_first_child() && schema_cursor.goto_first_child() {
        let new_result =
            validate_node_vs_node(&input_cursor, &schema_cursor, schema_str, input_str, got_eof);

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
    }

    result.schema_descendant_index = schema_cursor.descendant_index();
    result.input_descendant_index = input_cursor.descendant_index();
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::{
        node_walker::node_vs_node::validate_node_vs_node, utils::parse_markdown,
    };

    #[test]
    fn test_validate_node_vs_node_simple_text_vs_text() {
        let schema_str = "Some Literal";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some Literal";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_node_vs_node(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        // Literal matching doesn't capture anything, they just (maybe) error
        assert!(
            result.errors.is_empty(),
            "Errors found: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_simple_matcher() {
        let schema_str = "`test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "test";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_node_vs_node(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        // The simple matcher should capture the literal "test"
        assert!(
            result.errors.is_empty(),
            "Errors found: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({"test": "test"}));
    }
}
