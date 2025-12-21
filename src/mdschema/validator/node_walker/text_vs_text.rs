use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
    node_walker::ValidationResult,
};

/// Validate a textual region of input against a textual region of schema.
///
/// A "textual" region is a sequence of text content nodes, like
/// "heading_content", "paragraph", or similar, where cursors are pointing at
/// two text "containers".
///
/// # Arguments
///
/// * `input_cursor`: The cursor pointing to the input text container, like a "paragraph".
/// * `schema_cursor`: The cursor pointing to the schema text container, like a "paragraph".
/// * `schema_str`: The schema string.
/// * `input_str`: The input string.
/// * `got_eof`: Whether the input cursor provided's end is the end of the *entire* input.
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

    // If we have
    //
    // Schema:          Input:
    // ```md            ```md
    // ^foo bar buzz    ^fo
    // - test
    // ```              ```
    //
    // We should not error since we are still waiting on more input.

    // Move cursors to first child
    if !input_cursor.goto_first_child() || !schema_cursor.goto_first_child() {
        // No children to validate
        result.schema_descendant_index = schema_cursor.descendant_index();
        result.input_descendant_index = input_cursor.descendant_index();
        return result;
    }

    let mut has_next_input;
    let mut has_next_schema;
    let mut i = 0;
    loop {
        let is_last_input_node = i == input_child_count - 1;

        let schema_child = schema_cursor.node();
        let input_child = input_cursor.node();

        let (mut schema_child_text, input_child_text) = match (
            schema_child.utf8_text(schema_str.as_bytes()),
            input_child.utf8_text(input_str.as_bytes()),
        ) {
            (Ok(text), Ok(other)) => (text, other),
            (Err(_), _) | (_, Err(_)) => {
                return result;
            }
        };

        // If we're on the last child, and haven't reached EOF, don't validate
        // this full node just yet
        if is_last_input_node && !got_eof {
            // If we got more input than expected, it's an error
            if input_child_text.len() > schema_child_text.len() {
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_cursor.descendant_index(),
                        expected: schema_child_text.into(),
                        actual: input_child_text.into(),
                        kind: NodeContentMismatchKind::Literal,
                    },
                ));

                return result;
            } else {
                // The schema might be longer than the input, so crop the schema to the input we've got
                schema_child_text = &schema_child_text[..input_child_text.len()];
            }
        };

        // Check that they are the same *kind* of text node
        if schema_child.kind() != input_child.kind() {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_child.kind().into(),
                    actual: input_child.kind().into(),
                },
            ));

            return result;
        }

        if schema_child_text != input_child_text {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_child_text.into(),
                    actual: input_child_text.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));

            return result;
        }

        // Move to next siblings
        has_next_input = input_cursor.goto_next_sibling();
        has_next_schema = schema_cursor.goto_next_sibling();

        if !has_next_input || !has_next_schema {
            break;
        }

        i += 1;
    }

    // Move cursors back to parent and then to next sibling.
    if !got_eof && has_next_schema && !has_next_input {
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

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::errors::{
        NodeContentMismatchKind, SchemaViolationError, ValidationError,
    };
    use crate::mdschema::validator::{
        node_walker::text_vs_text::validate_text_vs_text, utils::parse_markdown,
    };
    use serde_json::json;

    #[test]
    fn test_text_vs_text_with_different_node_count() {
        let schema_str = "Some *Literal* **Other**";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some Literal _test_";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        // Should error because children length mismatch when not at EOF
        assert!(
            !result.errors.is_empty(),
            "Expected errors for different node count"
        );

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
        assert!(
            !result.errors.is_empty(),
            "Expected errors when input has more nodes than schema even with eof=false"
        );
    }

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
    fn test_text_vs_text_with_mismatched_paragraphs_not_at_end() {
        let input_str = "Some Lit\n- foo";
        let input_tree = parse_markdown(input_str).unwrap();

        let schema_str = "Some Literal\n- bar";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let mut input_cursor = input_tree.walk();
        let mut schema_cursor = schema_tree.walk();

        let original_schema_descendant_index = schema_cursor.descendant_index();
        let original_input_descendant_index = input_cursor.descendant_index();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({}));
        // We don't move forward, since we were partially matched at the last
        // node and weren't able to finish the validation
        assert_eq!(
            result.schema_descendant_index,
            original_schema_descendant_index + 3
        );
        assert_eq!(
            result.input_descendant_index,
            original_input_descendant_index + 3
        );

        // But if the prefix for the input doesn't match we should error early
        let input_str = "Some Wro\n- foo";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut input_cursor = input_tree.walk();
        let mut schema_cursor = schema_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index,
                input_index,
                expected,
                actual,
                kind,
            }) => {
                assert_eq!(*schema_index, 2);
                assert_eq!(*input_index, 2);
                assert_eq!(*expected, "Some Lit");
                assert_eq!(*actual, "Some Wro");
                assert_eq!(*kind, NodeContentMismatchKind::Literal);
            }
            _ => panic!("Unexpected error kind"),
        }
    }

    #[test]
    fn test_text_vs_text_with_mismatched_paragraphs() {
        let schema_str = "Some Literal";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some OTHER Literal";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut input_cursor = input_tree.walk();
        let mut schema_cursor = schema_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

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
                assert_eq!(*schema_index, 2);
                assert_eq!(*input_index, 2);
                assert_eq!(*expected, "Some Literal");
                assert_eq!(*actual, "Some OTHER Literal");
                assert_eq!(*kind, NodeContentMismatchKind::Literal);
            }
            _ => panic!("Expected a SchemaViolationError!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_bold_mismatch() {
        let input_str = "This is *bold* text.";
        let schema_str = "This is **bold** text.";

        let input_tree = parse_markdown(input_str).unwrap();
        let schema_tree = parse_markdown(schema_str).unwrap();

        let mut input_cursor = input_tree.walk();
        let mut schema_cursor = schema_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph

        // For the first part of this, EOF=true, and we need the match already
        let result = validate_text_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        assert!(!result.errors.is_empty());
        assert_eq!(result.value, json!({}));

        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeTypeMismatch {
                schema_index,
                input_index,
                expected,
                actual,
            }) => {
                assert_eq!(*schema_index, 3);
                assert_eq!(*input_index, 3);
                assert_eq!(*expected, "strong_emphasis"); // we wanted bold (**)
                assert_eq!(*actual, "emphasis"); // we got italic (*)
            }
            _ => panic!("Expected a SchemaViolationError!"),
        }

        // The last node, in this case "text.", is allowed to be different if
        // EOF=false, as long as it's a partial match (so far). So let's make it
        // different.
        let input_str = "This is **bold** tex";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        input_cursor.goto_first_child(); // document -> paragraph
        let result = validate_text_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );
        assert!(
            result.errors.is_empty(),
            "Expected no errors, got {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}), "Expected empty JSON object");

        // But if we are wrong so far the validation should fail early
        let input_str = "This is **bold** buzz";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        input_cursor.goto_first_child(); // document -> paragraph
        let result = validate_text_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );
        assert!(
            !result.errors.is_empty(),
            "Expected errors, got {:?}",
            result.errors
        );
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index: _,
                input_index: _,
                expected,
                actual,
                kind,
            }) => {
                assert_eq!(*expected, " text");
                assert_eq!(*actual, " buzz");
                assert_eq!(*kind, NodeContentMismatchKind::Literal);
            }
            _ => panic!("Expected a SchemaViolationError!"),
        }

        assert_eq!(result.value, json!({}), "Expected empty JSON object");
    }
}
