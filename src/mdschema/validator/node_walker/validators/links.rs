use serde_json::json;
use tree_sitter::TreeCursor;

use crate::invariant_violation;
use crate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaError, SchemaViolationError, ValidationError,
};
use crate::mdschema::validator::matcher::matcher::MatcherError;
use crate::mdschema::validator::node_pos_pair::NodePosPair;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::helpers::compare_text_contents::compare_text_contents;
use crate::mdschema::validator::node_walker::helpers::curly_matchers::extract_matcher_from_curly_delineated_text;
use crate::mdschema::validator::node_walker::validators::ValidatorImpl;
#[cfg(feature = "invariant_violations")]
use crate::mdschema::validator::ts_types::both_are_link_description_nodes;
use crate::mdschema::validator::ts_types::{
    both_are_text_nodes, is_image_node, is_link_destination_node, is_link_node,
};
use crate::mdschema::validator::ts_utils::{get_node_text, waiting_at_end};
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

    let mut schema_cursor = walker.schema_cursor().clone();
    let mut input_cursor = walker.input_cursor().clone();

    compare_node_kinds_check!(schema_cursor, input_cursor, schema_str, input_str, result);

    if let Err(error) = ensure_at_link_start(&mut input_cursor) {
        result.add_error(error);
        return result;
    }

    if let Err(error) = ensure_at_link_start(&mut schema_cursor) {
        result.add_error(error);
        return result;
    }

    let link_input_cursor = input_cursor.clone();

    if !schema_cursor.goto_first_child() || !input_cursor.goto_first_child() {
        #[cfg(feature = "invariant_violations")]
        invariant_violation!(
            result,
            &schema_cursor,
            &input_cursor,
            "link nodes must have children"
        );
    }

    compare_node_kinds_check!(schema_cursor, input_cursor, schema_str, input_str, result);

    // We're now at the alt
    //
    // ├─ (text[4]1..10)
    // └─ (link[5]10..31)
    //    ├─ (<alt>[6]11..15)
    //    │  └─ (text[7]11..15)
    //    └─ (<src>[8]17..30)
    //       └─ (text[9]17..30)
    #[cfg(feature = "invariant_violations")]
    if !both_are_link_description_nodes(&schema_cursor.node(), &input_cursor.node()) {
        invariant_violation!(
            result,
            &schema_cursor,
            &input_cursor,
            "we should be at link text, but at {:?}",
            input_cursor.node().kind()
        );
    }

    let child_result = compare_link_child_text(
        &schema_cursor,
        &input_cursor,
        schema_str,
        input_str,
        got_eof,
    );
    result.join_other_result(&child_result);
    if child_result.has_errors() {
        return result;
    }

    if let Some(pos) = link_child_pos(&schema_cursor, &input_cursor) {
        result.keep_farther_pos(&pos);
    }

    #[cfg(feature = "invariant_violations")]
    if !schema_cursor.goto_next_sibling() || !input_cursor.goto_next_sibling() {
        invariant_violation!(
            result,
            &schema_cursor,
            &input_cursor,
            "link nodes must have a destination"
        );
    }

    compare_node_kinds_check!(schema_cursor, input_cursor, schema_str, input_str, result);

    if is_link_destination_node(&schema_cursor.node()) {
        let destination_result = validate_link_destination(
            &schema_cursor,
            &input_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        result.join_other_result(&destination_result);
        // Don't return early since we want to move the cursor (20 lines down) first
    } else {
        let child_result = compare_link_child_text(
            &schema_cursor,
            &input_cursor,
            schema_str,
            input_str,
            got_eof,
        );
        result.join_other_result(&child_result);
        if child_result.has_errors() {
            return result;
        }
    }

    if !waiting_at_end(got_eof, input_str, &link_input_cursor)
        && let Some(pos) = link_child_pos(&schema_cursor, &input_cursor)
    {
        result.keep_farther_pos(&pos);
    } else {
        result.sync_cursor_pos(&schema_cursor, &input_cursor);
    }

    result
}

fn ensure_at_link_start(cursor: &mut TreeCursor) -> Result<(), ValidationError> {
    if is_link_node(&cursor.node()) || is_image_node(&cursor.node()) {
        return Ok(());
    }

    #[cfg(feature = "invariant_violations")]
    invariant_violation!(
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
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    let mut schema_text_cursor = schema_cursor.clone();
    let mut input_text_cursor = input_cursor.clone();

    if !schema_text_cursor.goto_first_child() || !input_text_cursor.goto_first_child() {
        #[cfg(feature = "invariant_violations")]
        invariant_violation!(
            result,
            &schema_text_cursor,
            &input_text_cursor,
            "link child nodes must contain text"
        );
    }

    let is_partial_match = waiting_at_end(got_eof, input_str, &input_text_cursor);
    let text_result = compare_text_contents(
        schema_str,
        input_str,
        &schema_text_cursor,
        &input_text_cursor,
        is_partial_match,
        false,
    );
    // Only take errors and values, not position (parent already tracks position at link level)
    result.join_data(text_result.data());

    result
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
            invariant_violation!(
                result,
                &schema_text_cursor,
                &input_text_cursor,
                "link destination nodes must contain text"
            );
        }
    }

    let schema_text = get_node_text(&schema_text_cursor.node(), schema_str);
    let input_text = get_node_text(&input_text_cursor.node(), input_str);

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

    let text_result = compare_text_contents(
        schema_str,
        input_str,
        &schema_text_cursor,
        &input_text_cursor,
        is_partial_match,
        false,
    );
    // Only take errors and values, not position (parent already tracks position at link level)
    result.join_data(text_result.data());

    result
}

fn link_child_pos(schema_cursor: &TreeCursor, input_cursor: &TreeCursor) -> Option<NodePosPair> {
    let mut schema_text_cursor = schema_cursor.clone();
    let mut input_text_cursor = input_cursor.clone();

    if !schema_text_cursor.goto_first_child() || !input_text_cursor.goto_first_child() {
        return None;
    }

    #[cfg(feature = "invariant_violations")]
    if !both_are_text_nodes(&schema_text_cursor.node(), &input_text_cursor.node()) {
        invariant_violation!(
            &schema_text_cursor,
            &input_text_cursor,
            "link child nodes must both be text nodes"
        );
    }

    Some(NodePosPair::from_cursors(
        &schema_text_cursor,
        &input_text_cursor,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::{
        node_pos_pair::NodePosPair, node_walker::validators::test_utils::ValidatorTester,
    };

    use super::LinkVsLinkValidator;

    #[test]
    fn test_validate_link_vs_link_literal() {
        let schema_str = "[hi](https://test.com)";
        let input_str = "[hi](https://test.com)";

        let tester = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str);
        let mut tester_walker = tester.walk();
        tester_walker
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap();

        let result = tester_walker.validate_incomplete();
        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(5, 5));
        // Don't go "inside" the link source unless we are EOF. If we are EOF we scrape all the way to the very very end.
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({}));

        let result = tester_walker.validate_complete();
        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(6, 6));
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_link_vs_link_literal_mismatch() {
        let schema_str = "[hi](https://test.com)";
        let input_str = "[hi](https://different.com)";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(6, 6));
        assert!(!result.errors().is_empty());
    }

    #[test]
    fn test_validate_link_vs_link_destination_matcher_in_schema() {
        let schema_str = "[test]({foo:/\\w+/})";
        let input_str = "[test](hello)";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate(true);

        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({"foo": "hello"}));
    }

    #[test]
    fn test_validate_link_vs_link_destination_matcher_in_input() {
        let schema_str = "[test](hello)";
        let input_str = "[test]({foo:/\\w+/})";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate(true);

        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({"foo": "hello"}));
    }

    #[test]
    fn test_validate_image_vs_image_literal() {
        let schema_str = "![hi](https://test.com)";
        let input_str = "![hi](https://test.com)";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_image_vs_image_literal_mismatch() {
        let schema_str = "![hi](https://test.com)";
        let input_str = "![hi](https://different.com)";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert!(!result.errors().is_empty());
    }

    #[test]
    fn test_validate_link_vs_link_alt_text_matcher_in_schema() {
        let schema_str = "[{foo:/\\w+/}](https://test.com)";
        let input_str = "[hello](https://test.com)";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({"foo": "hello"}));
    }

    #[test]
    fn test_validate_link_vs_link_alt_text_matcher_mismatch() {
        let schema_str = "[{foo:/\\d+/}](https://test.com)";
        let input_str = "[hello](https://test.com)";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert!(!result.errors().is_empty());
    }

    #[test]
    fn test_validate_image_vs_image_alt_text_matcher() {
        let schema_str = "![{desc:/.+/}](image.png)";
        let input_str = "![test image](image.png)";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({"desc": "test image"}));
    }

    #[test]
    fn test_validate_link_both_alt_and_destination_matchers() {
        let schema_str = "[{text:/\\w+/}]({url:/.+/})";
        let input_str = "[hello](https://test.com)";

        let result = ValidatorTester::<LinkVsLinkValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(result.errors(), &vec![]);
        assert_eq!(
            result.value(),
            &json!({"text": "hello", "url": "https://test.com"})
        );
    }
}
