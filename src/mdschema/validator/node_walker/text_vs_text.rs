use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{SchemaViolationError, ValidationError},
    node_walker::{
        utils::{compare_node_kinds, compare_text_contents},
        ValidationResult,
    },
    utils::is_textual_node,
};

/// Validate a textual region of input against a textual region of schema.
///
/// Both the input cursor and schema cursor should either:
/// - Both point to textual nodes, like "emphasis", "text", or similar.
/// - Both point to textual containers, like "heading_content", "paragraph", or similar.
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_text_vs_text(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    // Check if both nodes are textual nodes
    let input_node = input_cursor.node();
    let schema_node = schema_cursor.node();

    if is_textual_node(&input_node) && is_textual_node(&schema_node) {
        // Both are textual nodes, validate them directly
        return validate_textual_nodes(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    // First, count the children to check for length mismatches
    let input_child_count = input_cursor.node().child_count();
    let schema_child_count = schema_cursor.node().child_count();

    // Handle node mismatches
    {
        // If we have reached the EOF:
        //   No difference in the number of children
        // else:
        //   We can have less input children
        //
        let children_len_mismatch_err =
            ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_child_count,
                actual: input_child_count,
            });
        if got_eof {
            // At EOF, children count must match exactly
            if input_child_count != schema_child_count {
                result.add_error(children_len_mismatch_err);
                return result;
            }
        } else {
            // Not at EOF: input can have fewer children, but not more
            if input_child_count > schema_child_count {
                result.add_error(children_len_mismatch_err);
                return result;
            }
        }
    }

    // Move cursors to first child
    if !input_cursor.goto_first_child() || !schema_cursor.goto_first_child() {
        // No children to validate
        result.schema_descendant_index = schema_cursor.descendant_index();
        result.input_descendant_index = input_cursor.descendant_index();
        return result;
    }

    // Recursively validate children. If they weren't textual, that means they're textual containers.
    let child_result = validate_textual_container_children(
        &mut input_cursor,
        &mut schema_cursor,
        schema_str,
        input_str,
        got_eof,
        input_child_count,
    );

    result.join_other_result(&child_result);

    // Move cursors back to parent and then to next sibling if needed
    if !got_eof && schema_cursor.goto_next_sibling() && !input_cursor.goto_next_sibling() {
        // If we haven't gotten EOF, and the schema has more siblings and the input
        // doesn't, then just leave cursors where they are, since more siblings will
        // need to be validated.
    } else {
        input_cursor.goto_parent();
        schema_cursor.goto_parent();
        input_cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();
    }

    result.schema_descendant_index = schema_cursor.descendant_index();
    result.input_descendant_index = input_cursor.descendant_index();
    result
}

/// Validate textual nodes directly (both nodes are textual). Checks the kind
/// and text contents.
fn validate_textual_nodes(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    let schema_node = schema_cursor.node();
    let input_node = input_cursor.node();

    // Check node kind first
    if let Some(error) = compare_node_kinds(&schema_node, &input_node, schema_cursor, input_cursor)
    {
        result.add_error(error);
        return result;
    }

    // Then compare text contents
    if let Some(error) = compare_text_contents(
        &schema_node,
        &input_node,
        schema_str,
        input_str,
        schema_cursor,
        input_cursor,
        got_eof,
    ) {
        result.add_error(error);
        return result;
    }

    result
}

/// Validate children of text containers.
///
/// The schema and input cursors are advanced to the first child of the current
/// node, and then the siblings are walked in lock step checking each textual
/// node against the other.
fn validate_textual_container_children(
    input_cursor: &mut TreeCursor,
    schema_cursor: &mut TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
    input_child_count: usize,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    let mut i = 0;
    loop {
        let is_last_input_node = i == input_child_count - 1;

        let schema_child = schema_cursor.node();
        let input_child = input_cursor.node();

        // Check if both are textual nodes
        if is_textual_node(&input_child) && is_textual_node(&schema_child) {
            // Both are textual, compare them directly
            if let Some(error) =
                compare_node_kinds(&schema_child, &input_child, schema_cursor, input_cursor)
            {
                result.add_error(error);
                return result;
            }

            if let Some(error) = compare_text_contents(
                &schema_child,
                &input_child,
                schema_str,
                input_str,
                schema_cursor,
                input_cursor,
                is_last_input_node && !got_eof,
            ) {
                result.add_error(error);
                return result;
            }
        } else {
            // If not both textual, we need to recurse into them
            let child_result = validate_text_vs_text(
                input_cursor,
                schema_cursor,
                schema_str,
                input_str,
                got_eof && is_last_input_node,
            );
            result.join_other_result(&child_result);
            if !result.errors.is_empty() {
                return result;
            }
        }

        // Move to next siblings
        let has_next_input = input_cursor.goto_next_sibling();
        let has_next_schema = schema_cursor.goto_next_sibling();

        if !has_next_input || !has_next_schema {
            break;
        }

        i += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::utils::parse_markdown;

    #[test]
    fn test_text_vs_text_with_text_nodes() {
        let schema_str = "Some *Literal* **Other**";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some *Different* **Other**";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // Expect a NodeContentMismatch error for "Literal" vs "Different"
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert!(expected.contains("Literal"));
                assert!(actual.contains("Different"));
            }
            _ => panic!("Expected a NodeContentMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_different_node_count() {
        // Schema has more nodes than input when eof is true
        let schema_str = "Some *Literal* **Other**";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some **Other**";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
                expected,
                actual,
                ..
            }) => {
                assert_eq!(*expected, 4); // text, italic, text, strong
                assert_eq!(*actual, 2); // text, strong
            }
            _ => panic!("Expected a ChildrenLengthMismatch error!"),
        }

        // When eof is false, it's okay if input has fewer nodes (still waiting for input)
        let schema_str = "Some *Literal* **Other**";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some *Literal*";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // eof is false
        );

        // Should not error because fewer nodes is okay when not at EOF
        assert!(
            result.errors.is_empty(),
            "Expected no errors when input has fewer nodes and eof=false"
        );

        // But if input has MORE nodes than schema when eof is false, it should error
        let schema_str = "Some *Literal*";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some *Literal* **Other**";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // eof is false
        );

        // Should error because input has more nodes than schema
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
                ..
            }) => {
                // This is what we expect
            }
            _ => panic!("Expected a ChildrenLengthMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_matching_paragraphs() {
        let schema_str = "This is a paragraph with some *emphasis* and **bold** text.";
        let input_str = "This is a paragraph with some *emphasis* and **bold** text.";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors for matching paragraphs"
        );
    }

    #[test]
    fn test_text_vs_text_with_mismatched_paragraphs_not_at_end() {
        let schema_str = "This is a paragraph with *emphasis* and some trailing text.";
        let input_str = "This is a paragraph with *different* and some trailing text.";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert!(expected.contains("emphasis"));
                assert!(actual.contains("different"));
            }
            _ => panic!("Expected a NodeContentMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_mismatched_paragraphs() {
        let schema_str = "Hello world";
        let input_str = "Goodbye world";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert_eq!(expected, "Hello world");
                assert_eq!(actual, "Goodbye world");
            }
            _ => panic!("Expected a NodeContentMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_bold_mismatch() {
        let schema_str = "This has **bold** text";
        let input_str = "This has *italic* text";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeTypeMismatch {
                expected,
                actual,
                ..
            }) => {
                assert_eq!(expected, "strong_emphasis");
                assert_eq!(actual, "emphasis");
            }
            _ => panic!("Expected a NodeTypeMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_identical_bold_paragraphs() {
        let schema_str = "this is **bold** text.";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "this is **bold** text.";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate directly to the paragraph nodes
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // Should have no errors for identical content
        assert!(result.errors.is_empty(), "Expected no errors for identical paragraphs, got: {:?}", result.errors);
    }
}
