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

#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, _got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_code_vs_code(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    _got_eof: bool,
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
        result.add_error(ValidationError::InternalInvariantViolated(
            format!(
                "Failed to extract code block contents from input or schema (input: {:?}, schema: {:?})",
                input_extracted, schema_extracted
            ),
        ));
        return result;
    };

    // Check if schema language has a matcher pattern
    let lang_matcher = schema_lang
        .as_ref()
        .and_then(|(lang, _)| extract_matcher_from_curly_delineated_text(lang));

    // Check if input language has a matcher pattern
    let input_lang_matcher = input_lang
        .as_ref()
        .and_then(|(lang, _)| extract_matcher_from_curly_delineated_text(lang));

    // Check if code content has an ID for capture
    let code_id = extract_id_from_curly_braces(schema_code);

    // Validate language
    validate_language(
        input_lang,
        schema_lang,
        input_code_idx,
        schema_code_idx,
        &input_lang_matcher,
        &lang_matcher,
        &mut result,
    );

    // If there were language errors, return early
    if !result.errors.is_empty() {
        return result;
    }

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

fn validate_language(
    input_lang: &Option<(String, usize)>,
    schema_lang: &Option<(String, usize)>,
    input_code_idx: &usize,
    schema_code_idx: &usize,
    input_lang_matcher: &Option<Result<Matcher, MatcherError>>,
    lang_matcher: &Option<Result<Matcher, MatcherError>>,
    result: &mut ValidationResult,
) {
    // Validate language with matcher
    match (lang_matcher, input_lang_matcher) {
        (Some(Ok(schema_matcher)), Some(Ok(input_matcher))) => {
            // Both have matchers - schema matcher validates against input matcher's ID if available
            if let Some(input_id) = input_matcher.id() {
                if let Some(match_result) = schema_matcher.match_str(input_id) {
                    if let Some(id) = schema_matcher.id() {
                        result.set_match(id, json!(match_result));
                    }
                } else {
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
                            actual: input_lang
                                .as_ref()
                                .map(|(s, _)| s.clone())
                                .unwrap_or_default(),
                            kind: NodeContentMismatchKind::Literal,
                        },
                    ));
                }
            }
        }
        (Some(Ok(schema_matcher)), None) => {
            // Schema has matcher, input is literal
            if let Some((input_lang_str, _)) = input_lang {
                if let Some(match_result) = schema_matcher.match_str(input_lang_str) {
                    if let Some(id) = schema_matcher.id() {
                        result.set_match(id, json!(match_result));
                    }
                } else {
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
                            actual: input_lang
                                .as_ref()
                                .map(|(s, _)| s.clone())
                                .unwrap_or_default(),
                            kind: NodeContentMismatchKind::Literal,
                        },
                    ));
                }
            }
        }
        (Some(Err(error)), _) | (_, Some(Err(error))) => {
            result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error: error.clone(),
                schema_index: *schema_code_idx,
                input_index: *input_code_idx,
            }));
            return;
        }
        (None, Some(Ok(_))) => {
            // Input has matcher but schema doesn't - this is an error
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
                    actual: input_lang
                        .as_ref()
                        .map(|(s, _)| s.clone())
                        .unwrap_or_default(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
            return;
        }
        (None, None) => {
            // No matcher for language, do literal language comparison if needed
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
                    return;
                }
            }
        }
    }
}

static CURLY_MATCHER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\{(?P<inner>.+?)\}(?P<suffix>.*)?$").unwrap());

static CURLY_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\{(?P<id>\w+)\}$").unwrap());

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
            true,
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
            true,
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
            true,
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
            true,
        );
        assert!(result.errors.is_empty());
        assert_eq!(
            result.value,
            json!({ "lang": "rust", "code": "fn main() {}" })
        )
    }
}
