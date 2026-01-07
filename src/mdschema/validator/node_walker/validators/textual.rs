use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::node_walker::validators::ValidatorImpl;
use crate::mdschema::validator::node_walker::validators::matchers::MatcherVsTextValidator;
use crate::mdschema::validator::ts_utils::{
    both_are_textual_nodes, get_next_node, is_code_node, is_text_node,
};
use crate::mdschema::validator::{
    node_walker::{ValidationResult, validators::Validator},
    ts_utils::waiting_at_end,
    utils::{compare_node_kinds, compare_text_contents},
};

/// Validate two textual elements.
///
/// # Algorithm
///
/// 1. Check if the schema node is at a `code_span`, or the current node is a
///    text node and the next node is a `code_span`. If so, delegate to
///    `MatcherVsTextValidator::validate`.
/// 2. Otherwise, check that the node kind and text contents are the same.
pub(super) struct TextualVsTextualValidator;

impl ValidatorImpl for TextualVsTextualValidator {
    #[track_caller]
    fn validate_impl(
        input_cursor: &TreeCursor,
        schema_cursor: &TreeCursor,
        schema_str: &str,
        input_str: &str,
        got_eof: bool,
    ) -> ValidationResult {
        validate_textual_vs_textual_impl(
            input_cursor,
            schema_cursor,
            schema_str,
            input_str,
            got_eof,
        )
    }
}

#[track_caller]
fn validate_textual_vs_textual_impl(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    // If the schema is pointed at a code node, or a text node followed by a
    // code node, validate it using `MatcherVsTextValidator::validate`

    let current_node_is_code_node = is_code_node(&schema_cursor.node());
    let current_node_is_text_node_and_next_node_code_node = {
        get_next_node(&schema_cursor)
            .map(|n| is_text_node(&schema_cursor.node()) && is_code_node(&n))
            .unwrap_or(false)
    };

    if current_node_is_code_node || current_node_is_text_node_and_next_node_code_node {
        return MatcherVsTextValidator::validate(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
        );
    }

    validate_textual_vs_textual_direct(input_cursor, schema_cursor, schema_str, input_str, got_eof)
}

/// Validate two textual elements directly without checking for matchers.
///
/// This performs the actual node kind and text content comparison without
/// delegating to matcher validation.
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
#[track_caller]
pub(super) fn validate_textual_vs_textual_direct(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    if !both_are_textual_nodes(&schema_cursor.node(), &input_cursor.node()) {
        crate::invariant_violation!(
            result,
            input_cursor,
            schema_cursor,
            format!(
                "expected textual nodes, got schema kind: {:?}, input kind: {:?}",
                schema_cursor.node().kind(),
                input_cursor.node().kind()
            )
        );
        return result;
    }

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
