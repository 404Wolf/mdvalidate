use serde_json::json;

use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::mdschema::validator::{
    errors::{NodeContentMismatchKind, SchemaError, SchemaViolationError, ValidationError},
    node_walker::{
        ValidationResult,
        helpers::curly_matchers::{
            extract_id_from_curly_braces, extract_matcher_from_curly_delineated_text,
        },
        validators::ValidatorImpl,
    },
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
pub(super) struct CodeVsCodeValidator;

impl ValidatorImpl for CodeVsCodeValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let _got_eof = got_eof;
        validate_code_vs_code_impl(walker)
    }
}

fn validate_code_vs_code_impl(walker: &ValidatorWalker) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(walker.input_cursor(), walker.input_cursor());

    let input_cursor = walker.input_cursor().clone();
    let schema_cursor = walker.schema_cursor().clone();
    let input_str = walker.input_str();
    let schema_str = walker.schema_str();

    #[cfg(feature = "invariant_violations")]
    if input_cursor.node().kind() != "fenced_code_block"
        || schema_cursor.node().kind() != "fenced_code_block"
    {
        crate::invariant_violation!(
            result,
            &input_cursor,
            &schema_cursor,
            "code validation expects fenced_code_block nodes"
        );
    }

    let input_extracted = match extract_codeblock_contents(&input_cursor, input_str) {
        Ok(value) => value,
        Err(error) => {
            result.add_error(error);
            return result;
        }
    };
    let schema_extracted = match extract_codeblock_contents(&schema_cursor, schema_str) {
        Ok(value) => value,
        Err(error) => {
            result.add_error(error);
            return result;
        }
    };

    let (
        Some((input_lang, (input_code, input_code_descendant_index))),
        Some((schema_lang, (schema_code, schema_code_descendant_index))),
    ) = (&input_extracted, &schema_extracted)
    else {
        #[cfg(feature = "invariant_violations")]
        {
            // The only reason the "entire thing" would be wrong is because we're
            // doing something wrong in our usage of it. That would be a bug!
            crate::invariant_violation!(
                result,
                &input_cursor,
                &schema_cursor,
                "Failed to extract code block contents from input or schema (input: {:?}, schema: {:?})",
                input_extracted,
                schema_extracted
            );
        }

        #[cfg(not(feature = "invariant_violations"))]
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::node_walker::validators::test_utils::ValidatorTester;
    use crate::mdschema::validator::ts_utils::is_codeblock_node;

    use super::*;

    #[test]
    fn test_validate_code_vs_code_literal_same() {
        // positive case: input and schema are identical
        let input_str = "```rust\nfn main() {}\n```";
        let schema_str = "```rust\nfn main() {}\n```";

        let (value, errors, _farthest_reached_pos) =
            ValidatorTester::<CodeVsCodeValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(i, s)| {
                    assert!(is_codeblock_node(i));
                    assert!(is_codeblock_node(s));
                })
                .validate_complete()
                .destruct();

        assert_eq!(errors, vec![], "Expected no errors, got {:?}", errors);
        assert_eq!(value, json!({}));

        // negative case: change input so it is not the same as the schema
        let input_str_negative = "```rust\nfn main() { println!(\"hi\"); }\n```";
        let (_value, errors, _farthest_reached_pos) =
            ValidatorTester::<CodeVsCodeValidator>::from_strs(schema_str, input_str_negative)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(i, s)| {
                    assert!(is_codeblock_node(i));
                    assert!(is_codeblock_node(s));
                })
                .validate_complete()
                .destruct();

        assert!(!errors.is_empty());
    }

    #[test]
    fn test_validate_code_vs_code_matcher_lang() {
        let schema_str = r#"```{lang:/\w+/}
fn main() {}
```"#;
        let input_str = r#"```rust
fn main() {}
```"#;
        let (value, errors, _farthest_reached_pos) =
            ValidatorTester::<CodeVsCodeValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(i, s)| {
                    assert!(is_codeblock_node(i));
                    assert!(is_codeblock_node(s));
                })
                .validate_complete()
                .destruct();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({ "lang": "rust" }));
    }

    #[test]
    fn test_validate_code_vs_code_matcher_lang_and_id_content() {
        let schema_str = r#"```{lang:/\w+/}
{code}
```"#;
        let input_str = r#"```rust
fn main() {}
```"#;
        let (value, errors, _farthest_reached_pos) =
            ValidatorTester::<CodeVsCodeValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(i, s)| {
                    assert!(is_codeblock_node(i));
                    assert!(is_codeblock_node(s));
                })
                .validate_complete()
                .destruct();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({ "lang": "rust", "code": "fn main() {}" }))
    }
}
