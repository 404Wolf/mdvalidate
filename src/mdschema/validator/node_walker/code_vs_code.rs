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
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_code_vs_code(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        input_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    let input_cursor = input_cursor.clone();
    let schema_cursor = schema_cursor.clone();

    debug_assert_eq!(input_cursor.node().kind(), "fenced_code_block");
    debug_assert_eq!(schema_cursor.node().kind(), "fenced_code_block");

    let input_extracted = extract_codeblock_contents(&input_cursor, input_str);
    let schema_extracted = extract_codeblock_contents(&schema_cursor, schema_str);

    let (
        Some((input_lang, (input_code, input_code_idx))),
        Some((schema_lang, (schema_code, schema_code_idx))),
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

    // Check if schema language has a matcher pattern
    let schema_lang_matcher = schema_lang
        .as_ref()
        .and_then(|(lang, _)| extract_matcher_from_curly_delineated_text(lang));

    // Check if code content has an ID for capture
    let code_id = extract_id_from_curly_braces(schema_code);

    // Validate language
    let lang_result = validate_language(
        input_lang,
        schema_lang,
        input_code_idx,
        schema_code_idx,
        &schema_lang_matcher,
    );
    result.join_other_result(&lang_result);

    // Validate code content
    if let Some(id) = code_id {
        // Schema has {id} - capture the input code
        result.set_match(id, json!(input_code));
    } else {
        // No ID - do literal comparison
        if input_code != schema_code {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: *schema_code_idx,
                    input_index: *input_code_idx,
                    expected: schema_code.into(),
                    actual: input_code.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
        }
    }

    result
}

/// Validates code block language specifications between schema and input.
///
/// This function handles two validation scenarios:
/// - **Literal matching**: When schema has no matcher pattern, performs exact string comparison
/// - **Pattern matching**: When schema has a matcher like `{lang:/\w+/}`, validates input against the pattern
///   and captures the matched value if the matcher has an ID
///
/// # Arguments
/// * `input_lang` - Optional tuple of (language string, byte index) from input code block
/// * `schema_lang` - Optional tuple of (language string, byte index) from schema code block
/// * `input_code_idx` - Byte index of input code block (used as fallback for error reporting)
/// * `schema_code_idx` - Byte index of schema code block (used as fallback for error reporting)
/// * `schema_lang_matcher` - Optional matcher extracted from schema language string
///
/// # Returns
/// A `ValidationResult` containing any validation errors and captured values
fn validate_language(
    input_lang: &Option<(String, usize)>,
    schema_lang: &Option<(String, usize)>,
    input_code_idx: &usize,
    schema_code_idx: &usize,
    schema_lang_matcher: &Option<Result<Matcher, MatcherError>>,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(*input_code_idx, *input_code_idx);

    match schema_lang_matcher {
        Some(Ok(schema_matcher)) => {
            // Schema has matcher, validate input against it
            if let Some((input_lang_str, _)) = input_lang {
                if let Some(match_result) = schema_matcher.match_str(input_lang_str) {
                    // Match succeeded - capture if matcher has an ID
                    if let Some(id) = schema_matcher.id() {
                        result.set_match(id, json!(match_result));
                    }
                } else {
                    // Match failed
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: schema_lang
                                .as_ref()
                                .map(|(_, idx)| *idx)
                                .unwrap_or(*schema_code_idx),
                            input_index: input_lang
                                .as_ref()
                                .map(|(_, idx)| *idx)
                                .unwrap_or(*input_code_idx),
                            expected: schema_lang
                                .as_ref()
                                .map(|(s, _)| s.clone())
                                .unwrap_or_default(),
                            actual: input_lang_str.clone(),
                            kind: NodeContentMismatchKind::Literal,
                        },
                    ));
                }
            }
        }
        Some(Err(error)) => {
            // Schema matcher is malformed
            result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error: error.clone(),
                schema_index: *schema_code_idx,
                input_index: *input_code_idx,
            }));
        }
        None => {
            // No matcher - do literal language comparison
            if let (
                Some((input_lang_str, input_lang_idx)),
                Some((schema_lang_str, schema_lang_idx)),
            ) = (input_lang, schema_lang)
            {
                if input_lang_str != schema_lang_str {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: *schema_lang_idx,
                            input_index: *input_lang_idx,
                            expected: schema_lang_str.clone(),
                            actual: input_lang_str.clone(),
                            kind: NodeContentMismatchKind::Literal,
                        },
                    ));
                }
            }
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

    use crate::mdschema::validator::{matcher::matcher::MatcherType, ts_utils::parse_markdown};

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

        match result.pattern() {
            MatcherType::Regex(regex) => {
                assert_eq!(regex.as_str(), "^test");
            }
            _ => panic!("Expected Regex pattern"),
        }

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

        let result = validate_code_vs_code(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
        );
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

        let result = validate_code_vs_code(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
        );
        assert!(result.errors.is_empty());
        assert_eq!(
            result.value,
            json!({ "lang": "rust", "code": "fn main() {}" })
        )
    }

    #[test]
    fn test_validate_language_literal_match() {
        // Both schema and input have literal languages that match
        let input_lang = Some(("rust".to_string(), 10));
        let schema_lang = Some(("rust".to_string(), 20));
        let input_code_idx = 15;
        let schema_code_idx = 25;
        let schema_lang_matcher = None;

        let result = validate_language(
            &input_lang,
            &schema_lang,
            &input_code_idx,
            &schema_code_idx,
            &schema_lang_matcher,
        );

        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_language_literal_mismatch() {
        // Schema and input have different literal languages
        let input_lang = Some(("rust".to_string(), 10));
        let schema_lang = Some(("python".to_string(), 20));
        let input_code_idx = 15;
        let schema_code_idx = 25;
        let schema_lang_matcher = None;

        let result = validate_language(
            &input_lang,
            &schema_lang,
            &input_code_idx,
            &schema_code_idx,
            &schema_lang_matcher,
        );

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert_eq!(expected, "python");
                assert_eq!(actual, "rust");
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_validate_language_pattern_match_with_capture() {
        // Schema has a matcher pattern with ID, input matches
        let input_lang = Some(("rust".to_string(), 10));
        let schema_lang = Some(("{lang:/\\w+/}".to_string(), 20));
        let input_code_idx = 15;
        let schema_code_idx = 25;
        let schema_lang_matcher = extract_matcher_from_curly_delineated_text("{lang:/\\w+/}");

        let result = validate_language(
            &input_lang,
            &schema_lang,
            &input_code_idx,
            &schema_code_idx,
            &schema_lang_matcher,
        );

        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({ "lang": "rust" }));
    }

    #[test]
    fn test_validate_language_pattern_match_without_capture() {
        // Schema has a matcher pattern without ID, input matches
        let input_lang = Some(("rust".to_string(), 10));
        let schema_lang = Some(("{/\\w+/}".to_string(), 20));
        let input_code_idx = 15;
        let schema_code_idx = 25;
        let schema_lang_matcher = extract_matcher_from_curly_delineated_text("{/\\w+/}");

        let result = validate_language(
            &input_lang,
            &schema_lang,
            &input_code_idx,
            &schema_code_idx,
            &schema_lang_matcher,
        );

        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_language_pattern_mismatch() {
        // Schema has a matcher pattern, input doesn't match
        let input_lang = Some(("123invalid".to_string(), 10));
        let schema_lang = Some(("{lang:/[a-z]+/}".to_string(), 20));
        let input_code_idx = 15;
        let schema_code_idx = 25;
        let schema_lang_matcher = extract_matcher_from_curly_delineated_text("{lang:/[a-z]+/}");

        let result = validate_language(
            &input_lang,
            &schema_lang,
            &input_code_idx,
            &schema_code_idx,
            &schema_lang_matcher,
        );

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert_eq!(expected, "{lang:/[a-z]+/}");
                assert_eq!(actual, "123invalid");
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_validate_language_no_languages() {
        // Both input and schema have no languages specified
        let input_lang = None;
        let schema_lang = None;
        let input_code_idx = 15;
        let schema_code_idx = 25;
        let schema_lang_matcher = None;

        let result = validate_language(
            &input_lang,
            &schema_lang,
            &input_code_idx,
            &schema_code_idx,
            &schema_lang_matcher,
        );

        assert!(result.errors.is_empty());
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_language_matcher_error() {
        // Schema has a malformed matcher - simulate with a manually constructed error
        let input_lang = Some(("rust".to_string(), 10));
        let schema_lang = Some(("{lang:invalid}".to_string(), 20));
        let input_code_idx = 15;
        let schema_code_idx = 25;

        // Create a matcher result with an error (using a real error variant)
        let matcher_error = MatcherError::MatcherInteriorRegexInvalid("invalid regex".to_string());
        let schema_lang_matcher = Some(Err(matcher_error));

        let result = validate_language(
            &input_lang,
            &schema_lang,
            &input_code_idx,
            &schema_code_idx,
            &schema_lang_matcher,
        );

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaError(SchemaError::MatcherError { .. }) => {
                // Expected error type
            }
            _ => panic!("Expected MatcherError"),
        }
    }
}
