use log::trace;

use crate::compare_node_children_lengths_check;
use crate::mdschema::validator::errors::{SchemaError, ValidationError};
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::helpers::check_repeating_matchers::check_repeating_matchers;
use crate::mdschema::validator::node_walker::validators::code::CodeVsCodeValidator;
use crate::mdschema::validator::node_walker::validators::headings::HeadingVsHeadingValidator;
use crate::mdschema::validator::node_walker::validators::links::LinkVsLinkValidator;
use crate::mdschema::validator::node_walker::validators::lists::ListVsListValidator;
use crate::mdschema::validator::node_walker::validators::tables::TableVsTableValidator;
use crate::mdschema::validator::node_walker::validators::textual::TextualVsTextualValidator;
use crate::mdschema::validator::node_walker::validators::textual_container::TextualContainerVsTextualContainerValidator;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
use crate::mdschema::validator::ts_types::{
    both_are_codeblocks, both_are_image_nodes, both_are_link_nodes, both_are_list_nodes,
    both_are_matching_top_level_nodes, both_are_rulers, both_are_tables,
    both_are_textual_containers, both_are_textual_nodes, is_heading_node,
};
use crate::mdschema::validator::validator_walker::ValidatorWalker;

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
    let mut result = ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());

    let schema_str = walker.schema_str();
    let _input_str = walker.input_str();

    let schema_node = walker.schema_cursor().node();
    let input_node = walker.input_cursor().node();

    // Make mutable copies that we can walk
    let mut schema_cursor = walker.schema_cursor().clone();
    let mut input_cursor = walker.input_cursor().clone();

    // Both are textual nodes - use text_vs_text directly
    if both_are_textual_nodes(&schema_node, &input_node) {
        trace!("Both are textual nodes, validating text vs text");

        return TextualVsTextualValidator::validate(
            &walker.with_cursors(&schema_cursor, &input_cursor),
            got_eof,
        );
    }

    // Both are codeblock nodes
    if both_are_codeblocks(&schema_node, &input_node) {
        return CodeVsCodeValidator::validate(
            &walker.with_cursors(&schema_cursor, &input_cursor),
            got_eof,
        );
    }

    // Both are tables
    if both_are_tables(&schema_node, &input_node) {
        return TableVsTableValidator::validate(
            &walker.with_cursors(&schema_cursor, &input_cursor),
            got_eof,
        );
    }

    // Both are textual containers
    if both_are_textual_containers(&schema_node, &input_node) {
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
            &walker.with_cursors(&schema_cursor, &input_cursor),
            got_eof,
        );
    }

    // Both are textual nodes
    if both_are_textual_nodes(&schema_node, &input_node) {
        return TextualVsTextualValidator::validate(
            &walker.with_cursors(&schema_cursor, &input_cursor),
            got_eof,
        );
    }

    // Both are link nodes or image nodes
    if both_are_link_nodes(&schema_node, &input_node)
        || both_are_image_nodes(&schema_node, &input_node)
    {
        return LinkVsLinkValidator::validate(
            &walker.with_cursors(&schema_cursor, &input_cursor),
            got_eof,
        );
    }

    // Both are list nodes
    if both_are_list_nodes(&schema_node, &input_node) {
        return ListVsListValidator::validate(
            &walker.with_cursors(&schema_cursor, &input_cursor),
            got_eof,
        );
    }

    // Both are ruler nodes
    if both_are_rulers(&schema_node, &input_node) {
        trace!("Both are rulers. No extra validation happens for rulers.");
    }

    // Both are heading nodes or document nodes
    //
    // Crawl down one layer to get to the actual children
    if both_are_matching_top_level_nodes(&schema_node, &input_node) {
        // First, if they are headings, validate the headings themselves.
        if is_heading_node(&schema_node) && is_heading_node(&input_node) {
            trace!("Both are heading nodes, validating heading vs heading");

            let heading_result = HeadingVsHeadingValidator::validate(
                &walker.with_cursors(&schema_cursor, &input_cursor),
                got_eof,
            );
            result.join_other_result(&heading_result);
            result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);
            result.sync_cursor_pos(&schema_cursor, &input_cursor);
        }

        trace!("Both are heading nodes or document nodes. Recursing into sibling pairs.");

        // Since we're dealing with top level nodes it is our responsibility to ensure that they have the same number of children.
        compare_node_children_lengths_check!(schema_cursor, input_cursor, got_eof, result);

        // Now actually go down to the children
        if schema_cursor.goto_first_child() && input_cursor.goto_first_child() {
            let new_result = NodeVsNodeValidator::validate(
                &walker.with_cursors(&schema_cursor, &input_cursor),
                got_eof,
            );
            result.join_other_result(&new_result);
            result.sync_cursor_pos(&schema_cursor, &input_cursor);
        } else {
            return result; // nothing left
        }

        loop {
            // TODO: handle case where one has more children than the other
            let schema_had_sibling = schema_cursor.goto_next_sibling();
            let input_had_sibling = input_cursor.goto_next_sibling();
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
                    &walker.with_cursors(&schema_cursor, &input_cursor),
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
        node_walker::validators::{nodes::NodeVsNodeValidator, test_utils::ValidatorTester},
    };

    #[test]
    fn test_validate_node_vs_node_incomplete() {
        let schema_str = r#"
# `a:/.*/`

# `b:/.*/`

# `c:/.*/`
"#;

        let input_str = r#"
# a

# b

# c
"#;
        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({"a": "a", "b": "b", "c": "c"}));
    }

    #[test]
    fn test_validate_node_vs_node_doesnt_get_multiple_errors() {
        // Previously this test yielded multiple errors
        let schema_str = r#"# pre `assignment_number:/\d+/`"#;

        let input_str = r#"# pre dd"#;

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(
            result.errors().len(),
            1,
            "Expected exactly one error but found {:?}",
            result.errors()
        );
        assert!(
            result.value().is_null()
                || result
                    .value()
                    .as_object()
                    .map_or(true, |obj| obj.is_empty())
        );
    }

    #[test]
    fn test_validate_node_vs_node_with_with_nesting_lists() {
        let schema_str = r#"
- `test:/\w+/`{2,2}
  - `test2:/\w+/`{1,1}
"#;
        let input_str = r#"
- test1
- test2
  - deepy
"#;
        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(*result.errors(), []);

        assert_eq!(
            result.value(),
            &json!({
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

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_incomplete();

        assert_eq!(result.errors(), &[]);
        assert_eq!(result.value(), &json!({}));

        let schema_str2 = "This is *bold* text.";
        let input_str2 = "This is **bold** text.";

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str2, input_str2)
            .walk()
            .validate_incomplete();

        assert!(!result.errors().is_empty());
        assert_eq!(*result.value(), json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_simple_text_matcher() {
        let schema_str = "`name:/\\w+/`";
        let input_str = "Alice";

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(result.errors(), []);
        assert_eq!(*result.value(), json!({"name": "Alice"}));
    }

    #[test]
    fn test_validate_node_vs_node_with_empty_documents() {
        let schema_str = "";
        let input_str = "";

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(result.errors(), []);
        assert_eq!(*result.value(), json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_textual_container_without_matcher() {
        let schema_str = "Hello **world**";
        let input_str = "Hello **world**";

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(
            result.errors(),
            &[],
            "Expected no errors, got: {:?}",
            result.errors()
        );
        assert_eq!(*result.value(), json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_matcher_with_prefix_and_suffix() {
        let schema_str = "Hello `name:/\\w+/` world!";
        let input_str = "Hello Alice world!";

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(result.errors(), []);
        assert_eq!(*result.value(), json!({"name": "Alice"}));
    }

    #[test]
    fn test_validate_node_vs_node_with_empty_schema_with_non_empty_input() {
        let schema_str = "";
        let input_str = "# Some content\n";

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_ne!(result.errors(), []);

        match result.errors().first() {
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

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(result.errors(), []);
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_node_vs_node_with_simple_table() {
        let schema_str = r#"
|c1|c2|
|-|-|
|r1|r2|
"#;
        let input_str = r#"
|c1|c2|
|-|-|
|r1|r2|
"#;

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_complete();

        assert_eq!(result.errors(), []);
        assert_eq!(result.value(), &json!({}));
    }
}
