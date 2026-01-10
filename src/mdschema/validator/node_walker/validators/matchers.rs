#![allow(dead_code)]
//! Matcher validators for schema text.
//!
//! Types:
//! - `MatcherVsTextValidator`: handles pattern matching and capture logic used
//!   when schema nodes embed matcher syntax inside textual content.
//! - `TextualVsMatcherValidator`: validates textual nodes when matchers appear
//!   immediately after schema text and need to cooperate with surrounding
//!   literals.
//! - `LiteralMatcherVsTextualValidator`: resolves matcher usage when literal
//!   matchers span multiple textual nodes, computing matches across adjacent
//!   literal fragments.
use log::trace;
use serde_json::json;
use tree_sitter::TreeCursor;

use crate::invariant_violation;
use crate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaError, SchemaViolationError, ValidationError,
};
use crate::mdschema::validator::matcher::matcher::{Matcher, MatcherError};
use crate::mdschema::validator::matcher::matcher_extras::get_after_extras;
use crate::mdschema::validator::node_pos_pair::NodePosPair;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::helpers::compare_text_contents::compare_text_contents;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
use crate::mdschema::validator::ts_types::*;
use crate::mdschema::validator::ts_utils::{
    get_next_node, get_node_n_nodes_ahead, get_node_text, waiting_at_end,
};
use crate::mdschema::validator::validator_walker::ValidatorWalker;

use super::textual::validate_textual_vs_textual_direct;

#[derive(Default)]
pub(super) struct MatcherVsTextValidator;

impl ValidatorImpl for MatcherVsTextValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut result =
            ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());

        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        let schema_cursor_is_code_node = is_inline_code_node(&schema_cursor.node());
        let input_node = input_cursor.node();
        let schema_prefix_node = if schema_cursor_is_code_node {
            let mut prev_cursor = schema_cursor.clone();
            if prev_cursor.goto_previous_sibling() && is_text_node(&prev_cursor.node()) {
                Some(prev_cursor.node())
            } else {
                None
            }
        } else if is_text_node(&schema_cursor.node()) {
            Some(schema_cursor.node())
        } else {
            unreachable!(
                "only should be called with `code_span` or text but got {:?}",
                schema_cursor.node()
            )
        };

        let schema_suffix_node = {
            // If there is a prefix and we're at the prefix, this comes two nodes later.
            if schema_prefix_node.is_some() && !schema_cursor_is_code_node {
                get_node_n_nodes_ahead(&schema_cursor, 2)
            } else {
                get_next_node(&schema_cursor)
            }
        };

        let matcher = {
            // Make sure we create the matcher when we are pointing at a `code_span`
            let mut schema_cursor = schema_cursor.clone();
            if schema_prefix_node.is_some() && !schema_cursor_is_code_node {
                schema_cursor.goto_next_sibling();
            }
            Matcher::try_from_schema_cursor(&schema_cursor, walker.schema_str())
        };

        // How far along we've validated the input. We'll update this as we go
        let mut input_byte_offset = input_cursor.node().byte_range().start;

        // Descendant index of the input node, specifically the paragraph (not the interior text)
        let input_cursor_descendant_index = input_cursor.descendant_index();
        let input_cursor_at_prefix = input_cursor.clone();
        input_cursor.goto_first_child();

        // Preserve the cursor where it's pointing at the prefix node for error reporting
        let mut schema_cursor_at_prefix = schema_cursor.clone();
        if schema_cursor_is_code_node {
            let mut prev_cursor = schema_cursor.clone();
            if prev_cursor.goto_previous_sibling() && is_text_node(&prev_cursor.node()) {
                schema_cursor_at_prefix = prev_cursor;
            }
        }
        schema_cursor_at_prefix.goto_first_child();

        match at_text_and_next_at_literal_matcher(&schema_cursor, walker.schema_str()) {
            Ok(Some(true)) => {
                let prefix_result = validate_textual_vs_textual_direct(
                    &input_cursor,
                    &schema_cursor,
                    walker.schema_str(),
                    walker.input_str(),
                    got_eof,
                );
                result.join_other_result(&prefix_result);
            }
            Err(error) => {
                result.add_error(error);
                return result;
            }
            _ => {
                // Only do prefix verification if there is a prefix
                if let Some(schema_prefix_node) = schema_prefix_node {
                    trace!("Validating prefix before matcher");

                    let schema_prefix_str = &walker.schema_str()[schema_prefix_node.byte_range()];

                    // Calculate how much input we have available from the current offset
                    let input_prefix_len = walker.input_str().len() - input_byte_offset;

                    // Check that the input extends enough that we can cover the full prefix.
                    if input_prefix_len >= schema_prefix_str.len() {
                        // We have enough input to compare the full prefix
                        let input_prefix_str = &walker.input_str()
                            [input_byte_offset..input_byte_offset + schema_prefix_str.len()];

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
                            result.sync_cursor_pos(&schema_cursor, &input_cursor);

                            return result;
                        }

                        trace!("Prefix matched successfully");
                        input_byte_offset += schema_prefix_node.byte_range().len();
                    } else if got_eof {
                        // We've reached EOF, so the input is complete and too short
                        let input_prefix_str = &walker.input_str()[input_byte_offset..];

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
                        let input_prefix_str = &walker.input_str()[input_byte_offset..];
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
            }
        }

        // Don't validate after the prefix if there isn't enough content
        if input_byte_offset >= input_node.byte_range().end {
            if got_eof {
                let schema_prefix_str = schema_prefix_node
                    .map(|node| &walker.schema_str()[node.byte_range()])
                    .unwrap_or("");

                let best_prefix_input_we_can_do =
                    &walker.input_str()[input_cursor.node().byte_range().start..];

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
            walker.input_str()[input_byte_offset..input_cursor.node().byte_range().end].to_string();

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
                        //
                        // If we're at the end though, don't add it just yet!
                        if !waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                            if let Some(id) = matcher.id() {
                                trace!("Storing match for id '{}': '{}'", id, matched_str);
                                result.set_match(id, json!(matched_str));
                            } else {
                                trace!("Matcher has no id, not storing match");
                            }
                        }

                        // Walk so that we are ON the `code_span`
                        schema_cursor.goto_next_sibling();

                        // Walk down into the `code_span` and mark its child text as already validated!
                        {
                            let mut schema_cursor = schema_cursor.clone();

                            schema_cursor.goto_first_child();

                            // Only dig in if we won't need to rematch again
                            if !waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                                result.keep_farther_pos(&NodePosPair::from_cursors(
                                    &schema_cursor,
                                    &input_cursor,
                                ));
                            }
                        }
                    }
                    None => {
                        if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
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
                    // Move the schema/input to the code node before validating literal matchers.
                    let mut schema_cursor = schema_cursor.clone();
                    let mut input_cursor = input_cursor_at_prefix.clone();

                    if schema_prefix_node.is_some() {
                        schema_cursor.goto_next_sibling();
                        if !input_cursor.goto_next_sibling() {
                            result.sync_cursor_pos(&schema_cursor, &input_cursor);
                            return result;
                        }
                    }

                    // Delegate to the literal matcher validator
                    return LiteralMatcherVsTextualValidator::default()
                        .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
                }
                _ => result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                    error,
                    schema_index: schema_cursor.descendant_index(),
                })),
            },
        }

        // Validate suffix if there is one
        if let Some(schema_suffix_node) = schema_suffix_node {
            schema_cursor.goto_next_sibling(); // code_span -> text

            // Return early if it is not text
            if !is_text_node(&schema_cursor.node()) {
                return result;
            }

            // Everything that comes after the matcher
            let schema_suffix = {
                let text_node_after_code_node_str_contents =
                    get_node_text(&schema_suffix_node, walker.schema_str());
                // All text after the matcher node and maybe the text node right after it ("extras")
                get_after_extras(text_node_after_code_node_str_contents).unwrap()
            };

            // Seek forward from the current input byte offset by the length of the suffix
            let input_suffix_raw =
                &walker.input_str()[input_byte_offset..input_cursor.node().byte_range().end];
            
            // Trim the input suffix if we're in a table cell context, to match how schema_suffix is obtained
            let input_suffix = if is_table_cell_node(&input_cursor.node()) 
                || input_cursor.node().parent().is_some_and(|n| is_table_cell_node(&n)) {
                input_suffix_raw.trim()
            } else {
                input_suffix_raw
            };

            // Calculate the actual length after potential trimming
            let input_suffix_len = input_suffix.len();

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
                result.keep_farther_pos(&NodePosPair::from_cursors(
                    walker.schema_cursor(),
                    walker.input_cursor(),
                ));
            }
        }

        result
    }
}

#[derive(Default)]
pub(super) struct TextualVsMatcherValidator;

impl ValidatorImpl for TextualVsMatcherValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut result =
            ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());

        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        #[cfg(feature = "invariant_violations")]
        if !is_inline_code_node(&schema_cursor.node()) || !is_inline_code_node(&input_cursor.node())
        {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "literal matcher validation expects code_span nodes"
            );
        }

        // Walk into the code node and do regular textual validation.
        {
            let mut schema_cursor = schema_cursor.clone();
            let mut input_cursor = input_cursor.clone();
            input_cursor.goto_first_child();
            schema_cursor.goto_first_child();

            #[cfg(feature = "invariant_violations")]
            if !is_text_node(&schema_cursor.node()) || !is_text_node(&input_cursor.node()) {
                invariant_violation!(
                    result,
                    &schema_cursor,
                    &input_cursor,
                    "literal matcher validation expects text children"
                );
            }

            let text_result = compare_text_contents(
                walker.schema_str(),
                walker.input_str(),
                &schema_cursor,
                &input_cursor,
                false,
                false,
            );
            result.join_other_result(&text_result);
            if text_result.has_errors() {
                return result;
            }
        }

        // The schema cursor definitely has a text node after the code node, which
        // at minimum contains "!" (which indicates that it is a literal matcher in
        // the first place).
        #[cfg(feature = "invariant_violations")]
        if !schema_cursor.goto_next_sibling() && is_text_node(&schema_cursor.node()) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "validate_literal_matcher_vs_text called with a matcher that is not literal. \
                     A text node does not follow the schema."
            );
        }

        let schema_node_str = get_node_text(&schema_cursor.node(), walker.schema_str());

        let schema_node_str_has_more_than_extras = schema_node_str.len() > 1;

        // Now see if there is more text than just the "!" in the schema text node.
        let schema_text_after_extras = match get_after_extras(schema_node_str) {
            Some(text) => text,
            None => {
                #[cfg(feature = "invariant_violations")]
                {
                    invariant_violation!(
                        result,
                        &schema_cursor,
                        &input_cursor,
                        "we should have had extras in the matcher string"
                    );
                }
            }
        };

        #[cfg(feature = "invariant_violations")]
        if !input_cursor.goto_next_sibling() && schema_node_str_has_more_than_extras {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "at this point we should already have counted the number of nodes, \
                     factoring in literal matchers."
            );
        }

        if !is_text_node(&input_cursor.node()) {
            schema_cursor.goto_next_sibling();
            result.sync_cursor_pos(&schema_cursor, &input_cursor);
            return result;
        }

        let input_text_after_code = get_node_text(&input_cursor.node(), walker.input_str());

        // Partial match is OK if got_eof is false.
        if input_text_after_code.len() < schema_text_after_extras.len() {
            if !got_eof {
                let schema_text_after_extras_to_compare_against_so_far =
                    &schema_text_after_extras[..input_text_after_code.len()];

                // Do the partial comparison.
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
                    // will need to redo this part later until we've got EOF.
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
            // Compare the whole thing.
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
}

#[derive(Default)]
pub(super) struct LiteralMatcherVsTextualValidator;

impl ValidatorImpl for LiteralMatcherVsTextualValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let schema_cursor: &TreeCursor = walker.schema_cursor();
        let input_cursor: &TreeCursor = walker.input_cursor();
        let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

        let mut schema_cursor = schema_cursor.clone();
        let mut input_cursor = input_cursor.clone();

        #[cfg(feature = "invariant_violations")]
        if !is_inline_code_node(&schema_cursor.node()) || !is_inline_code_node(&input_cursor.node())
        {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "literal matcher validation expects code_span nodes"
            );
        }

        // Walk into the code node and do regular textual validation.
        {
            let mut schema_cursor = schema_cursor.clone();
            let mut input_cursor = input_cursor.clone();
            input_cursor.goto_first_child();
            schema_cursor.goto_first_child();

            #[cfg(feature = "invariant_violations")]
            if !is_text_node(&schema_cursor.node()) || !is_text_node(&input_cursor.node()) {
                invariant_violation!(
                    result,
                    &schema_cursor,
                    &input_cursor,
                    "literal matcher validation expects text children"
                );
            }

            let text_result = compare_text_contents(
                walker.schema_str(),
                walker.input_str(),
                &schema_cursor,
                &input_cursor,
                false,
                false,
            );
            result.join_other_result(&text_result);
            if text_result.has_errors() {
                return result;
            }
        }

        // The schema cursor definitely has a text node after the code node, which
        // at minimum contains "!" (which indicates that it is a literal matcher in
        // the first place).
        #[cfg(feature = "invariant_violations")]
        if !schema_cursor.goto_next_sibling() && is_text_node(&schema_cursor.node()) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "validate_literal_matcher_vs_text called with a matcher that is not literal. \
             A text node does not follow the schema."
            );
        }

        let schema_node_str = get_node_text(&schema_cursor.node(), walker.schema_str());

        let schema_node_str_has_more_than_extras = schema_node_str.len() > 1;

        // Now see if there is more text than just the "!" in the schema text node.
        let schema_text_after_extras = match get_after_extras(schema_node_str) {
            Some(text) => text,
            None => {
                #[cfg(feature = "invariant_violations")]
                {
                    invariant_violation!(
                        result,
                        &schema_cursor,
                        &input_cursor,
                        "we should have had extras in the matcher string"
                    );
                }
            }
        };

        #[cfg(feature = "invariant_violations")]
        if !input_cursor.goto_next_sibling() && schema_node_str_has_more_than_extras {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "at this point we should already have counted the number of nodes, \
             factoring in literal matchers."
            );
        }

        if !is_text_node(&input_cursor.node()) {
            schema_cursor.goto_next_sibling();
            result.sync_cursor_pos(&schema_cursor, &input_cursor);
            return result;
        }

        let input_text_after_code = input_cursor
            .node()
            .utf8_text(walker.input_str().as_bytes())
            .unwrap();

        // Partial match is OK if got_eof is false.
        if input_text_after_code.len() < schema_text_after_extras.len() {
            if !got_eof {
                let schema_text_after_extras_to_compare_against_so_far =
                    &schema_text_after_extras[..input_text_after_code.len()];

                // Do the partial comparison.
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
                    // will need to redo this part later until we've got EOF.
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
            // Compare the whole thing.
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
}

fn at_text_and_next_at_literal_matcher(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<Option<bool>, ValidationError> {
    if !is_text_node(&schema_cursor.node()) {
        return Ok(None);
    }

    let mut next_cursor = schema_cursor.clone();
    if !next_cursor.goto_next_sibling() || !is_inline_code_node(&next_cursor.node()) {
        return Ok(None);
    }

    match Matcher::try_from_schema_cursor(&next_cursor, schema_str) {
        Ok(_) => Ok(Some(false)),
        Err(MatcherError::WasLiteralCode) => Ok(Some(true)),
        Err(error) => Err(ValidationError::SchemaError(SchemaError::MatcherError {
            error,
            schema_index: schema_cursor.descendant_index(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::test_utils::ValidatorTester;
    use super::super::textual::TextualVsTextualValidator;
    use super::{LiteralMatcherVsTextualValidator, MatcherVsTextValidator};
    use crate::mdschema::validator::node_walker::validators::Validator;
    use crate::mdschema::validator::{
        errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
        node_pos_pair::NodePosPair,
        ts_types::*,
        ts_utils::parse_markdown,
        validator_walker::ValidatorWalker,
    };

    #[test]
    fn test_validate_matcher_vs_text_partial() {
        let schema_str = r#"`item:/\w+/`"#;
        let input_str = "appl";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
            .goto_first_child_then_unwrap()
            .validate_incomplete();

        // We should NOT go farther for now
        // Schema:                     Input:
        // (document[0]0..12)          (document[0]0..4)
        // └─ (paragraph[1]0..12)      └─ (paragraph[1]0..4)
        //    └─ (code_span[2]0..12)      └─ (text[2]0..4)
        //       └─ (text[3]1..11)
        // shouldn't capture just yet
        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(2, 2));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix_test() {
        let schema_str = "prefix `test:/test/`";
        let input_str = "prefix test";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 2));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix() {
        let schema_str = "prefix `test:/test/`";
        let input_str = "prefix test";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 2));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let walker =
            ValidatorWalker::from_cursors(&schema_cursor, schema_str, &input_cursor, input_str);
        let result = TextualVsTextualValidator::default().validate(&walker, true);

        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix_ends_at_end_of_text() {
        let schema_str = "prefix `test:/test/` *test*";
        let input_str = "prefix test *test*";
        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 2));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_and_suffix() {
        let schema_str = "prefix `test:/test/` suffix";
        let input_str = "prefix test suffix";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_and_suffix_ends_at_end_of_text() {
        let schema_str = "prefix `test:/test/` suffix *test* _*test*_";
        let input_str = "prefix test suffix *test* _*test*_";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }
    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_not_long_enough() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        let input_str = "prefix";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(is_paragraph_node(s) && is_paragraph_node(i)))
            .goto_first_child_then_unwrap()
            .validate_incomplete();

        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_prefix_good_so_far() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        let input_str = "prefix that is lo";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_incomplete();

        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_suffix_good_so_far() {
        let schema_str = "prefix `test:/test/` suffix that is longer";
        let input_str = "prefix test suffix that";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_incomplete();

        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(
            result.errors(),
            &[ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: 5,
                    input_index: 2,
                    expected: " suffix that is longer".into(),
                    actual: " suffix that".into(),
                    kind: NodeContentMismatchKind::Suffix,
                }
            )]
        );

        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_but_bad_prefix() {
        let schema_str = "good prefix `test:/test/`";
        let input_str = "bad p";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_incomplete();

        assert_eq!(result.errors().len(), 1);
        match &result.errors()[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                kind: NodeContentMismatchKind::Prefix,
                actual,
                expected,
                input_index,
                schema_index,
            }) => {
                assert_eq!(actual, "bad p");
                assert_eq!(expected, "good prefix ");
                assert_eq!(input_index, &2);
                assert_eq!(schema_index, &2);
            }
            _ => panic!(
                "Expected a prefix mismatch error, got: {:?}",
                result.errors()[0]
            ),
        }

        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher() {
        let schema_str = "`test`! foo";
        let input_str = "`test` foo";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher_with_prefix() {
        let schema_str = "prefix `test`! foo";
        let input_str = "prefix `test` foo";
        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(5, 5));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher_partial_suffix_match() {
        let schema_str = "`test`! foo";
        let input_str = "`test` f";
        let result =
            ValidatorTester::<LiteralMatcherVsTextualValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .validate_incomplete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(3, 3));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_incomplete() {
        let schema_str = "test `foo:/test/`";
        let input_str = "test te";

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_incomplete();

        // no errors so far
        assert!(result.errors().is_empty());
        // nor do we get any captures so far
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_repeating() {
        let schema_str = "test `test:/test/`{1,} foo";
        let input_str = "test test foo";
        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher_ends_at_end_of_text() {
        let schema_str = "`test`! foo *test*";
        let input_str = "`test` foo *testing*";
        let result =
            ValidatorTester::<LiteralMatcherVsTextualValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_literal_matcher_instant_non_text_in_input() {
        let schema_str = "`test`!*test*";
        let input_str = "`test`*testing*";
        let result =
            ValidatorTester::<LiteralMatcherVsTextualValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
                .goto_first_child_then_unwrap()
                .peek_nodes(|(s, i)| assert!(both_are_inline_code(s, i)))
                .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(5, 4));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));

        let result = ValidatorTester::<MatcherVsTextValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(5, 4));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }
}
