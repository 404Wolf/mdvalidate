use log::trace;
use serde_json::json;
use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::{ValidationError, NodeContentMismatchKind, SchemaError, SchemaViolationError},
    matcher::{extract_text_matcher, get_everything_after_special_chars, ExtractorError, Matcher},
    node_walker::ValidationResult,
    utils::{is_last_node, waiting_at_end},
};

/// Validate a matcher node against a text node.
///
/// The schema cursor should point at:
/// - A text node, followed by a code node, maybe followed by a text node
/// - A code node, maybe followed by a text node
/// - A code node only
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_matcher_vs_text(
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

    // Mutable cursors that we can walk forward as we validate
    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

    // How far along we've validated the input. We'll update this as we go
    let mut input_byte_offset = input_cursor.node().byte_range().start;

    let schema_nodes = schema_cursor
        .node()
        .children(&mut schema_cursor.clone())
        .collect::<Vec<Node>>();

    // Descendant index of the input node, specifically the paragraph (not the interior text)
    let input_node_descendant_index = input_cursor.descendant_index();
    input_cursor.goto_first_child();

    let (schema_prefix_node, _, schema_suffix_node) = match extract_matcher_nodes(&schema_nodes) {
        Some(t) => t,
        None => {
            // TODO: add test
            result.add_error(ValidationError::SchemaError(SchemaError::MissingMatcher {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
            }));

            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();
            return result;
        }
    };

    // Preserve the cursor where it's pointing at the prefix node for error reporting
    let mut schema_cursor_at_prefix = schema_cursor.clone();
    schema_cursor_at_prefix.goto_first_child(); // paragraph -> text

    // Walk the schema cursor forward one if we had a prefix, since
    // extract_text_matcher requires the cursor to be located at a code node
    if schema_prefix_node.is_some() {
        schema_cursor.goto_first_child(); // paragraph -> text
        debug_assert_eq!(schema_cursor.node().kind(), "text");
        schema_cursor.goto_next_sibling(); // code_span
    } else {
        schema_cursor.goto_first_child(); // paragraph -> code_span
    }
    debug_assert_eq!(schema_cursor.node().kind(), "code_span");

    // Only do prefix verification if there is a prefix
    if let Some(schema_prefix_node) = schema_prefix_node {
        trace!("Validating prefix before matcher");

        let schema_prefix_str = &schema_str[schema_prefix_node.byte_range()];
        let input_prefix_str =
            input_str.get(input_byte_offset..input_byte_offset + schema_prefix_str.len());

        // Check that the input extends enough that we can cover the full prefix.
        if let Some(input_prefix_str) = input_prefix_str {
            // Do the actual prefix comparison
            if schema_prefix_str != input_prefix_str {
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor_at_prefix.descendant_index(),
                        input_index: input_node_descendant_index,
                        expected: schema_prefix_str.into(),
                        actual: input_prefix_str.into(),
                        kind: NodeContentMismatchKind::Prefix,
                    },
                ));

                // If prefix validation fails don't try to validate further.
                // TODO: In the future we could attempt to validate further anyway!
                result.schema_descendant_index = schema_cursor.descendant_index();
                result.input_descendant_index = input_cursor.descendant_index();
                return result;
            }

            input_byte_offset += schema_prefix_node.byte_range().len();
        } else if is_last_node(input_str, &input_cursor.node()) {
            // If we're waiting at the end, we can't validate the prefix yet
            let best_prefix_input_we_can_do = &input_str[input_byte_offset..];
            let best_prefix_length = best_prefix_input_we_can_do.len();
            let schema_prefix_partial = &schema_prefix_str[..best_prefix_length];

            if waiting_at_end(got_eof, input_str, &input_cursor) {
                trace!("Input prefix not long enough, but waiting at end of input");

                if schema_prefix_partial != best_prefix_input_we_can_do {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: schema_cursor_at_prefix.descendant_index(),
                            input_index: input_node_descendant_index,
                            expected: schema_prefix_str.into(),
                            actual: best_prefix_input_we_can_do.into(),
                            kind: NodeContentMismatchKind::Prefix,
                        },
                    ));
                }
            } else {
                trace!("Input node is complete but no more input left, reporting mismatch error");

                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor_at_prefix.descendant_index(),
                        input_index: input_node_descendant_index,
                        expected: schema_prefix_str.into(),
                        actual: best_prefix_input_we_can_do.into(),
                        kind: NodeContentMismatchKind::Prefix,
                    },
                ));
            }

            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();
            return result;
        }
    }

    // All input that comes after the expected prefix
    let input_after_prefix = input_str[input_byte_offset..].to_string();

    let matcher =
        match extract_text_matcher_into_schema_err(&schema_cursor, &input_cursor, schema_str) {
            Ok(m) => m,
            Err(e) => {
                trace!("Error extracting matcher: {:?}", e);
                result.add_error(e);
                result.schema_descendant_index = schema_cursor.descendant_index();
                result.input_descendant_index = input_cursor.descendant_index();
                return result;
            }
        };
    dbg!(matcher.pattern());

    // If the matcher is for a ruler, we should expect the entire input node to be a ruler
    if matcher.is_ruler() {
        trace!("Matcher is for a ruler, validating node type");

        if input_cursor.node().kind() != "thematic_break" {
            trace!("Input node is not a ruler, reporting type mismatch error");

            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: "thematic_break".into(),
                    actual: input_cursor.node().kind().into(),
                },
            ));
            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();

            return result;
        } else {
            // It's a ruler, no further validation needed
            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();

            return result;
        }
    }

    // Actually perform the match for the matcher
    match matcher.match_str(&input_after_prefix) {
        Some(matched_str) => {
            trace!("Matcher matched input string: {}", matched_str);

            input_byte_offset += matched_str.len();

            // Good match! Add the matched node to the matches (if it has an id)
            if let Some(id) = matcher.id() {
                trace!("Matcher matched input string: {}", matched_str);
                result.set_match(id, json!(matched_str));
            }
        }
        None => {
            trace!("Matcher did not match input string, reporting mismatch error");

            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: matcher.pattern().to_string(),
                    actual: input_after_prefix.into(),
                    kind: NodeContentMismatchKind::Matcher,
                },
            ));

            // TODO: should we validate further when we fail to match the matcher?
            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();
            return result;
        }
    }

    // Validate suffix if there is one
    if let Some(schema_suffix_node) = schema_suffix_node {
        schema_cursor.goto_next_sibling(); // code_span -> text
        debug_assert_eq!(schema_cursor.node().kind(), "text");

        // Everything that comes after the matcher
        let schema_suffix = {
            let text_node_after_code_node_str_contents =
                &schema_str[schema_suffix_node.byte_range()];
            // All text after the matcher node and maybe the text node right after it ("extras")
            get_everything_after_special_chars(text_node_after_code_node_str_contents).unwrap()
        };

        // Seek forward from the current input byte offset by the length of the suffix
        let input_suffix = &input_str[input_byte_offset..input_byte_offset + schema_suffix.len()];

        if schema_suffix != input_suffix {
            trace!(
                "Suffix mismatch: expected '{}', got '{}'",
                schema_suffix,
                input_suffix
            );

            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: schema_suffix.into(),
                    actual: input_suffix.into(),
                    kind: NodeContentMismatchKind::Suffix,
                },
            ));
        }
    }

    result.schema_descendant_index = schema_cursor.descendant_index();
    result.input_descendant_index = input_cursor.descendant_index();
    result
}

/// Extracts the matcher node and optional prefix/suffix nodes from the list of schema nodes.
///
/// Returns a tuple of (prefix_node, matcher_node, suffix_node) where each node can be None.
///
/// - `prefix_node`: A text node that comes before the matcher (optional)
/// - `matcher_node`: The code_span node that contains the matcher (required)
/// - `suffix_node`: A text node that comes after the matcher (optional)
///
/// The children must be in one of these forms:
/// - code_span (matcher only)
/// - text, code_span (prefix + matcher)
/// - code_span, text (matcher + suffix)
/// - text, code_span, text (prefix + matcher + suffix)
fn extract_matcher_nodes<'a>(
    schema_nodes: &[Node<'a>],
) -> Option<(Option<Node<'a>>, Node<'a>, Option<Node<'a>>)> {
    if schema_nodes.is_empty() {
        return None;
    }

    // Find code_span (should be one of the first two)
    let code_span_index = schema_nodes
        .iter()
        .position(|node| node.kind() == "code_span")?;

    let matcher_node = schema_nodes[code_span_index];

    let prefix_node = if code_span_index > 0 {
        Some(schema_nodes[0])
    } else {
        None
    };

    let suffix_node = if code_span_index + 1 < schema_nodes.len() {
        Some(schema_nodes[code_span_index + 1])
    } else {
        None
    };

    Some((prefix_node, matcher_node, suffix_node))
}

/// Extracts a text matcher from the schema cursor location, converting any
/// extraction errors into schema errors.
fn extract_text_matcher_into_schema_err(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<Matcher, ValidationError> {
    extract_text_matcher(schema_cursor, schema_str).map_err(|e| match e {
        ExtractorError::MatcherError(regex_err) => ValidationError::SchemaError(SchemaError::MatcherError {
            error: regex_err,
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
        }),
        ExtractorError::UTF8Error(_) => ValidationError::SchemaError(SchemaError::UTF8Error {
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
        }),
        ExtractorError::InvariantError => {
            unreachable!("we should know it's a code node")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::utils::parse_markdown;
    use serde_json::json;

    #[test]
    fn test_extract_matcher_nodes_code_span_only() {
        let schema_str = "`test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let schema_nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<Node>>();

        let result = extract_matcher_nodes(&schema_nodes);
        assert!(result.is_some());
        let (prefix, matcher, suffix) = result.unwrap();
        assert!(prefix.is_none());
        assert_eq!(matcher.kind(), "code_span");
        assert!(suffix.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_with_prefix_only() {
        let schema_str = "prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let schema_nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<Node>>();

        let result = extract_matcher_nodes(&schema_nodes);
        assert!(result.is_some());
        let (prefix, matcher, suffix) = result.unwrap();
        assert!(prefix.is_some());
        assert_eq!(prefix.unwrap().kind(), "text");
        assert_eq!(matcher.kind(), "code_span");
        assert!(suffix.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_with_prefix_and_suffix() {
        let schema_str = "prefix `test:/test/` suffix";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let schema_nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<Node>>();

        let result = extract_matcher_nodes(&schema_nodes);
        assert!(result.is_some());
        let (prefix, matcher, suffix) = result.unwrap();
        assert!(prefix.is_some());
        assert_eq!(prefix.unwrap().kind(), "text");
        assert_eq!(matcher.kind(), "code_span");
        assert!(suffix.is_some());
        assert_eq!(suffix.unwrap().kind(), "text");
    }

    #[test]
    fn test_extract_matcher_nodes_no_matcher_code_node() {
        let schema_str = "just text";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let schema_nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<Node>>();

        let result = extract_matcher_nodes(&schema_nodes);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_empty_list() {
        let result = extract_matcher_nodes(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_no_code_span() {
        let schema_str = "text only";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let schema_nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<Node>>();

        let result = extract_matcher_nodes(&schema_nodes);
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_matcher_vs_text_with_no_prefix_or_suffix() {
        let schema_str = "`test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "test";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix() {
        let schema_str = "prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_and_suffix() {
        let schema_str = "prefix `test:/test/` suffix";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test suffix";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_not_long_enough() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        // When EOF is not set, we're waiting for more input
        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        // When waiting for more input without EOF, we shouldn't report errors yet
        // (The validation is incomplete)
        assert!(
            errors.is_empty(),
            "Should not have errors when waiting for more input: {:?}",
            errors
        );
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_good_so_far() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix that is lo";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        // When EOF is not set, we're waiting for more input
        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        // When waiting for more input without EOF and prefix matches so far, we shouldn't report errors yet
        assert!(
            errors.is_empty(),
            "Should not have errors when waiting for more input: {:?}",
            errors
        );
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_but_bad_prefix() {
        let schema_str = "good prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "bad p";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        // When EOF is not set, we're waiting for more input
        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        // Even though we're waiting for more input, if the prefix doesn't match what we have,
        // we should report an error
        assert!(
            !errors.is_empty(),
            "Should have errors when prefix doesn't match even while waiting for more input"
        );
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_empty() {
        let schema_str = "prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document

        // When EOF is not set and input is empty, we're waiting for more input
        // When EOF is not set, we're waiting for more input
        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );

        // When waiting for more input without EOF, we shouldn't report errors yet
        assert!(
            result.errors.is_empty(),
            "Should not have errors when waiting for more input with empty input: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_ruler() {
        let schema_str = "`ruler`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "---";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> thematic_break

        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        assert!(
            result.errors.is_empty(),
            "Errors found: {:?}",
            result.errors
        );
        // Rulers don't capture matches
        assert_eq!(result.value, json!({}));
    }
}
