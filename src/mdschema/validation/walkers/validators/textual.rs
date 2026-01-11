//! Textual node validator for node-walker comparisons.
//!
//! Types:
//! - `TextualVsTextualValidator`: compares text and inline code nodes, delegating
//!   to matcher validation when schema content contains matcher syntax.
use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::compare_node_kinds_check;
use crate::invariant_violation;
use crate::mdschema::validation::walkers::helpers::compare_text_contents::compare_text_contents;
use crate::mdschema::validation::walkers::validators::ValidatorImpl;
use crate::mdschema::validation::walkers::validators::matchers::MatcherVsTextValidator;
use crate::mdschema::validation::validator_walker::ValidatorWalker;
use crate::mdschema::validation::{
    walkers::{ValidationResult, validators::Validator},
    ts_types::*,
    ts_utils::{get_next_node, waiting_at_end},
};

/// Validate two textual elements.
///
/// # Algorithm
///
/// 1. Check if the schema node is at a `code_span`, or the current node is a
///    text node and the next node is a `code_span`. If so, delegate to
///    `MatcherVsTextValidator::validate`.
/// 2. Otherwise, check that the node kind and text contents are the same.
#[derive(Default)]
pub(super) struct TextualVsTextualValidator;

impl ValidatorImpl for TextualVsTextualValidator {
    #[track_caller]
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        validate_textual_vs_textual_impl(walker, got_eof)
    }
}

#[track_caller]
fn validate_textual_vs_textual_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
    // If the schema is pointed at a code node, or a text node followed by a
    // code node, validate it using `MatcherVsTextValidator::validate`

    let current_node_is_code_node = is_inline_code_node(&walker.schema_cursor().node());
    let current_node_is_text_node_and_next_node_code_node = {
        get_next_node(walker.schema_cursor())
            .map(|n| is_text_node(&walker.schema_cursor().node()) && is_inline_code_node(&n))
            .unwrap_or(false)
    };

    if current_node_is_code_node || current_node_is_text_node_and_next_node_code_node {
        return MatcherVsTextValidator.validate(walker, got_eof);
    }

    validate_textual_vs_textual_direct(
        walker.schema_cursor(),
        walker.input_cursor(),
        walker.schema_str(),
        walker.input_str(),
        got_eof,
    )
}

/// Validate two textual elements directly without checking for matchers.
///
/// This performs the actual node kind and text content comparison without
/// delegating to matcher validation.
#[instrument(skip(schema_cursor, input_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    s = %schema_cursor.descendant_index(),
    i = %input_cursor.descendant_index(),
), ret)]
#[track_caller]
pub(super) fn validate_textual_vs_textual_direct(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    #[cfg(feature = "invariant_violations")]
    if !both_are_textual_nodes(&schema_cursor.node(), &input_cursor.node()) {
        invariant_violation!(
            result,
            &schema_cursor,
            &input_cursor,
            "expected textual nodes, got schema kind: {:?}, input kind: {:?}",
            schema_cursor.node().kind(),
            input_cursor.node().kind()
        );
    }

    compare_node_kinds_check!(schema_cursor, input_cursor, schema_str, input_str, result);

    let is_partial_match = waiting_at_end(got_eof, input_str, input_cursor);
    let text_result = compare_text_contents(
        schema_str,
        input_str,
        schema_cursor,
        input_cursor,
        is_partial_match,
        false,
    );
    result.join_other_result(&text_result);
    if text_result.has_errors() {
        return result;
    }

    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::test_utils::ValidatorTester;
    use super::TextualVsTextualValidator;
    use crate::mdschema::validation::{node_pos_pair::NodePosPair, ts_types::*};

    #[test]
    fn test_validate_textual_vs_textual_with_literal_matcher() {
        let schema_str = "`code`! test";
        let input_str = "`code` test";

        let result = ValidatorTester::<TextualVsTextualValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_inline_code(s, i)))
            .validate_complete();

        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_textual_vs_textual_with_incomplete_matcher() {
        let schema_str = r#"prefix `test:/test/`

prefix `test:/test/`
"#;
        let input_str = r#"prefix t"#;

        let result = ValidatorTester::<TextualVsTextualValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_text_nodes(s, i)))
            .validate_incomplete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(2, 2));
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({}));
    }
}
