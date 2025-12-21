use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{ValidationError, NodeContentMismatchKind, SchemaViolationError},
    node_walker::ValidationResult,
};

#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_list_vs_list(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let schema_text = &schema_str[schema_cursor.node().byte_range()];
    let input_text = &input_str[input_cursor.node().byte_range()];

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    if schema_text != input_text && got_eof {
        result.add_error(ValidationError::SchemaViolation(
            SchemaViolationError::NodeContentMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_text.into(),
                actual: input_text.into(),
                kind: NodeContentMismatchKind::Literal,
            },
        ));
    } else {
        input_cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();
    }

    result.schema_descendant_index = schema_cursor.descendant_index();
    result.input_descendant_index = input_cursor.descendant_index();
    result
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::errors::{
        ValidationError, NodeContentMismatchKind, SchemaViolationError,
    };
    use crate::mdschema::validator::{
        node_walker::text_vs_text::validate_text_vs_text, utils::parse_markdown,
    };
    use serde_json::json;

    #[test]
    fn test_text_vs_text_with_matching_paragraphs() {
        let schema_str = "Some Literal\nfoo";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some Literal\nfoo";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        let original_schema_descendant_index = schema_cursor.descendant_index();
        let original_input_descendant_index = input_cursor.descendant_index();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
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

        // Make sure we moved forward
        assert_eq!(
            result.schema_descendant_index,
            original_schema_descendant_index + 1
        );
        assert_eq!(
            result.input_descendant_index,
            original_input_descendant_index + 1
        );
    }

    #[test]
    fn test_text_vs_text_with_mismatched_paragraphs() {
        let schema_str = "Some Literal";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some OTHER Literal";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let mut input_cursor = input_tree.walk();
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        assert_eq!(result.value, json!({}));

        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index,
                input_index,
                expected,
                actual,
                kind,
            }) => {
                assert_eq!(*schema_index, 1);
                assert_eq!(*input_index, 1);
                assert_eq!(*expected, "Some Literal");
                assert_eq!(*actual, "Some OTHER Literal");
                assert_eq!(*kind, NodeContentMismatchKind::Literal);
            }
            _ => panic!("Expected a SchemaViolationError!"),
        }
    }
}
