use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{NodeContentMismatchKind, SchemaError, SchemaViolationError, ValidationError},
    matcher::matcher::{Matcher, MatcherError},
    node_walker::ValidationResult,
    ts_utils::extract_codeblock_contents,
};

/// Validate a code block against a schema code block.
///
/// Compares both the language specifier and code content between input and schema.
/// The schema can use matchers in the language field to capture or validate patterns,
/// and can use `{id}` in the code content to capture the entire code block.
///
/// Language validation supports:
/// - Literal matching: both must have the same language string
/// - Pattern matching: schema can use `{lang:/pattern/}` to match and optionally capture
///
/// Code content validation supports:
/// - Literal matching: exact string comparison
/// - Capture: schema uses `{id}` to capture input code without validation
///
/// # Examples
///
/// **Pattern matching with capture:**
/// ```markdown
/// Schema:
/// ```{lang:/\w+/}
/// {code}
/// ```
///
/// Input:
/// ```python
/// print("hello")
/// ```
///
/// Captures: { "lang": "python", "code": "print(\"hello\")" }
///
/// Note you cannot yet enforce regex on the actual code content.
/// ```
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str), level = "debug", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
pub fn validate_code_vs_code(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(input_cursor, input_cursor);

    let input_cursor = input_cursor.clone();
    let schema_cursor = schema_cursor.clone();

    debug_assert_eq!(input_cursor.node().kind(), "fenced_code_block");
    debug_assert_eq!(schema_cursor.node().kind(), "fenced_code_block");

    let input_extracted = extract_codeblock_contents(&input_cursor, input_str);
    let schema_extracted = extract_codeblock_contents(&schema_cursor, schema_str);

    let (
        Some((input_lang, (input_code, input_code_descendant_index))),
        Some((schema_lang, (schema_code, schema_code_descendant_index))),
    ) = (&input_extracted, &schema_extracted)
    else {
        // The only reason the "entire thing" would be wrong is because we're
        // doing something wrong in our usage of it. That would be a bug!
        result.add_error(ValidationError::InternalInvariantViolated(
            format!(
                "Failed to extract code block contents from input or schema (input: {:?}, schema: {:?})",
                input_extracted, schema_extracted
            ),
        ));
        return result;
    };

    // Check if schema language has a matcher pattern (like {lang:/\w*/})
    match schema_lang.as_ref().and_then(|(lang, descendant_index)| {
        extract_matcher_from_curly_delineated_text(lang)
            .map(|matcher_result| (matcher_result, descendant_index))
    }) {
        // If the schema has a matcher, and we were able to extract it, do matching!
        Some((Ok(schema_lang_matcher), schema_lang_descendant_index)) => {
            // Schema has matcher, validate input against it
            if let Some((input_lang_str, input_lang_descendant_index)) = input_lang {
                if let Some(match_result) = schema_lang_matcher.match_str(input_lang_str) {
                    // Match succeeded - capture if matcher has an ID
                    if let Some(id) = schema_lang_matcher.id() {
                        result.set_match(id, json!(match_result));
                    }
                } else {
                    // Match failed
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: *schema_lang_descendant_index,
                            input_index: *input_lang_descendant_index,
                            expected: schema_lang
                                .as_ref()
                                .map(|(s, _)| s.clone())
                                .unwrap_or_default(),
                            actual: input_lang_str.clone(),
                            kind: NodeContentMismatchKind::Literal,
                        },
                    ));
                    return result;
                }
            }
        }
        // If the schema has a matcher, but we had an issue extracting it, raise an error
        Some((Err(error), schema_lang_descendant_index)) => {
            // Schema matcher is malformed
            result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error: error.clone(),
                schema_index: *schema_lang_descendant_index,
            }));
            return result;
        }
        None => {
            // No matcher - do literal language comparison, treating as a literal string
            if let (
                Some((input_lang_str, input_lang_descendant_index)),
                Some((schema_lang_str, schema_lang_descendant_index)),
            ) = (input_lang, schema_lang)
            {
                if input_lang_str != schema_lang_str {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: *schema_lang_descendant_index,
                            input_index: *input_lang_descendant_index,
                            expected: schema_lang_str.clone(),
                            actual: input_lang_str.clone(),
                            kind: NodeContentMismatchKind::Literal,
                        },
                    ));
                }
            }
        }
    }

    // Validate code content. This is much simpler! If the area in the
    //
    // ```{id:/.*/}
    // {test}
    // ```
    //
    // Has {test} (a string surrounded by curly braces) then store the code in
    // that key in the result.
    if let Some(id) = extract_id_from_curly_braces(schema_code) {
        // Schema has {id} - capture the input code
        result.set_match(id, json!(input_code));
    } else {
        // No ID - do literal comparison of the code, treating it as a literal string
        if input_code != schema_code {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: *schema_code_descendant_index,
                    input_index: *input_code_descendant_index,
                    expected: schema_code.into(),
                    actual: input_code.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
        }
    }

    result
}

static CURLY_MATCHER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\{(?P<inner>.+?)\}(?P<suffix>.*)?$").unwrap());

static CURLY_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\{(?P<id>\w+)\}$").unwrap());

fn extract_matcher_from_curly_delineated_text(
    input: &str,
) -> Option<Result<Matcher, MatcherError>> {
    let caps = CURLY_MATCHER.captures(input)?;

    let matcher_str = caps.name("inner").map(|m| m.as_str()).unwrap_or("").trim();
    let suffix = caps.name("suffix").map(|m| m.as_str());

    Some(Matcher::try_from_pattern_and_suffix_str(
        &format!("`{}`{}", matcher_str, suffix.unwrap_or("")),
        suffix,
    ))
}

/// Extract a simple ID from curly braces like `{id}` for code content capture
fn extract_id_from_curly_braces(input: &str) -> Option<&str> {
    let caps = CURLY_ID.captures(input)?;
    caps.name("id").map(|m| m.as_str())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::ts_utils::parse_markdown;

    use super::*;

    #[test]
    fn test_extract_id_from_curly_braces() {
        let input = "{test}";
        let result = extract_id_from_curly_braces(input).unwrap();
        assert_eq!(result, "test");

        let input = "";
        let result = extract_id_from_curly_braces(input);
        assert!(result.is_none());

        let input = "{a}{b}{c}";
        let result = extract_id_from_curly_braces(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_matcher_from_curly_delineated_text() {
        let input = "{id:/test/}{1,2}";
        let result = extract_matcher_from_curly_delineated_text(input)
            .unwrap()
            .unwrap();
        assert_eq!(result.id(), Some("id"));

        // Check that the pattern displays correctly
        assert_eq!(format!("{}", result.pattern()), "^test");

        assert!(result.extras().had_min_max());
        assert_eq!(result.extras().min_items(), Some(1));
        assert_eq!(result.extras().max_items(), Some(2));
    }

    #[test]
    fn test_validate_code_vs_code_literal_same() {
        // positive case: input and schema are identical
        let input_str = "```rust\nfn main() {}\n```";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        assert!(input_cursor.goto_first_child()); // move to fenced_code_block

        let schema_str = "```rust\nfn main() {}\n```";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        assert!(schema_cursor.goto_first_child()); // move to fenced_code_block

        let result = validate_code_vs_code(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str.into(),
            input_str.into(),
        );
        assert!(
            result.errors.is_empty(),
            "Expected no errors, got {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));

        // negative case: change input so it is not the same as the schema
        let input_str_negative = "```rust\nfn main() { println!(\"hi\"); }\n```";
        let input_tree_negative = parse_markdown(input_str_negative).unwrap();
        let mut input_cursor_negative = input_tree_negative.walk();
        assert!(input_cursor_negative.goto_first_child()); // move to fenced_code_block

        // Recreate schema cursor to ensure it's at the correct position
        let schema_tree_again = parse_markdown(schema_str).unwrap();
        let mut schema_cursor_again = schema_tree_again.walk();
        assert!(schema_cursor_again.goto_first_child()); // move to fenced_code_block

        let result_negative = validate_code_vs_code(
            &mut input_cursor_negative,
            &mut schema_cursor_again,
            schema_str.into(),
            input_str_negative.into(),
        );

        assert!(!result_negative.errors.is_empty());
    }

    #[test]
    fn test_validate_code_vs_code_matcher_lang() {
        let schema_str = r#"```{lang:/\w+/}
fn main() {}
```"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        assert!(schema_cursor.goto_first_child()); // move to fenced_code_block

        let input_str = r#"```rust
fn main() {}
```"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        assert!(input_cursor.goto_first_child()); // move to fenced_code_block

        let result =
            validate_code_vs_code(&mut input_cursor, &mut schema_cursor, schema_str, input_str);
        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({ "lang": "rust" }))
    }

    #[test]
    fn test_validate_code_vs_code_matcher_lang_and_id_content() {
        let schema_str = r#"```{lang:/\w+/}
{code}
```"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        assert!(schema_cursor.goto_first_child()); // move to fenced_code_block

        let input_str = r#"```rust
fn main() {}
```"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        assert!(input_cursor.goto_first_child()); // move to fenced_code_block

        let result =
            validate_code_vs_code(&mut input_cursor, &mut schema_cursor, schema_str, input_str);
        assert!(result.errors.is_empty());
        assert_eq!(
            result.value,
            json!({ "lang": "rust", "code": "fn main() {}" })
        )
    }
}
