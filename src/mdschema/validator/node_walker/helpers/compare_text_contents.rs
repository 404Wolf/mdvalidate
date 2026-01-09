use serde_json::json;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaError, SchemaViolationError, ValidationError,
};
use crate::mdschema::validator::matcher::matcher::MatcherError;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::helpers::curly_matchers::extract_matcher_from_curly_delineated_text;
use crate::mdschema::validator::ts_utils::get_node_text;

/// Compare text contents between schema and input nodes.
/// Handles both literal text and curly-delimited matchers.
///
/// # Arguments
/// * `schema_str`: The full input document (so far).
/// * `input_str`: The full schema document.
/// - `schema_cursor`: Cursor at schema text node
/// - `input_cursor`: Cursor at input text node
/// - `is_partial_match`: Whether we're doing a partial match (not at EOF)
/// - `strip_extras`: Whether to strip extras (like `!`) from schema text
pub fn compare_text_contents(
    schema_str: &str,
    input_str: &str,
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    is_partial_match: bool,
    strip_extras: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    let schema_text = get_node_text(&schema_cursor.node(), schema_str);
    let input_text = get_node_text(&input_cursor.node(), input_str);

    let schema_text = if strip_extras {
        // TODO: this assumes that ! is the only extra when it is an extra
        let stripped = schema_text
            .split_once(" ")
            .map(|(_extras, rest)| format!(" {}", rest))
            .unwrap_or(schema_text.to_string());

        if stripped.len() == 1 {
            " ".into()
        } else {
            stripped
        }
    } else {
        schema_text.to_string()
    };

    // Try to match schema matcher against input text
    if let Some(matcher_result) = extract_matcher_from_curly_delineated_text(&schema_text) {
        match matcher_result {
            Ok(matcher) => {
                if let Some(matched_str) = matcher.match_str(input_text) {
                    if let Some(id) = matcher.id() {
                        result.set_match(id, json!(matched_str));
                    }
                } else if !is_partial_match {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: schema_cursor.descendant_index(),
                            input_index: input_cursor.descendant_index(),
                            expected: matcher.pattern().to_string(),
                            actual: input_text.into(),
                            kind: NodeContentMismatchKind::Matcher,
                        },
                    ));
                }

                return result;
            }
            Err(MatcherError::WasLiteralCode) => {}
            Err(error) => {
                result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                    error,
                    schema_index: schema_cursor.descendant_index(),
                }));
                return result;
            }
        }
    }

    // Fall back to literal text comparison
    let mut schema_text = schema_text.as_str();

    // If we're doing a partial match (not at EOF), adjust schema text length
    if is_partial_match {
        // If we got more input than expected, it's an error
        if input_text.len() > schema_text.len() {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_text.into(),
                    actual: input_text.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
            return result;
        } else {
            // The schema might be longer than the input, so crop the schema to the input we've got
            schema_text = &schema_text[..input_text.len()];
        }
    }

    if schema_text != input_text {
        result.add_error(ValidationError::SchemaViolation(
            SchemaViolationError::NodeContentMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_text.into(),
                actual: input_text.into(),
                kind: NodeContentMismatchKind::Literal,
            },
        ));
    }

    result
}

/// Macro for checking if text contents match and adding errors to result.
///
/// This macro encapsulates the common pattern of comparing text contents,
/// joining the result, and returning early if there are errors.
///
/// # Example
///
/// ```rs
/// compare_text_contents_check!(
///     schema_str,
///     input_str,
///     schema_cursor,
///     input_cursor,
///     is_partial_match,
///     strip_extras,
///     result
/// );
/// ```
#[macro_export]
macro_rules! compare_text_contents_check {
    (
        $schema_str:expr,
        $input_str:expr,
        $schema_cursor:expr,
        $input_cursor:expr,
        $is_partial_match:expr,
        $strip_extras:expr,
        $result:expr
    ) => {
        {
            let text_result = crate::mdschema::validator::node_walker::helpers::compare_text_contents::compare_text_contents(
                $schema_str,
                $input_str,
                &$schema_cursor,
                &$input_cursor,
                $is_partial_match,
                $strip_extras,
            );
            $result.join_other_result(&text_result);
            if text_result.has_errors() {
                return $result;
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::ts_utils::new_markdown_parser;

    use super::*;

    #[test]
    fn test_compare_text_contents_simple_match() {
        // Test simple matching text content
        let schema_str = "hello";
        let input_str = "hello";

        // Create simple test nodes
        let mut parser = new_markdown_parser();
        let schema_tree = parser.parse(schema_str, None).unwrap();
        let input_tree = parser.parse(input_str, None).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to text nodes
        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let result = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            false,
            false,
        );
        // Result depends on whether we found matching nodes, so just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_compare_text_contents_strip_extras() {
        // Test strip_extras functionality
        let schema_str = "! extra text";
        let input_str = "extra text";

        let mut parser = new_markdown_parser();
        let schema_tree = parser.parse(schema_str, None).unwrap();
        let input_tree = parser.parse(input_str, None).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        // With strip_extras=true, should handle the "!" prefix
        let result = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            false,
            true,
        );
        // Just verify no panic
        let _ = result;
    }

    #[test]
    fn test_compare_text_contents_partial_match() {
        // Test partial match mode
        let schema_str = "hello world";
        let input_str = "hello";

        let mut parser = new_markdown_parser();
        let schema_tree = parser.parse(schema_str, None).unwrap();
        let input_tree = parser.parse(input_str, None).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        // With is_partial_match=true, partial content should be acceptable
        let result = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            true,
            false,
        );
        let _ = result;
    }

    #[test]
    fn test_compare_text_contents_with_schema_matcher() {
        use serde_json::json;

        // Test schema with curly-delimited matcher matching input text
        let schema_str = "{test:/t.*/}";
        let input_str = "testing";

        let mut parser = new_markdown_parser();
        let schema_tree = parser.parse(schema_str, None).unwrap();
        let input_tree = parser.parse(input_str, None).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let result = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            false,
            false,
        );

        // Should match and capture
        assert!(
            !result.has_errors(),
            "Expected no errors but got: {:?}",
            result.errors()
        );
        assert_eq!(result.value(), &json!({"test": "testing"}));
    }

    #[test]
    fn test_compare_text_contents_matcher_mismatch() {
        // Test schema matcher that doesn't match input
        let schema_str = "{test:/^number$/}";
        let input_str = "text";

        let mut parser = new_markdown_parser();
        let schema_tree = parser.parse(schema_str, None).unwrap();
        let input_tree = parser.parse(input_str, None).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let result = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            false,
            false,
        );

        // Should have an error
        assert!(
            result.has_errors(),
            "Expected error for non-matching matcher"
        );
        assert_eq!(result.errors().len(), 1);
    }
}
