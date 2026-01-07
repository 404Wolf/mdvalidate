use log::trace;

use crate::mdschema::validator::errors::{SchemaError, ValidationError};
use crate::mdschema::validator::node_walker::helpers::check_repeating_matchers::check_repeating_matchers;
use crate::mdschema::validator::node_walker::validators::code::CodeVsCodeValidator;
use crate::mdschema::validator::node_walker::validators::headings::HeadingVsHeadingValidator;
use crate::mdschema::validator::node_walker::validators::links::LinkVsLinkValidator;
use crate::mdschema::validator::node_walker::validators::lists::ListVsListValidator;
use crate::mdschema::validator::node_walker::validators::textual::TextualVsTextualValidator;
use crate::mdschema::validator::node_walker::validators::textual_container::TextualContainerVsTextualContainerValidator;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
use crate::mdschema::validator::ts_utils::{
    both_are_codeblocks, both_are_image_nodes, both_are_link_nodes, both_are_list_nodes,
    both_are_matching_top_level_nodes, both_are_rulers, both_are_textual_containers,
    both_are_textual_nodes, is_heading_node,
};
use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::compare_node_children_lengths_check;

/// Validate two arbitrary nodes against each other.
///
/// Dispatches to the appropriate validator based on node types:
/// - Textual nodes -> `TextualVsTextualValidator::validate`
/// - Code blocks -> `CodeVsCodeValidator::validate`
/// - Lists -> `ListVsListValidator::validate`
/// - Headings/documents -> recursively validate children
pub struct NodeVsNodeValidator;

impl ValidatorImpl for NodeVsNodeValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        validate_node_vs_node_impl(walker, got_eof)
    }
}

fn validate_node_vs_node_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(walker.input_cursor(), walker.schema_cursor());

    let schema_str = walker.schema_str();
    let _input_str = walker.input_str();

    let input_node = walker.input_cursor().node();
    let schema_node = walker.schema_cursor().node();

    // Make mutable copies that we can walk
    let mut input_cursor = walker.input_cursor().clone();
    let mut schema_cursor = walker.schema_cursor().clone();

    // Both are textual nodes - use text_vs_text directly
    if both_are_textual_nodes(&input_node, &schema_node) {
        trace!("Both are textual nodes, validating text vs text");

        return TextualVsTextualValidator::validate(
            &walker.with_cursors(&input_cursor, &schema_cursor),
            got_eof,
        );
    }

    // Both are container nodes - use container_vs_container directly
    if both_are_codeblocks(&input_node, &schema_node) {
        trace!("Both are container nodes, validating container vs container");

        return CodeVsCodeValidator::validate(
            &walker.with_cursors(&input_cursor, &schema_cursor),
            got_eof,
        );
    }

    // Both are textual containers
    if both_are_textual_containers(&input_node, &schema_node) {
        trace!("Both are textual containers, validating text vs text");

        // If we have top level textual containers, they CANNOT have repeating
        // matchers. `validate_textual_container_vs_textual_container` allows
        // the containers to contain repeating matchers since the same utility
        // is used for list validation.

        if let Some(repeating_matcher_index) = check_repeating_matchers(&schema_cursor, schema_str)
        {
            result.add_error(ValidationError::SchemaError(
                SchemaError::RepeatingMatcherInTextContainer {
                    schema_index: repeating_matcher_index,
                },
            ));
            return result;
        }

        return TextualContainerVsTextualContainerValidator::validate(
            &walker.with_cursors(&input_cursor, &schema_cursor),
            got_eof,
        );
    }

    // Both are textual nodes
    if both_are_textual_nodes(&input_node, &schema_node) {
        trace!("Both are textual nodes, validating text vs text");

        return TextualVsTextualValidator::validate(
            &walker.with_cursors(&input_cursor, &schema_cursor),
            got_eof,
        );
    }

    // Both are link nodes or image nodes
    if both_are_link_nodes(&input_node, &schema_node)
        || both_are_image_nodes(&input_node, &schema_node)
    {
        trace!("Both are links or images, validating link vs link");

        return LinkVsLinkValidator::validate(
            &walker.with_cursors(&input_cursor, &schema_cursor),
            got_eof,
        );
    }

    // Both are list nodes
    if both_are_list_nodes(&input_node, &schema_node) {
        trace!("Both are list nodes, validating list vs list");

        return ListVsListValidator::validate(
            &walker.with_cursors(&input_cursor, &schema_cursor),
            got_eof,
        );
    }

    // Both are ruler nodes
    if both_are_rulers(&input_node, &schema_node) {
        trace!("Both are rulers. No extra validation happens for rulers.");
    }

    // Both are heading nodes or document nodes
    //
    // Crawl down one layer to get to the actual children
    if both_are_matching_top_level_nodes(&input_node, &schema_node) {
        trace!("Both are matching top level nodes. Checking of which kind.");

        // First, if they are headings, validate the headings themselves.
        if is_heading_node(&input_node) && is_heading_node(&schema_node) {
            trace!("Both are heading nodes, validating heading vs heading");

            let heading_result = HeadingVsHeadingValidator::validate(
                &walker.with_cursors(&input_cursor, &schema_cursor),
                got_eof,
            );
            result.join_other_result(&heading_result);
            result.sync_cursor_pos(&schema_cursor, &input_cursor);
        }

        trace!("Both are heading nodes or document nodes. Recursing into sibling pairs.");

        // Since we're dealing with top level nodes it is our responsibility to ensure that they have the same number of children.
        compare_node_children_lengths_check!(schema_cursor, input_cursor, got_eof, result);

        // Now actually go down to the children
        if input_cursor.goto_first_child() && schema_cursor.goto_first_child() {
            let new_result = NodeVsNodeValidator::validate(
                &walker.with_cursors(&input_cursor, &schema_cursor),
                got_eof,
            );
            result.join_other_result(&new_result);
            result.sync_cursor_pos(&schema_cursor, &input_cursor);
        } else {
            return result; // nothing left
        }

        loop {
            // TODO: handle case where one has more children than the other
            let input_had_sibling = input_cursor.goto_next_sibling();
            let schema_had_sibling = schema_cursor.goto_next_sibling();
            trace!(
                "input_had_sibling: {}, schema_had_sibling: {}, input_kind: {}, schema_kind: {}",
                input_had_sibling,
                schema_had_sibling,
                input_cursor.node().kind(),
                schema_cursor.node().kind()
            );

            if input_had_sibling && schema_had_sibling {
                trace!("Both input and schema node have siblings");

                let new_result = NodeVsNodeValidator::validate(
                    &walker.with_cursors(&input_cursor, &schema_cursor),
                    got_eof,
                );
                result.join_other_result(&new_result);
                result.sync_cursor_pos(&schema_cursor, &input_cursor);
            } else {
                trace!("One of input or schema node does not have siblings");

                break;
            }
        }

        return result;
    }
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::{
        errors::{ChildrenCount, SchemaViolationError, ValidationError},
        node_walker::validators::{Validator, nodes::NodeVsNodeValidator},
        ts_utils::parse_markdown,
        validator_walker::ValidatorWalker,
    };

    #[test]
    fn test_validate_node_vs_node_with_with_nesting_lists() {
        let schema_str = r#"
- `test:/\w+/`{2,2}
  - `test2:/\w+/`{1,1}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let schema_cursor = schema_tree.walk();

        let input_str = r#"
- test1
- test2
  - deepy
"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let input_cursor = input_tree.walk();
        assert_eq!(input_cursor.node().kind(), "document");

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
        let result = NodeVsNodeValidator::validate(&walker, true);

        assert_eq!(result.errors, vec![]);

        assert_eq!(
            result.value,
            json!({
                "test": [
                    "test1",
                    "test2",
                    { "test2": [ "deepy" ] }
                ]
            })
        );
    }

    #[test]
    fn test_validate_node_vs_node_with_two_mixed_paragraphs() {
        let schema_str = "this is **bold** text.";
        let input_str = "this is **bold** text.";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
        let result = NodeVsNodeValidator::validate(&walker, false);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({}));

        let schema_str2 = "This is *bold* text.";
        let input_str2 = "This is **bold** text.";
        let schema2 = parse_markdown(schema_str2).unwrap();
        let input2 = parse_markdown(input_str2).unwrap();

        let schema_cursor = schema2.walk();
        let input_cursor = input2.walk();

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str2, input_str2);
        let result = NodeVsNodeValidator::validate(&walker, false);

        assert!(!result.errors.is_empty());
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_simple_text_matcher() {
        let schema_str = "`name:/\\w+/`";
        let input_str = "Alice";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
        let result = NodeVsNodeValidator::validate(&walker, true);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({"name": "Alice"}));
    }

    #[test]
    fn test_validate_node_vs_node_with_empty_documents() {
        let schema_str = "";
        let input_str = "";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
        let result = NodeVsNodeValidator::validate(&walker, true);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_textual_container_without_matcher() {
        let schema_str = "Hello **world**";
        let input_str = "Hello **world**";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
        let result = NodeVsNodeValidator::validate(&walker, true);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_matcher_with_prefix_and_suffix() {
        let schema_str = "Hello `name:/\\w+/` world!";
        let input_str = "Hello Alice world!";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
        let result = NodeVsNodeValidator::validate(&walker, true);

        assert_eq!(result.errors, vec![]);
        assert_eq!(result.value, json!({"name": "Alice"}));
    }

    #[test]
    fn test_validate_node_vs_node_with_empty_schema_with_non_empty_input() {
        let schema_str = "";
        let input_str = "# Some content\n";
        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
        let result = NodeVsNodeValidator::validate(&walker, true);

        assert_ne!(result.errors, vec![]);

        match result.errors.first() {
            Some(error) => match error {
                ValidationError::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch { expected, .. },
                ) => {
                    assert_eq!(
                        *expected,
                        ChildrenCount::SpecificCount(0),
                        "expected should be 0 for empty schema"
                    );
                }
                _ => panic!("Expected ChildrenLengthMismatch error, got: {:?}", error),
            },
            None => panic!("Expected error"),
        }
    }

    #[test]
    fn test_validate_node_vs_node_with_heading_and_codeblock() {
        let schema_str = "## Heading\n```\nCode\n```";
        let input_str = "## Heading\n```\nCode\n```";

        let schema = parse_markdown(schema_str).unwrap();
        let input = parse_markdown(input_str).unwrap();

        let schema_cursor = schema.walk();
        let input_cursor = input.walk();

        let walker =
            ValidatorWalker::from_cursors(&input_cursor, &schema_cursor, schema_str, input_str);
        let result = NodeVsNodeValidator::validate(&walker, true);

        assert_eq!(
            result.errors,
            vec![],
            "Expected no errors, got: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }
}
