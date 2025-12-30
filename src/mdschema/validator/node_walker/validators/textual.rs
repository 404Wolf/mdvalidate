use log::trace;
use serde_json::json;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::matcher::matcher::{
    Matcher, MatcherError, get_everything_after_special_chars,
};
use crate::mdschema::validator::node_walker::validators::textual_containers::validate_text_vs_text;
use crate::mdschema::validator::ts_utils::{
    both_are_textual_nodes, get_next_node, get_node_and_next_node, is_code_node, is_last_node,
    is_text_node,
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

    // If the schema is pointed at a code node, attempt to validate it using `validate_text_vs_matcher`
    if is_code_node(&schema_cursor.node()) {
        let matcher_vs_text_result = validate_matcher_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        result.join_other_result(&matcher_vs_text_result);
    }

    debug_assert!(both_are_textual_nodes(
        &schema_cursor.node(),
        &input_cursor.node(),
    ));

    if let Some(error) = compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str) {
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

/// Validate a sequence of nodes that includes a matcher node against a text
/// node. This is used for when we have 1-3 nodes, where there may be a center
/// node that is a code node that is a matcher.
///
/// Called by `validate_text_vs_text` when a matcher group is detected in the schema.
/// A matcher group consists of text-code-text nodes where the code contains a pattern.
///
/// This also supports literal matchers, like
///
/// ```md
/// `test`!
/// ```
///
/// Which match input like
///
/// ```md
/// `test`
/// ```
///
/// So, when the matcher has extras, assume that the subsequent text node also will get validated.
pub fn validate_matcher_vs_text<'a>(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

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

    // Descendant index of the input node, specifically the paragraph (not the interior text)
    let input_node_descendant_index = input_cursor.descendant_index();
    let input_cursor_at_prefix = input_cursor.clone();
    input_cursor.goto_first_child();

    // Preserve the cursor where it's pointing at the prefix node for error reporting
    let mut schema_cursor_at_prefix = schema_cursor.clone();
    schema_cursor_at_prefix.goto_first_child(); // paragraph -> text

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

            let best_prefix_input_we_can_do = &input_str[input_cursor.node().byte_range().start..];

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

    match matcher {
        Ok(matcher) => {
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
            result.sync_cursor_pos(&input_cursor, &schema_cursor);
        }
        Err(error) => match error {
            MatcherError::WasLiteralCode => {
                // move the input to the code node now
                input_cursor.reset_to(&input_cursor_at_prefix);

                let literal_matcher_result = validate_literal_matcher_vs_text(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                );
                result.join_other_result(&literal_matcher_result);
                result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);
                if !result.errors.is_empty() {
                    return result;
                }
            }
            _ => result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error,
                schema_index: schema_cursor.descendant_index(),
            })),
        },
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
            get_everything_after_special_chars(text_node_after_code_node_str_contents).unwrap()
        };

        // Seek forward from the current input byte offset by the length of the suffix
        let input_suffix = &input_str[input_byte_offset..input_byte_offset + schema_suffix.len()];

        // Check if input_suffix is shorter than schema_suffix
        if input_suffix.len() < schema_suffix.len() {
            if got_eof {
                // We've reached EOF, so the input is complete and too short
                trace!(
                    "Suffix mismatch (input too short at EOF): expected '{}', got '{}'",
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
                // We haven't reached EOF yet, so partial match is OK
                // Check if what we have so far matches
                let schema_suffix_partial = &schema_suffix[..input_suffix.len()];
                if schema_suffix_partial != input_suffix {
                    trace!(
                        "Suffix partial mismatch: expected '{}', got '{}'",
                        schema_suffix_partial, input_suffix
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
                    trace!("Suffix partial match successful, waiting for more input");
                }
            }
        } else if schema_suffix != input_suffix {
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
    result.sync_cursor_pos(&schema_cursor, &input_cursor);

    result
}

/// Validate a literal matcher against an input string.
///
/// Requires a cursor pointing at a code_span node for the schema, that maybe
/// has text following it, and a cursor pointing to the equivalent text node to
/// validate in the input.
///
/// # Algorithm
///
/// 1. Recurse down into the actual code node, and validate the inside
/// 2. Validate the remaining portion of the suffix node, which contains the "!".
fn validate_literal_matcher_vs_text(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    debug_assert!(is_code_node(&input_cursor.node())); // `test` | `test` more text here
    debug_assert!(is_code_node(&schema_cursor.node())); // `test`! | `text`! more text here

    // Walk into the code node and do regular textual validation.
    let interior_validation_result = {
        let mut input_cursor = input_cursor.clone();
        let mut schema_cursor = schema_cursor.clone();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        debug_assert!(is_text_node(&input_cursor.node()));
        debug_assert!(is_text_node(&schema_cursor.node()));

        validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        )
    };

    result.join_other_result(&interior_validation_result);
    if !result.errors.is_empty() {
        return result;
    }

    // The schema cursor definitely has a text node after the code node, which
    // at minimum contains "!" (which indicates that it is a literal matcher in
    // the first place)
    if !schema_cursor.goto_next_sibling() && is_text_node(&schema_cursor.node()) {
        result.add_error(ValidationError::InternalInvariantViolated(
            "validate_literal_matcher_vs_text called with a matcher that is not literal. \
             A text node does not follow the schema."
                .into(),
        ));
        return result;
    }

    let schema_node_str = schema_cursor
        .node()
        .utf8_text(schema_str.as_bytes())
        .unwrap();

    let schema_node_str_has_more_than_extras = schema_node_str.len() > 1;

    // Now see if there is more text than just the "!" in the schema text node.
    let Some(schema_text_after_extras) = get_everything_after_special_chars(schema_node_str) else {
        result.add_error(ValidationError::InternalInvariantViolated(
            "we should have had extras in the matcher string".into(),
        ));
        return result;
    };

    if !input_cursor.goto_next_sibling() && schema_node_str_has_more_than_extras {
        result.add_error(ValidationError::InternalInvariantViolated(
            "at this point we should already have counted the number of nodes, \
             factoring in literal matchers."
                .into(),
        ))
    }

    if !is_text_node(&input_cursor.node()) {
        schema_cursor.goto_next_sibling();
        result.sync_cursor_pos(&schema_cursor, &input_cursor);
        return result;
    }

    let input_text_after_code = input_cursor.node().utf8_text(input_str.as_bytes()).unwrap();

    // Partial match is OK if got_eof is false
    if input_text_after_code.len() < schema_text_after_extras.len() {
        if got_eof {
            let input_text_after_code_so_far =
                &input_text_after_code[..schema_text_after_extras.len()];

            // Do the partial comparison
            if input_text_after_code_so_far != schema_text_after_extras {
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_cursor.descendant_index(),
                        expected: schema_text_after_extras.into(),
                        actual: input_text_after_code_so_far.into(),
                        kind: NodeContentMismatchKind::Literal,
                    },
                ));
            } else {
                // Move on!
            }
        } else {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_text_after_extras.into(),
                    actual: input_text_after_code.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
        }
    } else if input_text_after_code.len() < schema_text_after_extras.len() {
        result.add_error(ValidationError::SchemaViolation(
            SchemaViolationError::NodeContentMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_text_after_extras.into(),
                actual: input_text_after_code.into(),
                kind: NodeContentMismatchKind::Literal,
            },
        ));
    } else {
        // Compare the whole thing
        if input_text_after_code != schema_text_after_extras {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_text_after_extras.into(),
                    actual: input_text_after_code.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
        }
    }

    // Walk to the next node, then return
    schema_cursor.goto_next_sibling();
    input_cursor.goto_next_sibling();
    result.sync_cursor_pos(&schema_cursor, &input_cursor);

    result
}

#[cfg(test)]
mod tests {
    use std::vec;

    use serde_json::json;

    use crate::mdschema::validator::{
        errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
        node_walker::validators::{
            textual::{
                validate_literal_matcher_vs_text, validate_matcher_vs_text,
                validate_textual_container_vs_textual_container,
            },
            textual_containers::validate_text_vs_text,
        },
        ts_utils::parse_markdown,
        validator_state::NodePosPair,
    };

    #[test]
    fn test_validate_text_vs_text_on_text() {
        let schema_str = "test";
        // (document[0]0..5)
        // └─ (paragraph[1]0..4)
        //    └─ (text[2]0..4)

        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "test";
        // (document[0]0..5)
        // └─ (paragraph[1]0..4)
        //    └─ (text[2]0..4)

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

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(2, 2));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix() {
        let schema_str = "prefix `test:/test/`";
        // (document[0]0..21)
        // └─ (paragraph[1]0..20)
        //    ├─ (text[2]0..7)
        //    └─ (code_span[3]7..20)
        //       └─ (text[4]8..19)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test";
        // (document[0]0..12)
        // └─ (paragraph[1]0..11)
        //    └─ (text[2]0..11)

        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({"test": "test"}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 2));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix_goes_farther_if_it_can() {
        let schema_str = "prefix `test:/test/` *test*";
        // (document[0]0..28)
        // └─ (paragraph[1]0..27)
        //    ├─ (text[2]0..7)
        //    ├─ (code_span[3]7..20)
        //    │  └─ (text[4]8..19)
        //    ├─ (text[5]20..21)
        //    └─ (emphasis[6]21..27)
        //       └─ (text[7]22..26)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test *test*";
        // (document[0]0..19)
        // └─ (paragraph[1]0..18)
        //    ├─ (text[2]0..12)
        //    └─ (emphasis[3]12..18)
        //       └─ (text[4]13..17)

        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({"test": "test"}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(6, 3)); // emphasis for both
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_and_suffix() {
        let schema_str = "prefix `test:/test/` suffix";
        // (document[0]0..28)
        // └─ (paragraph[1]0..27)
        //    ├─ (text[2]0..7)
        //    ├─ (code_span[3]7..20)
        //    │  └─ (text[4]8..19)
        //    └─ (text[5]20..27)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test suffix";
        // (document[0]0..19)
        // └─ (paragraph[1]0..18)
        //    └─ (text[2]0..18)

        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({"test": "test"}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(5, 2))
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_and_suffix_goes_farther_if_can() {
        let schema_str = "prefix `test:/test/` suffix *test* _*test*_";
        // (document[0]0..44)
        // └─ (paragraph[1]0..43)
        //    ├─ (text[2]0..7)
        //    ├─ (code_span[3]7..20)
        //    │  └─ (text[4]8..19)
        //    ├─ (text[5]20..28)
        //    ├─ (emphasis[6]28..34)
        //    │  └─ (text[7]29..33)
        //    ├─ (text[8]34..35)
        //    └─ (emphasis[9]35..43)
        //       └─ (emphasis[10]36..42)
        //          └─ (text[11]37..41)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test suffix *test* _*test*_";
        // (document[0]0..35)
        // └─ (paragraph[1]0..34)
        //    ├─ (text[2]0..19)
        //    ├─ (emphasis[3]19..25)
        //    │  └─ (text[4]20..24)
        //    ├─ (text[5]25..26)
        //    └─ (emphasis[6]26..34)
        //       └─ (emphasis[7]27..33)
        //          └─ (text[8]28..32)

        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({"test": "test"})); // capture what we could so far
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(9, 3)) // emphasis for both
    }
    #[test]
    fn test_validate_text_vs_text_header_content() {
        let schema_str = "# Test Wolf";
        // (document[0]0..12)
        // └─ (atx_heading[1]0..11)
        //    ├─ (atx_h1_marker[2]0..1)
        //    └─ (heading_content[3]1..11)
        //       └─ (text[4]1..11)
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "# Test Wolf";
        // (document[0]0..12)
        // └─ (atx_heading[1]0..11)
        //    ├─ (atx_h1_marker[2]0..1)
        //    └─ (heading_content[3]1..11)
        //       └─ (text[4]1..11)
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

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
        // (document[0]0..26)
        // └─ (atx_heading[1]0..25)
        //    ├─ (atx_h1_marker[2]0..1)
        //    └─ (heading_content[3]1..25)
        //       ├─ (text[4]1..7)
        //       └─ (code_span[5]7..25)
        //          └─ (text[6]8..24)

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
        assert_eq!(value, json!({"name": "Wolf"}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 5));
    }

    #[test]
    fn test_validate_text_vs_text_with_incomplete_matcher() {
        let schema_str = "prefix `test:/test/`";
        // (document[0]0..21)
        // └─ (paragraph[1]0..20)
        //    ├─ (text[2]0..7)
        //    └─ (code_span[3]7..20)
        //       └─ (text[4]8..19)

        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "prefix `test:/te";
        // (document[0]0..17)
        // └─ (paragraph[1]0..16)
        //    └─ (text[2]0..16)

        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> text

        input_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // paragraph -> text

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // we are allowed to have a broken matcher if it is the last com
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(2, 2)) // no movement yet
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_not_long_enough() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        // (document[0]0..47)
        // └─ (paragraph[1]0..46)
        //    ├─ (text[2]0..33)
        //    └─ (code_span[3]33..46)
        //       └─ (text[4]34..45)
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "prefix";
        // (document[0]0..7)
        // └─ (paragraph[1]0..6)
        //    └─ (text[2]0..6)

        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> text
        input_cursor.goto_first_child(); //  paragraph -> text

        // When EOF is not set, we're waiting for more input
        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false);

        // When waiting for more input without EOF, we shouldn't report errors yet
        // (The validation is incomplete)
        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(2, 2)); // bad so no movement
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_prefix_good_so_far() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        // (document[0]0..47)
        // └─ (paragraph[1]0..46)
        //    ├─ (text[2]0..33)
        //    └─ (code_span[3]33..46)
        //       └─ (text[4]34..45)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix that is lo";
        // (document[0]0..18)
        // └─ (paragraph[1]0..17)
        //    └─ (text[2]0..17)

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

        // When waiting for more input without EOF and prefix matches so far, we shouldn't report errors yet
        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(2, 2)); // bad so no movement
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_suffix_good_so_far() {
        let schema_str = "prefix `test:/test/` suffix that is longer";
        // (document[0]0..44)
        // └─ (paragraph[1]0..43)
        //    ├─ (text[2]0..7)
        //    ├─ (code_span[3]7..20)
        //    │  └─ (text[4]8..19)
        //    └─ (text[5]20..43)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test suffix that";
        // (document[0]0..24)
        // └─ (paragraph[1]0..23)
        //    └─ (text[2]0..23)

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

        // When waiting for more input without EOF and suffix matches so far, we shouldn't report errors yet
        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({"test": "test"}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(5, 2));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_but_bad_prefix() {
        let schema_str = "good prefix `test:/test/`";
        // (document[0]0..26)
        // └─ (paragraph[1]0..25)
        //    ├─ (text[2]0..12)
        //    └─ (code_span[3]12..25)
        //       └─ (text[4]13..24)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "bad p";
        // (document[0]0..26)
        // └─ (paragraph[1]0..25)
        //    └─ (text[2]0..4)
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

        // Even though we're waiting for more input, if the prefix doesn't match what we have,
        // we should report an error
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                kind: NodeContentMismatchKind::Prefix,
                actual,
                expected,
                input_index,
                schema_index,
            }) => {
                assert_eq!(actual, "bad p");
                assert_eq!(expected, "good prefix ");
                assert_eq!(*input_index, 2);
                assert_eq!(*schema_index, 2);
            }
            _ => panic!(
                "Expected a prefix mismatch error, got: {:?}",
                result.errors[0]
            ),
        }

        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(2, 2)); // didn't move forward
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_empty() {
        let schema_str = "prefix `test:/test/`";
        // (document[0]0..21)
        // └─ (paragraph[1]0..20)
        //    ├─ (text[2]0..7)
        //    └─ (code_span[3]7..20)
        //       └─ (text[4]8..19)
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
        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(2, 0)); // didn't move forward
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher() {
        let schema_str = "`test`! foo";
        let schema_tree = parse_markdown(schema_str).unwrap();
        // (document[0]0..12)
        // └─ (paragraph[1]0..11)
        //    ├─ (code_span[2]0..6) ^
        //    │  └─ (text[3]1..5)
        //    └─ (text[4]6..11)

        let input_str = "`test` foo";
        let input_tree = parse_markdown(input_str).unwrap();
        // (document[0]0..11)
        // └─ (paragraph[1]0..10)
        //    ├─ (code_span[2]0..6) ^
        //    │  └─ (text[3]1..5)
        //    └─ (text[4]6..10)

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> code_span
        input_cursor.goto_first_child(); //  paragraph -> code_span

        let result = validate_literal_matcher_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true,
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher_partial_suffix_match() {
        let schema_str = "`test`! foo";
        let schema_tree = parse_markdown(schema_str).unwrap();
        // (document[0]0..12)
        // └─ (paragraph[1]0..11)
        //    ├─ (code_span[2]0..6)
        //    │  └─ (text[3]1..5) ^
        //    └─ (text[4]6..11)

        let input_str = "`test` f";
        let input_tree = parse_markdown(input_str).unwrap();
        // (document[0]0..11)
        // └─ (paragraph[1]0..10)
        //    ├─ (code_span[2]0..6)
        //    │  └─ (text[3]1..5) ^
        //    └─ (text[4]6..10)

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> code_span
        input_cursor.goto_first_child(); //  paragraph -> code_span

        let result = validate_literal_matcher_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // got_eof = false
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(3, 3));

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false); // got_eof = false

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(3, 3));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher_goes_farther_if_it_can() {
        let schema_str = "`test`! foo *test*";
        // (document[0]0..19)
        // └─ (paragraph[1]0..18)
        //    ├─ (code_span[2]0..6)
        //    │  └─ (text[3]1..5)
        //    ├─ (text[4]6..12)
        //    └─ (emphasis[5]12..18)
        //       └─ (text[6]13..17)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "`test` foo *testing*";
        // (document[0]0..21)
        // └─ (paragraph[1]0..20)
        //    ├─ (code_span[2]0..6)
        //    │  └─ (text[3]1..5)
        //    ├─ (text[4]6..11)
        //    └─ (emphasis[5]11..20)
        //       └─ (text[6]12..19)

        let input_tree = parse_markdown(input_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> code_span
        input_cursor.goto_first_child(); //  paragraph -> code_span

        let result = validate_literal_matcher_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true,
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(5, 5));

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(5, 5));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher_instant_non_text_in_input() {
        let schema_str = "`test`!*test*";
        // (document[0]0..19)
        // └─ (paragraph[1]0..18)
        //    ├─ (code_span[2]0..6)
        //    │  └─ (text[3]1..5)
        //    ├─ (text[4]6..12)
        //    └─ (emphasis[5]12..18)
        //       └─ (text[6]13..17)

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "`test`*testing*";
        // (document[0]0..16)
        // └─ (paragraph[1]0..15)
        //    ├─ (code_span[2]0..6)
        //    │  └─ (text[3]1..5)
        //    └─ (emphasis[4]6..15)
        //       └─ (text[5]7..14)

        let input_tree = parse_markdown(input_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> code_span
        input_cursor.goto_first_child(); //  paragraph -> code_span

        let result = validate_literal_matcher_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true,
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(5, 4));

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(5, 4));
    }
}
