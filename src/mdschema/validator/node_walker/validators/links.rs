use serde_json::json;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaError, SchemaViolationError, ValidationError,
};
use crate::mdschema::validator::matcher::matcher::MatcherError;
use crate::mdschema::validator::node_pos_pair::NodePosPair;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::helpers::curly_matchers::extract_matcher_from_curly_delineated_text;
use crate::mdschema::validator::node_walker::helpers::compare_text_contents::compare_text_contents;
use crate::mdschema::validator::node_walker::validators::ValidatorImpl;
use crate::mdschema::validator::ts_utils::{
    is_image_node, is_link_destination_node, is_link_node, waiting_at_end,
};
use crate::mdschema::validator::validator_walker::ValidatorWalker;

// Use the macro from node_walker module
use crate::compare_node_kinds_check;

/// Validate two link-like nodes (links or images) against each other.
pub(super) struct LinkVsLinkValidator;

impl ValidatorImpl for LinkVsLinkValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        validate_link_vs_link_impl(walker, got_eof)
    }
}

fn validate_link_vs_link_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());

    let schema_str = walker.schema_str();
    let input_str = walker.input_str();

    let link_schema_cursor = walker.schema_cursor().clone();
    let link_input_cursor = walker.input_cursor().clone();

    let mut input_cursor = walker.input_cursor().clone();
    let mut schema_cursor = walker.schema_cursor().clone();

    compare_node_kinds_check!(schema_cursor, input_cursor, input_str, schema_str, result);

    if let Err(error) = ensure_at_link_start(&mut input_cursor) {
        result.add_error(error);
        return result;
    }

    if let Err(error) = ensure_at_link_start(&mut schema_cursor) {
        result.add_error(error);
        return result;
    }

    #[cfg(feature = "invariant_violations")]
    if !input_cursor.goto_first_child() || !schema_cursor.goto_first_child() {
        crate::invariant_violation!(
            result,
            &input_cursor,
            &schema_cursor,
            "link nodes must have children"
        );
    }

    compare_node_kinds_check!(schema_cursor, input_cursor, input_str, schema_str, result);

    if is_link_destination_node(&schema_cursor.node()) {
        let destination_result = validate_link_destination(
            &schema_cursor,
            &input_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        result.join_other_result(&destination_result);
        if destination_result.has_errors() {
            return result;
        }
    } else if let Some(error) = compare_link_child_text(
        &schema_cursor,
        &input_cursor,
        schema_str,
        input_str,
        got_eof,
    ) {
        result.add_error(error);
        return result;
    }

    if let Some(pos) = link_child_pos(&schema_cursor, &input_cursor) {
        result.keep_farther_pos(&pos);
    }

    #[cfg(feature = "invariant_violations")]
    if !input_cursor.goto_next_sibling() || !schema_cursor.goto_next_sibling() {
        crate::invariant_violation!(
            result,
            &input_cursor,
            &schema_cursor,
            "link nodes must have a destination"
        );
    }

    compare_node_kinds_check!(schema_cursor, input_cursor, input_str, schema_str, result);

    if is_link_destination_node(&schema_cursor.node()) {
        let destination_result = validate_link_destination(
            &schema_cursor,
            &input_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        result.join_other_result(&destination_result);
        if destination_result.has_errors() {
            return result;
        }
    } else if let Some(error) = compare_link_child_text(
        &schema_cursor,
        &input_cursor,
        schema_str,
        input_str,
        got_eof,
    ) {
        result.add_error(error);
        return result;
    }

    if let Some(pos) = link_child_pos(&schema_cursor, &input_cursor) {
        result.keep_farther_pos(&pos);
    }

    result.sync_cursor_pos(&link_schema_cursor, &link_input_cursor);

    result
}

fn ensure_at_link_start(cursor: &mut TreeCursor) -> Result<(), ValidationError> {
    if is_link_node(&cursor.node()) || is_image_node(&cursor.node()) {
        return Ok(());
    }

    #[cfg(feature = "invariant_violations")]
    crate::invariant_violation!(
        cursor,
        cursor,
        "Expected to be at link or image node, but found {}",
        cursor.node().kind()
    );
}

fn compare_link_child_text(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> Option<ValidationError> {
    let mut schema_text_cursor = schema_cursor.clone();
    let mut input_text_cursor = input_cursor.clone();

    #[cfg(feature = "invariant_violations")]
    if !schema_text_cursor.goto_first_child() || !input_text_cursor.goto_first_child() {
        crate::invariant_violation!(
            &input_text_cursor,
            &schema_text_cursor,
            "link child nodes must contain text"
        );
    }

    let is_partial_match = waiting_at_end(got_eof, input_str, &input_text_cursor);
    compare_text_contents(
        schema_str,
        input_str,
        &schema_text_cursor,
        &input_text_cursor,
        is_partial_match,
        false,
    )
}

fn validate_link_destination(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    let mut schema_text_cursor = schema_cursor.clone();
    let mut input_text_cursor = input_cursor.clone();

    #[cfg(feature = "invariant_violations")]
    {
        if !schema_text_cursor.goto_first_child() || !input_text_cursor.goto_first_child() {
            crate::invariant_violation!(
                result,
                &input_text_cursor,
                &schema_text_cursor,
                "link destination nodes must contain text"
            );
        }
    }

    let schema_text = match schema_text_cursor.node().utf8_text(schema_str.as_bytes()) {
        Ok(text) => text,
        Err(_) => return result,
    };
    let input_text = match input_text_cursor.node().utf8_text(input_str.as_bytes()) {
        Ok(text) => text,
        Err(_) => return result,
    };

    let is_partial_match = waiting_at_end(got_eof, input_str, &input_text_cursor);

    if let Some(matcher_result) = extract_matcher_from_curly_delineated_text(schema_text) {
        match matcher_result {
            Ok(matcher) => {
                if let Some(matched_str) = matcher.match_str(input_text) {
                    if let Some(id) = matcher.id() {
                        result.set_match(id, json!(matched_str));
                    }
                } else if !is_partial_match {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: schema_text_cursor.descendant_index(),
                            input_index: input_text_cursor.descendant_index(),
                            expected: matcher.pattern().to_string(),
                            actual: input_text.into(),
                            kind: NodeContentMismatchKind::Matcher,
                        },
                    ));
                }

                return result;
            }
            Err(MatcherError::WasLiteralCode) => {}
            Err(error) => {
                result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                    error,
                    schema_index: schema_text_cursor.descendant_index(),
                }));
                return result;
            }
        }
    }

    if let Some(matcher_result) = extract_matcher_from_curly_delineated_text(input_text) {
        if let Ok(matcher) = matcher_result {
            if let Some(matched_str) = matcher.match_str(schema_text) {
                if let Some(id) = matcher.id() {
                    result.set_match(id, json!(matched_str));
                }
            } else if !is_partial_match {
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_text_cursor.descendant_index(),
                        input_index: input_text_cursor.descendant_index(),
                        expected: matcher.pattern().to_string(),
                        actual: schema_text.into(),
                        kind: NodeContentMismatchKind::Matcher,
                    },
                ));
            }

            return result;
        }
    }

    if let Some(error) = compare_text_contents(
        schema_str,
        input_str,
        &schema_text_cursor,
        &input_text_cursor,
        is_partial_match,
        false,
    ) {
        result.add_error(error);
    }

    result
}

fn link_child_pos(schema_cursor: &TreeCursor, input_cursor: &TreeCursor) -> Option<NodePosPair> {
    let mut schema_text_cursor = schema_cursor.clone();
    let mut input_text_cursor = input_cursor.clone();

    if !schema_text_cursor.goto_first_child() || !input_text_cursor.goto_first_child() {
        return None;
    }

    Some(NodePosPair::from_cursors(
        &schema_text_cursor,
        &input_text_cursor,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::node_walker::validators::test_utils::ValidatorTester;

    use super::LinkVsLinkValidator;

    #[test]
    fn test_validate_link_vs_link_literal() {
        let schema_str = "[hi](https://test.com)";
        let input_str = "[hi](https://test.com)";

        let (value, errors, _) =
            ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .validate_complete()
                .destruct();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_link_vs_link_literal_mismatch() {
        let schema_str = "[hi](https://test.com)";
        let input_str = "[hi](https://different.com)";

        let (_value, errors, _) =
            ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .validate_complete()
                .destruct();

        assert!(!errors.is_empty());
    }

    #[test]
    fn test_validate_link_vs_link_destination_matcher_in_schema() {
        let schema_str = "[test]({foo:/\\w+/})";
        let input_str = "[test](hello)";

        let (value, errors, _) =
            ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .validate(true)
                .destruct();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"foo": "hello"}));
    }

    #[test]
    fn test_validate_link_vs_link_destination_matcher_in_input() {
        let schema_str = "[test](hello)";
        let input_str = "[test]({foo:/\\w+/})";

        let (value, errors, _) =
            ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .validate(true)
                .destruct();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"foo": "hello"}));
    }

    #[test]
    fn test_validate_image_vs_image_literal() {
        let schema_str = "![hi](https://test.com)";
        let input_str = "![hi](https://test.com)";

        let (value, errors, _) =
            ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .validate_complete()
                .destruct();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_image_vs_image_literal_mismatch() {
        let schema_str = "![hi](https://test.com)";
        let input_str = "![hi](https://different.com)";

        let (_value, errors, _) =
            ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
                .walk()
                .goto_first_child_then_unwrap()
                .goto_first_child_then_unwrap()
                .validate_complete()
                .destruct();

        assert!(!errors.is_empty());
    }
}
