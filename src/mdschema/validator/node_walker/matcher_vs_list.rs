use serde_json::json;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{Error, SchemaViolationError}, 
    matcher::Matcher, 
    node_walker::ValidationResult, 
    utils::is_list_node
};

#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str), level = "debug", fields(
     input = %input_cursor.node().kind(),
     schema = %schema_cursor.node().kind()
 ), ret)]
pub fn validate_matcher_vs_list(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
) -> ValidationResult {
    let mut matches = json!({});
    let mut errors = Vec::new();

    // Called when we have our cursors pointed at a schema list node and an
    // input list node where the schema has only one child (the list item to
    // match against all input list items) and the input has (>=1) children.

    debug_assert!(
        is_list_node(&input_cursor.node()),
        "Input node is not a list, got {}",
        input_cursor.node().kind()
    );
    debug_assert!(
        is_list_node(&schema_cursor.node()),
        "Schema node is not a list, got {}",
        schema_cursor.node().kind()
    );

    let input_list_node = input_cursor.node();
    let input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    let input_list_children_count = input_list_node.children(&mut input_cursor.clone()).count();

    schema_cursor.goto_first_child(); // we're at a list_item
    assert_eq!(schema_cursor.node().kind(), "list_item");
    schema_cursor.goto_first_child(); // we're at a list_marker
    assert_eq!(schema_cursor.node().kind(), "list_marker");
    schema_cursor.goto_next_sibling(); // list_marker -> content (may be paragraph)

    // Get the matcher for this level
    let matcher_str = &schema_str[schema_cursor.node().child(0).unwrap().byte_range()].to_string();

    let child1_text = schema_cursor
        .node()
        .child(1)
        .map(|child1| &schema_str[child1.byte_range()]);

    let main_matcher = Matcher::new(matcher_str.as_str(), child1_text).unwrap(); // TODO: don't unwrap

    // When there are multiple nodes in the input list we require a
    // repeating matcher
    if !main_matcher.is_repeated() && input_list_children_count > 1 {
        errors.push(Error::SchemaViolation(
            SchemaViolationError::NonRepeatingMatcherInListContext {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
            },
        ));
    }

    let main_matcher_id = main_matcher.id();
    let mut main_items = Vec::new();
    let mut notes_objects = Vec::new();

    // Process each list item at this level
    for child in input_list_node.children(
        &mut input_cursor.clone(), // TODO: don't clone cursor
    ) {
        let mut child_cursor = child.walk();

        assert_eq!(child_cursor.node().kind(), "list_item");

        if !child_cursor.goto_first_child() {
            continue;
        }

        assert_eq!(child_cursor.node().kind(), "list_marker");

        if !child_cursor.goto_next_sibling() {
            continue;
        }

        // Process paragraph if present
        if child_cursor.node().kind() == "paragraph" {
            let paragraph_text = input_str[child_cursor.node().byte_range()].trim();

            main_items.push(json!(paragraph_text));

            let has_nested_list = child_cursor.goto_next_sibling();
            if has_nested_list && is_list_node(&child_cursor.node()) {
                // Save a copy of the schema cursor
                let mut schema_list_cursor = schema_cursor.clone();

                // Navigate to the nested list in the schema
                let schema_has_nested_list = schema_list_cursor.goto_next_sibling();

                if schema_has_nested_list && is_list_node(&schema_list_cursor.node()) {
                    // Process the nested list
                    let (nested_matches, nested_errors) = validate_matcher_vs_list(
                        &child_cursor,
                        &schema_list_cursor,
                        schema_str,
                        input_str,
                    );

                    // Add nested errors to our error collection
                    errors.extend(nested_errors);

                    // Add each nested match as a separate object in the notes_objects array
                    for (key, value) in nested_matches.as_object().unwrap() {
                        let mut note_obj = json!({});
                        note_obj[key] = value.clone();
                        notes_objects.push(note_obj);
                    }
                }
            }
        } else {
            todo!(
                "nested lists not supported, got {}",
                child_cursor.node().kind()
            )
        }
    }

    // Add all notes objects to the main items array
    for note_obj in notes_objects {
        main_items.push(note_obj);
    }

    // Add the main items to the result
    if let Some(id) = main_matcher_id {
        matches[id] = json!(main_items);
    }

    (matches, errors)
}