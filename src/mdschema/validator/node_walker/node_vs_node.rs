use log::debug;
use serde_json::json;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{Error, SchemaError, SchemaViolationError},
    node_walker::{
        ValidationResult,
        matcher_vs_list::validate_matcher_vs_list,
        matcher_vs_text::validate_matcher_vs_text,
        text_vs_text::validate_text_vs_text,
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
    debug!("Input sexpr: {}", input_cursor.node().to_sexp());
    debug!("Schema sexpr: {}", schema_cursor.node().to_sexp());

    let input_cursor = &mut input_cursor.clone();
    let schema_cursor = &mut schema_cursor.clone();

    let schema_node = schema_cursor.node();
    let input_node = input_cursor.node();

    let mut matches = json!({});
    let mut errors = Vec::new();

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
        errors.push(Error::SchemaError(
            SchemaError::MultipleMatchersInNodeChildren {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                received: schema_direct_children_code_node_count,
            },
        ));
        return (json!({}), errors);
    }

    if schema_direct_children_code_node_count == 1 && input_is_text_only {
        let (new_matches, new_errors) = validate_matcher_vs_text(
            input_cursor, 
            schema_cursor, 
            schema_str, 
            input_str, 
            got_eof
        );

        errors.extend(new_errors);

        // Add the validation matches to our top-level matches
        if let Some(obj) = new_matches.as_object() {
            for (key, value) in obj {
                matches
                    .as_object_mut()
                    .unwrap()
                    .insert(key.clone(), value.clone());
            }
        }

        return (new_matches, errors);
    } else if is_list_node(&schema_node) && is_list_node(&input_node) {
        let (new_matches, new_errors) = validate_matcher_vs_list(
            input_cursor, 
            schema_cursor, 
            schema_str, 
            input_str
        );

        errors.extend(new_errors);

        // Add the validation matches to our top-level matches
        if let Some(obj) = new_matches.as_object() {
            for (key, value) in obj {
                matches
                    .as_object_mut()
                    .unwrap()
                    .insert(key.clone(), value.clone());
            }
        }

        return (matches, errors);
    } else if schema_cursor.node().kind() == "text" {
        let (_, text_errors) = validate_text_vs_text(
            input_cursor, 
            schema_cursor, 
            schema_str, 
            input_str, 
            got_eof
        );
        errors.extend(text_errors);
    }

    if input_cursor.node().child_count() != schema_cursor.node().child_count() {
        if is_list_node(&schema_cursor.node()) && is_list_node(&input_cursor.node()) {
            // If both nodes are list nodes, don't handle them here
        } else if got_eof {
            // TODO: this feels wrong, we should check to make sure that when eof is false we detect nested incomplete nodes too
            errors.push(Error::SchemaViolation(
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
        let (new_matches, new_errors) = validate_node_vs_node(
            input_cursor, 
            schema_cursor, 
            schema_str, 
            input_str, 
            got_eof
        );

        errors.extend(new_errors);

        // Add the new matches to our top-level matches
        if let Some(obj) = new_matches.as_object() {
            for (key, value) in obj {
                matches
                    .as_object_mut()
                    .unwrap()
                    .insert(key.clone(), value.clone());
            }
        }

        loop {
            // TODO: handle case where one has more children than the other
            let input_had_sibling = input_cursor.goto_next_sibling();
            let schema_had_sibling = schema_cursor.goto_next_sibling();

            if input_had_sibling && schema_had_sibling {
                let (new_matches, new_errors) = validate_node_vs_node(
                    input_cursor, 
                    schema_cursor, 
                    schema_str, 
                    input_str, 
                    got_eof
                );

                errors.extend(new_errors);

                // Add the new matches to our top-level matches
                if let Some(obj) = new_matches.as_object() {
                    for (key, value) in obj {
                        matches
                            .as_object_mut()
                            .unwrap()
                            .insert(key.clone(), value.clone());
                    }
                }
            } else {
                break;
            }
        }
    }

    (matches, errors)
}