use log::trace;
use serde_json::json;
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::matcher::matcher::{Matcher, MatcherError};
use crate::mdschema::validator::matcher::matcher_extras::get_everything_after_extras;
use crate::mdschema::validator::ts_utils::{
    both_are_textual_nodes, get_next_node, get_node_n_nodes_ahead, is_code_node, is_text_node,
};
use crate::mdschema::validator::validator_state::NodePosPair;
use crate::mdschema::validator::{
    errors::*,
    node_walker::ValidationResult,
    ts_utils::waiting_at_end,
    utils::{compare_node_kinds, compare_text_contents},
};

/// Validate two textual elements.
///
/// # Algorithm
///
/// 1. Check if the schema node is at a `code_span`, or the current node is a
///    text node and the next node is a `code_span`. If so, delegate to
///    `validate_matcher_vs_text`.
/// 2. Otherwise, check that the node kind and text contents are the same.
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
#[track_caller]
pub fn validate_textual_vs_textual(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    // If the schema is pointed at a code node, or a text node followed by a
    // code node, validate it using `validate_matcher_vs_text`

    let current_node_is_code_node = is_code_node(&schema_cursor.node());
    let current_node_is_text_node_and_next_node_code_node = {
        get_next_node(&schema_cursor)
            .map(|n| is_text_node(&schema_cursor.node()) && is_code_node(&n))
            .unwrap_or(false)
    };

    if current_node_is_code_node || current_node_is_text_node_and_next_node_code_node {
        return validate_matcher_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    debug_assert!(
        both_are_textual_nodes(&schema_cursor.node(), &input_cursor.node()),
        "got schema kind: {:?}, input kind: {:?}",
        schema_cursor.node().kind(),
        input_cursor.node().kind()
    );

    if let Some(error) = compare_node_kinds(&schema_cursor, &input_cursor, input_str, schema_str) {
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
///
/// Repeating matchers are **ignored** and are treated as normal matchers.
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

    let input_node = input_cursor.node();

    let schema_prefix_node = {
        if is_code_node(&schema_cursor.node()) {
            None
        } else if is_text_node(&schema_cursor.node()) {
            Some(schema_cursor.node())
        } else {
            unreachable!(
                "only should be called with `code_span` or text but got {:?}",
                schema_cursor.node()
            )
        }
    };

    let schema_suffix_node = {
        // If there is a prefix, this comes two nodes later
        if schema_prefix_node.is_some() {
            get_node_n_nodes_ahead(&schema_cursor, 2)
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
    let input_cursor_descendant_index = input_cursor.descendant_index();
    let input_cursor_at_prefix = input_cursor.clone();
    input_cursor.goto_first_child();

    // Preserve the cursor where it's pointing at the prefix node for error reporting
    let mut schema_cursor_at_prefix = schema_cursor.clone();
    schema_cursor_at_prefix.goto_first_child();

    // Only do prefix verification if there is a prefix
    if let Some(schema_prefix_node) = schema_prefix_node {
        trace!("Validating prefix before matcher");

        let schema_prefix_str = &schema_str[schema_prefix_node.byte_range()];

        // Calculate how much input we have available from the current offset
        let input_prefix_len = input_str.len() - input_byte_offset;

        // Check that the input extends enough that we can cover the full prefix.
        if input_prefix_len >= schema_prefix_str.len() {
            // We have enough input to compare the full prefix
            let input_prefix_str =
                &input_str[input_byte_offset..input_byte_offset + schema_prefix_str.len()];

            // Do the actual prefix comparison
            if schema_prefix_str != input_prefix_str {
                trace!(
                    "Prefix mismatch: expected '{}', got '{}'",
                    schema_prefix_str, input_prefix_str
                );
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor_at_prefix.descendant_index(),
                        input_index: input_cursor_descendant_index,
                        expected: schema_prefix_str.into(),
                        actual: input_prefix_str.into(),
                        kind: NodeContentMismatchKind::Prefix,
                    },
                ));

                // If prefix validation fails don't try to validate further.
                // TODO: In the future we could attempt to validate further anyway!
                result.sync_cursor_pos(&schema_cursor, &input_cursor);

                return result;
            }

            trace!("Prefix matched successfully");
            input_byte_offset += schema_prefix_node.byte_range().len();
        } else if got_eof {
            // We've reached EOF, so the input is complete and too short
            let input_prefix_str = &input_str[input_byte_offset..];

            trace!(
                "Prefix mismatch (input too short at EOF): expected '{}', got '{}'",
                schema_prefix_str, input_prefix_str
            );

            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor_at_prefix.descendant_index(),
                    input_index: input_cursor_descendant_index,
                    expected: schema_prefix_str.into(),
                    actual: input_prefix_str.into(),
                    kind: NodeContentMismatchKind::Prefix,
                },
            ));

            result.sync_cursor_pos(&schema_cursor, &input_cursor);
            return result;
        } else {
            // We haven't reached EOF yet, so partial match is OK
            // Check if what we have so far matches
            let input_prefix_str = &input_str[input_byte_offset..];
            let schema_prefix_partial = &schema_prefix_str[..input_prefix_str.len()];

            trace!("Input prefix not long enough, but waiting at end of input");

            if schema_prefix_partial != input_prefix_str {
                trace!(
                    "Prefix partial mismatch: expected '{}', got '{}'",
                    schema_prefix_partial, input_prefix_str
                );
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor_at_prefix.descendant_index(),
                        input_index: input_cursor_descendant_index,
                        expected: schema_prefix_str.into(),
                        actual: input_prefix_str.into(),
                        kind: NodeContentMismatchKind::Prefix,
                    },
                ));
            }

            result.sync_cursor_pos(&schema_cursor, &input_cursor);

            return result;
        }
    }

    // Don't validate after the prefix if there isn't enough content
    if input_byte_offset >= input_node.byte_range().end {
        if got_eof {
            let schema_prefix_str = schema_prefix_node
                .map(|node| &schema_str[node.byte_range()])
                .unwrap_or("");

            let best_prefix_input_we_can_do = &input_str[input_cursor.node().byte_range().start..];

            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor_at_prefix.descendant_index(),
                    input_index: input_cursor_descendant_index,
                    expected: schema_prefix_str.into(),
                    actual: best_prefix_input_we_can_do.into(),
                    kind: NodeContentMismatchKind::Prefix,
                },
            ));
        }

        result.sync_cursor_pos(&schema_cursor, &input_cursor);

        return result;
    }

    // All input that comes after the expected prefix
    let input_after_prefix =
        input_str[input_byte_offset..input_cursor.node().byte_range().end].to_string();

    dbg!(
        input_cursor.node().to_sexp(),
        schema_cursor.node().to_sexp(),
        input_cursor.node().utf8_text(input_str.as_bytes()).unwrap(),
        schema_cursor
            .node()
            .utf8_text(schema_str.as_bytes())
            .unwrap(),
        &matcher
    );
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

                    // Walk so that we are ON the `code_span`
                    schema_cursor.goto_next_sibling();

                    // Walk down into the `code_span` and mark its child text as already validated!
                    {
                        let mut schema_cursor = schema_cursor.clone();

                        schema_cursor.goto_first_child();
                        result.keep_farther_pos(&NodePosPair::from_cursors(
                            &schema_cursor,
                            &input_cursor,
                        ));
                    }
                }
                None => {
                    // TODO: is this right?
                    if waiting_at_end(got_eof, input_str, &input_cursor) {
                        return result;
                    };

                    trace!(
                        "Matcher did not match input string: pattern={}, input='{}'",
                        matcher.pattern().to_string(),
                        input_after_prefix
                    );

                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: schema_cursor.descendant_index(),
                            input_index: input_cursor_descendant_index,
                            expected: matcher.pattern().to_string(),
                            actual: input_after_prefix.into(),
                            kind: NodeContentMismatchKind::Matcher,
                        },
                    ));

                    return result;
                }
            }
        }
        Err(error) => match error {
            MatcherError::WasLiteralCode => {
                // Move the input to the code node now
                input_cursor.reset_to(&input_cursor_at_prefix);

                // Delegate to the literal matcher validator
                return validate_literal_matcher_vs_textual(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                );
            }
            _ => result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error,
                schema_index: schema_cursor.descendant_index(),
            })),
        },
    }

    // Validate suffix if there is one
    if let Some(schema_suffix_node) = schema_suffix_node {
        // (document[0]0..28)
        // └─ (paragraph[1]0..27)
        //    ├─ (text[2]0..7)
        //    ├─ (code_span[3]7..20) <-- from here...
        //    │  └─ (text[4]8..19)
        //    └─ (text[5]20..21) <-- ...go here!
        schema_cursor.goto_next_sibling(); // code_span -> text

        // Return early if it is not text
        if !is_text_node(&schema_cursor.node()) {
            return result;
        }

        // Everything that comes after the matcher
        let schema_suffix = {
            let text_node_after_code_node_str_contents =
                &schema_str[schema_suffix_node.byte_range()];
            // All text after the matcher node and maybe the text node right after it ("extras")
            get_everything_after_extras(text_node_after_code_node_str_contents).unwrap()
        };

        // Seek forward from the current input byte offset by the length of the suffix
        let input_suffix_len = input_cursor.node().byte_range().end - input_byte_offset;

        // Check if input_suffix is shorter than schema_suffix
        let input_suffix = &input_str[input_byte_offset..input_cursor.node().byte_range().end];

        if input_suffix_len < schema_suffix.len() {
            if got_eof {
                // We've reached EOF, so the input is complete and too short
                trace!(
                    "Suffix mismatch (input too short at EOF): expected '{}', got '{}'",
                    schema_suffix, input_suffix
                );

                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_cursor_descendant_index,
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
                            input_index: input_cursor_descendant_index,
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
                    input_index: input_cursor_descendant_index,
                    expected: schema_suffix.into(),
                    actual: input_suffix.into(),
                    kind: NodeContentMismatchKind::Suffix,
                },
            ));
        } else {
            trace!("Suffix matched successfully");

            // We validated this one! Load the result with the new pos!
            result.keep_farther_pos(&NodePosPair::from_cursors(&schema_cursor, &input_cursor));
        }
    }

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
fn validate_literal_matcher_vs_textual(
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
    {
        let mut input_cursor = input_cursor.clone();
        let mut schema_cursor = schema_cursor.clone();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        debug_assert!(is_text_node(&input_cursor.node()));
        debug_assert!(is_text_node(&schema_cursor.node()));

        if let Some(error) = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            false,
            false,
        ) {
            result.add_error(error);
            return result;
        }
        result.keep_farther_pos(&NodePosPair::from_cursors(&schema_cursor, &input_cursor));
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
    let schema_text_after_extras = match get_everything_after_extras(schema_node_str) {
        Ok(text) => text,
        Err(_) => {
            result.add_error(ValidationError::InternalInvariantViolated(
                "we should have had extras in the matcher string".into(),
            ));
            return result;
        }
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
        if !got_eof {
            let schema_text_after_extras_to_compare_against_so_far =
                &schema_text_after_extras[..input_text_after_code.len()];

            // Do the partial comparison
            if schema_text_after_extras_to_compare_against_so_far != input_text_after_code {
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_cursor.descendant_index(),
                        expected: schema_text_after_extras_to_compare_against_so_far.into(),
                        actual: input_text_after_code.into(),
                        kind: NodeContentMismatchKind::Literal,
                    },
                ));
            } else {
                // Return early for now. We don't want to move on because we
                // will need to redo this part later until we've got EOF
                return result;
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

    result.sync_cursor_pos(&schema_cursor, &input_cursor);

    result
}

#[cfg(test)]
mod tests {
    use std::vec;

    use serde_json::json;

    use crate::mdschema::validator::{
        errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
        node_walker::validators::textual::{
            validate_literal_matcher_vs_textual, validate_matcher_vs_text,
            validate_textual_vs_textual,
        },
        ts_utils::parse_markdown,
        validator_state::NodePosPair,
    };

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

        // They should use the same internal function
        assert_eq!(
            validate_textual_vs_textual(&input_cursor, &schema_cursor, schema_str, input_str, true),
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true)
        )
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix_ends_at_end_of_text() {
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
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(5, 2)); // text before emphasis for both
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
    fn test_validate_matcher_vs_text_with_prefix_and_suffix_ends_at_end_of_text() {
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
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(5, 2))
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
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 2)); // schema doesn't progress since we didn't finish validating the suffix
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

        // let result = validate_literal_matcher_vs_textual(
        //     &input_cursor,
        //     &schema_cursor,
        //     schema_str,
        //     input_str,
        //     true,
        // );

        // assert_eq!(result.errors, vec![]);
        // assert_eq!(result.value, json!({}));
        // assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));

        let result =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));

        let result =
            validate_textual_vs_textual(&input_cursor, &schema_cursor, schema_str, input_str, true);

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

        let result = validate_literal_matcher_vs_textual(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // got_eof = false
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(3, 3));

        // let result =
        //     validate_matcher_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, false); // got_eof = false

        // assert_eq!(result.errors, vec![]);
        // assert_eq!(result.value, json!({}));
        // assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(3, 3));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_repeating() {
        let schema_str = "test `test:/test/`{1,} foo";
        let schema_tree = parse_markdown(schema_str).unwrap();
        // (document[0]0..14)
        // └─ (paragraph[1]0..13)
        //    ├─ (code_span[2]0..9)
        //    │  └─ (text[3]1..8) ^
        //    └─ (text[4]9..13)

        let input_str = "test test foo";
        let input_tree = parse_markdown(input_str).unwrap();
        // (document[0]0..9)
        // └─ (paragraph[1]0..8)
        //    └─ (text[2]0..8)

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> code_span
        input_cursor.goto_first_child(); // document -> text

        let result = validate_textual_vs_textual(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // got_eof = true
        );

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher_ends_at_end_of_text() {
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

        let result = validate_literal_matcher_vs_textual(
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

        let result = validate_literal_matcher_vs_textual(
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
