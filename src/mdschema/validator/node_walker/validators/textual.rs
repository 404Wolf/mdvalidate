use log::trace;
use serde_json::json;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::matcher::matcher::{Matcher, get_everything_after_special_chars};
use crate::mdschema::validator::ts_utils::{
    get_next_node, is_code_node, is_last_node, is_text_node,
};
use crate::mdschema::validator::{
    errors::*,
    node_walker::ValidationResult,
    ts_utils::{both_are_textual_containers, waiting_at_end},
    utils::{compare_node_kinds, compare_text_contents},
};

/// Validate two textual elements.
///
/// # Algorithm
/// 1. Check if the schema node is at a matcher literal. If it is, then check if the next node is a text node. If it is, attempt to gather together
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
pub fn validate_textual_container_vs_textual_container(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    if both_are_textual_containers(&schema_cursor.node(), &input_cursor.node()) {
        todo!()
    } else {
        if let Some(error) =
            compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str)
        {
            trace!(
                "Node kinds do not match. Got schema_kind={} and input_kind={}",
                schema_cursor.node().kind(),
                input_cursor.node().kind()
            );
            result.add_error(error);

            return result;
        }

        let is_partial_match = waiting_at_end(got_eof, input_str, &input_cursor);
        if let Some(error) = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            is_partial_match,
            false,
        ) {
            trace!(
                "Text contents do not match. Got schema_text={} and input_text={}",
                schema_str, input_str
            );
            result.add_error(error);

            return result;
        }

        result
    }
}

/// Validate a sequence of nodes that includes a matcher node against a text
/// node. This is used for when we have 1-3 nodes, where there may be a center
/// node that is a code node that is a matcher.
///
/// The schema cursor should point at:
/// Validate text using a matcher pattern from the schema.
///
/// Called by `validate_text_vs_text` when a matcher group is detected in the schema.
/// A matcher group consists of text-code-text nodes where the code contains a pattern.
///
/// The matcher can match against input text and optionally capture the matched value.
/// Supports prefix/suffix matching and various pattern types (regex, literal, etc.).
pub fn validate_matcher_vs_text<'a>(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let schema_prefix_node = {
        if is_code_node(&schema_cursor.node()) {
            None
        } else if is_text_node(&schema_cursor.node()) {
            Some(schema_cursor.node())
        } else {
            unreachable!(
                "only should be called with `code_span` or `text` but got {:?}",
                schema_cursor.node()
            )
        }
    };

    let schema_suffix_node = {
        // If there is a prefix, this comes two nodes later
        if schema_prefix_node.is_some() {
            get_next_node(&schema_cursor).and_then(|n| get_next_node(&n.walk()))
        } else {
            get_next_node(&schema_cursor)
        }
    };

    let matcher = {
        // Make sure we create the matcher when we are pointing at a `code_span`
        let mut schema_cursor = schema_cursor.clone();
        if schema_prefix_node.is_some() {
            schema_cursor.goto_next_sibling();
        }
        Matcher::try_from_schema_cursor(&schema_cursor, schema_str)
    };

    // How far along we've validated the input. We'll update this as we go
    let mut input_byte_offset = input_cursor.node().byte_range().start;

    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    // Mutable cursors that we can walk forward as we validate
    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

    match matcher {
        Ok(matcher) => {
            // Descendant index of the input node, specifically the paragraph (not the interior text)
            let input_node_descendant_index = input_cursor.descendant_index();
            input_cursor.goto_first_child();

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
                        trace!(
                            "Prefix mismatch: expected '{}', got '{}'",
                            schema_prefix_str, input_prefix_str
                        );
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
                        result.sync_cursor_pos(&input_cursor, &schema_cursor);

                        return result;
                    }

                    trace!("Prefix matched successfully");
                    input_byte_offset += schema_prefix_node.byte_range().len();
                } else if is_last_node(input_str, &input_cursor.node()) {
                    // If we're waiting at the end, we can't validate the prefix yet
                    let best_prefix_input_we_can_do = &input_str[input_byte_offset..];
                    let best_prefix_length = best_prefix_input_we_can_do.len();
                    let schema_prefix_partial = &schema_prefix_str[..best_prefix_length];

                    if waiting_at_end(got_eof, input_str, &input_cursor) {
                        trace!("Input prefix not long enough, but waiting at end of input");

                        if schema_prefix_partial != best_prefix_input_we_can_do {
                            trace!(
                                "Prefix partial mismatch at end: expected '{}', got '{}'",
                                schema_prefix_partial, best_prefix_input_we_can_do
                            );
                            result.add_error(ValidationError::SchemaViolation(
                                SchemaViolationError::NodeContentMismatch {
                                    schema_index: schema_cursor_at_prefix.descendant_index(),
                                    input_index: input_node_descendant_index,
                                    expected: schema_prefix_str.into(),
                                    actual: best_prefix_input_we_can_do.into(),
                                    kind: NodeContentMismatchKind::Prefix,
                                },
                            ));
                        } else {
                            trace!("Prefix partial match successful, deferring full validation");
                        }
                    } else {
                        trace!(
                            "Input node is complete but no more input left, reporting mismatch error"
                        );

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

                    result.sync_cursor_pos(&input_cursor, &schema_cursor);
                    return result;
                }
            }

            // Don't validate after the prefix if there isn't enough content
            if input_byte_offset >= input_str.len() {
                if got_eof {
                    let schema_prefix_str = schema_prefix_node
                        .map(|node| &schema_str[node.byte_range()])
                        .unwrap_or("");

                    let best_prefix_input_we_can_do =
                        &input_str[input_cursor.node().byte_range().start..];

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

                return result;
            }

            // All input that comes after the expected prefix
            let input_after_prefix =
                input_str[input_byte_offset..input_cursor.node().byte_range().end].to_string();

            if !got_eof && input_after_prefix.contains("`") {
                return result;
            } else {
                trace!(
                    "xAttempting to match the input \"{}\"'s prefix, which is {}",
                    input_cursor.node().utf8_text(input_str.as_bytes()).unwrap(),
                    input_after_prefix
                );
            }

            // Actually perform the match for the matcher
            match matcher.match_str(&input_after_prefix) {
                Some(matched_str) => {
                    trace!(
                        "Matcher successfully matched input: '{}' (length={})",
                        matched_str,
                        matched_str.len()
                    );

                    input_byte_offset += matched_str.len();

                    // Good match! Add the matched node to the matches (if it has an id)
                    if let Some(id) = matcher.id() {
                        trace!("Storing match for id '{}': '{}'", id, matched_str);
                        result.set_match(id, json!(matched_str));
                    } else {
                        trace!("Matcher has no id, not storing match");
                    }
                }
                None => {
                    trace!(
                        "Matcher did not match input string: pattern={}, input='{}'",
                        matcher.pattern().to_string(),
                        input_after_prefix
                    );
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
                    result.sync_cursor_pos(&input_cursor, &schema_cursor);

                    return result;
                }
            }

            // Validate suffix if there is one
            if let Some(schema_suffix_node) = schema_suffix_node {
                trace!("Validating suffix");
                schema_cursor.goto_next_sibling(); // code_span -> text
                debug_assert_eq!(schema_cursor.node().kind(), "text");

                // Everything that comes after the matcher
                let schema_suffix = {
                    let text_node_after_code_node_str_contents =
                        &schema_str[schema_suffix_node.byte_range()];
                    // All text after the matcher node and maybe the text node right after it ("extras")
                    get_everything_after_special_chars(text_node_after_code_node_str_contents)
                        .unwrap()
                };

                // Seek forward from the current input byte offset by the length of the suffix
                let input_suffix =
                    &input_str[input_byte_offset..input_byte_offset + schema_suffix.len()];

                if schema_suffix != input_suffix {
                    trace!(
                        "Suffix mismatch: expected '{}', got '{}'",
                        schema_suffix, input_suffix
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
                } else {
                    trace!("Suffix matched successfully");
                }
            } else {
                trace!("No suffix to validate");
            }

            result.sync_cursor_pos(&input_cursor, &schema_cursor);
        }
        Err(error) => {
            result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error,
                schema_index: schema_cursor.descendant_index(),
            }));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::{
        node_walker::validators::{
            textual::{validate_matcher_vs_text, validate_textual_container_vs_textual_container},
            textual_containers::validate_text_vs_text,
        },
        ts_utils::parse_markdown,
        validator_state::NodePosPair,
    };

    #[test]
    fn test_validate_text_vs_text_on_text() {
        let schema_str = "test";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // paragraph -> text
        schema_cursor.goto_first_child(); // paragraph -> text

        let result = validate_textual_container_vs_textual_container(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false,
        );

        assert!(result.errors.is_empty());
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
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
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
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_text_vs_text_header_content() {
        let schema_str = "# Test Wolf";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "# Test Wolf";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        // (document[0])
        // └─ (atx_heading[1])
        //    ├─ (atx_h1_marker[2])
        //    └─ (heading_content[3])
        //       └─ (text[4])

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();
        assert_eq!(input_cursor.node().kind(), "heading_content");
        assert_eq!(schema_cursor.node().kind(), "heading_content");

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_text_vs_text_header_content_and_matcher() {
        let schema_str = "# Test `name:/[a-zA-Z]+/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "# Test Wolf";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        // (document[0])
        // └─ (atx_heading[1])
        //    ├─ (atx_h1_marker[2])
        //    └─ (heading_content[3])
        //       └─ (text[4])

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();
        assert_eq!(input_cursor.node().kind(), "heading_content");
        assert_eq!(schema_cursor.node().kind(), "heading_content");

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 5));
        assert_eq!(value, json!({"name": "Wolf"}));
    }

    #[test]
    fn test_validate_text_vs_text_with_incomplete_matcher() {
        let schema_str = "prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "prefix `test:/te";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // we are allowed to have a broken matcher if it is the last com
        );

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_not_long_enough() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "prefix";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        // When EOF is not set, we're waiting for more input
        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false);

        let errors = result.errors.clone();
        let value = result.value.clone();

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
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        // When EOF is not set, we're waiting for more input
        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false);

        let errors = result.errors.clone();
        let value = result.value.clone();

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
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        // When EOF is not set, we're waiting for more input
        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false);

        let errors = result.errors.clone();
        let value = result.value.clone();

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
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        // When EOF is not set and input is empty, we're waiting for more input
        // When EOF is not set, we're waiting for more input
        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false);

        // When waiting for more input without EOF, we shouldn't report errors yet
        assert!(
            result.errors.is_empty(),
            "Should not have errors when waiting for more input with empty input: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }
}
