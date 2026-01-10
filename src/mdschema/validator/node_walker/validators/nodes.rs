//! Node dispatch validator.
//!
//! Types:
//! - `NodeVsNodeValidator`: routes node-vs-node checks to the specific validator
//!   based on node kinds and performs shared structural checks.
use log::trace;

use crate::mdschema::validator::errors::{
    MalformedStructureKind, SchemaViolationError, ValidationError,
};
use crate::mdschema::validator::node_pos_pair::NodePosPair;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::validators::code::CodeVsCodeValidator;
use crate::mdschema::validator::node_walker::validators::containers::ContainerVsContainerValidator;
use crate::mdschema::validator::node_walker::validators::headings::HeadingVsHeadingValidator;
use crate::mdschema::validator::node_walker::validators::links::LinkVsLinkValidator;
use crate::mdschema::validator::node_walker::validators::lists::ListVsListValidator;
use crate::mdschema::validator::node_walker::validators::quotes::QuoteVsQuoteValidator;
use crate::mdschema::validator::node_walker::validators::tables::TableVsTableValidator;
use crate::mdschema::validator::node_walker::validators::textual::TextualVsTextualValidator;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
use crate::mdschema::validator::ts_types::*;
use crate::mdschema::validator::ts_utils::waiting_at_end;
use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::{compare_node_kinds_check, invariant_violation};

/// Validate two arbitrary nodes against each other.
///
/// Dispatches to the appropriate validator based on node types:
/// - Textual nodes -> `TextualVsTextualValidator::validate`
/// - Code blocks -> `CodeVsCodeValidator::validate`
/// - Lists -> `ListVsListValidator::validate`
/// - Headings/documents -> recursively validate children
#[derive(Default)]
pub struct NodeVsNodeValidator;

impl ValidatorImpl for NodeVsNodeValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        validate_node_vs_node_impl(walker, got_eof)
    }
}

fn validate_node_vs_node_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());
    let need_to_restart_result = result.clone();

    let schema_node = walker.schema_cursor().node();
    let input_node = walker.input_cursor().node();

    // Make mutable copies that we can walk
    let mut schema_cursor = walker.schema_cursor().clone();
    let mut input_cursor = walker.input_cursor().clone();

    // Both are textual nodes - use text_vs_text directly
    if both_are_textual_nodes(&schema_node, &input_node) {
        trace!("Both are textual nodes, validating text vs text");

        return TextualVsTextualValidator
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    }
    // Both are codeblock nodes
    else if both_are_codeblocks(&schema_node, &input_node) {
        return CodeVsCodeValidator
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    } else if both_are_quotes(&schema_node, &input_node) {
        return QuoteVsQuoteValidator
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    }
    // Both are tables
    else if both_are_tables(&schema_node, &input_node) {
        return TableVsTableValidator
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    }
    // Both are textual containers
    else if both_are_textual_containers(&schema_node, &input_node) {
        return ContainerVsContainerValidator::default()
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    }
    // Both are textual nodes
    else if both_are_textual_nodes(&schema_node, &input_node) {
        return TextualVsTextualValidator
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    }
    // Both are link nodes or image nodes
    else if both_are_link_nodes(&schema_node, &input_node)
        || both_are_image_nodes(&schema_node, &input_node)
    {
        return LinkVsLinkValidator
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    }
    // Both are list nodes
    else if both_are_list_nodes(&schema_node, &input_node) {
        return ListVsListValidator
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    }
    // Both are ruler nodes
    else if both_are_rulers(&schema_node, &input_node) {
        trace!("Both are rulers. No extra validation happens for rulers.");
    } else if both_are_headings(&schema_node, &input_node) {
        // First, if they are headings, validate the headings themselves.
        trace!("Both are heading nodes, validating heading vs heading");

        let heading_result = HeadingVsHeadingValidator
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);

        result.join_other_result(&heading_result);

        // If heading validation produced errors (e.g., mismatched heading levels),
        // don't validate children as they will also mismatch
        if heading_result.has_errors() {
            return result;
        }

        result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);
        return result;
    } else if both_are_matching_top_level_nodes(&schema_node, &input_node) {
        // Both are heading nodes or document nodes
        //
        // Crawl down one layer to get to the actual children
        trace!("Both are heading nodes or document nodes. Recursing into sibling pairs.");

        // Since we're dealing with top level nodes it is our responsibility to ensure that they have the same number of children.
        // compare_node_children_lengths_check!(schema_cursor, input_cursor, got_eof, result);

        let parent_pos = NodePosPair::from_cursors(&schema_cursor, &input_cursor);

        // Now actually go down to the children
        match (
            schema_cursor.goto_first_child(),
            input_cursor.goto_first_child(),
        ) {
            (true, true) => {
                let new_result = NodeVsNodeValidator
                    .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
                result.join_other_result(&new_result);
                result.sync_cursor_pos(&schema_cursor, &input_cursor);
            }
            (true, false) if waiting_at_end(got_eof, walker.input_str(), &input_cursor) => {
                // Stop for now. We will revalidate from here later.
                result.set_farthest_reached_pos(parent_pos);
                return result;
            }
            (false, true) => {
                if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                    // okay, we'll just wait!
                    return need_to_restart_result;
                } else {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::MalformedNodeStructure {
                            schema_index: schema_cursor.descendant_index(),
                            input_index: input_cursor.descendant_index(),
                            kind: MalformedStructureKind::InputHasChildSchemaDoesnt,
                        },
                    ));
                }
            }
            (true, false) => {
                if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                    // okay, we'll just wait!
                    return need_to_restart_result;
                } else {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::MalformedNodeStructure {
                            schema_index: schema_cursor.descendant_index(),
                            input_index: input_cursor.descendant_index(),
                            kind: MalformedStructureKind::SchemaHasChildInputDoesnt,
                        },
                    ));
                }
                return result;
            }
            (false, false) => {
                return result; // nothing left
            }
        }

        loop {
            match (
                schema_cursor.goto_next_sibling(),
                input_cursor.goto_next_sibling(),
            ) {
                (true, true) => {
                    let new_result = NodeVsNodeValidator
                        .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
                    result.join_other_result(&new_result);
                    result.sync_cursor_pos(&schema_cursor, &input_cursor);
                }
                (true, false) if waiting_at_end(got_eof, walker.input_str(), &input_cursor) => {
                    // Stop for now. We will revalidate from here later.
                    result.set_farthest_reached_pos(parent_pos);
                    return result;
                }
                (false, true) => {
                    if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                        // okay, we'll just wait!
                        return need_to_restart_result;
                    } else {
                        result.add_error(ValidationError::SchemaViolation(
                            SchemaViolationError::MalformedNodeStructure {
                                schema_index: schema_cursor.descendant_index(),
                                input_index: input_cursor.descendant_index(),
                                kind: MalformedStructureKind::InputHasChildSchemaDoesnt,
                            },
                        ));
                    }
                }
                (true, false) => {
                    if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                        // okay, we'll just wait!
                        return need_to_restart_result;
                    } else {
                        result.add_error(ValidationError::SchemaViolation(
                            SchemaViolationError::MalformedNodeStructure {
                                schema_index: schema_cursor.descendant_index(),
                                input_index: input_cursor.descendant_index(),
                                kind: MalformedStructureKind::SchemaHasChildInputDoesnt,
                            },
                        ));
                    }
                    return result;
                }
                (false, false) => break,
            }
        }

        return result;
    } else {
        // otherwise, at the minimum check the type
        compare_node_kinds_check!(
            schema_cursor,
            input_cursor,
            walker.schema_str(),
            walker.input_str(),
            result
        );

        if result.has_errors() {
            return result;
        }

        #[cfg(feature = "invariant_violations")]
        invariant_violation!(
            result,
            &schema_cursor,
            &input_cursor,
            "node kind comparison is not implemented yet for {:?} vs {:?}",
            schema_cursor.node().kind(),
            input_cursor.node().kind()
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::test_utils::ValidatorTester;
    use super::NodeVsNodeValidator;
    use crate::mdschema::validator::{
        errors::{MalformedStructureKind, SchemaViolationError, ValidationError},
        node_pos_pair::NodePosPair,
        ts_types::both_are_paragraphs,
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
    fn test_validate_heading_vs_heading_incomplete() {
        let schema_str = "# Test";
        let input_str = "#";

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .validate_incomplete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(1, 1));
        assert_eq!(result.errors(), vec![]);
        assert_eq!(result.value(), &json!({}));
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
                    .is_none_or(|obj| obj.is_empty())
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

        assert_eq!(
            result.errors(),
            &vec![ValidationError::SchemaViolation(
                SchemaViolationError::MalformedNodeStructure {
                    schema_index: 0,
                    input_index: 1,
                    kind: MalformedStructureKind::InputHasChildSchemaDoesnt,
                }
            )]
        );
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

    #[test]
    fn test_node_vs_node_repeated_paragraph_nothing_after() {
        let schema_str = r#"
`items`{,}
"#;
        let input_str = r#"
foo

bar

buzz
"#;

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(1, 5));
        assert_eq!(result.errors(), vec![]);
        assert_eq!(*result.value(), json!({"items": ["foo", "bar", "buzz"]}));
    }

    #[test]
    fn test_node_vs_node_repeated_paragraph_heading_after() {
        let schema_str = r#"
`items`{,}

# Test
"#;
        let input_str = r#"
foo

bar

buzz

# Test
"#;

        let result = ValidatorTester::<NodeVsNodeValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_paragraphs(s, i)))
            .validate_complete();

        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(5, 7) // at the subsequent heading
        );
        assert_eq!(result.errors(), vec![]);
        assert_eq!(*result.value(), json!({"items": ["foo", "bar", "buzz"]}));
    }
}
