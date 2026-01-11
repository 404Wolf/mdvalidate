//! Heading validator for node-walker comparisons.
//!
//! Types:
//! - `HeadingVsHeadingValidator`: confirms heading kinds align and delegates
//!   content checks to textual container validation.
use log::trace;
use tree_sitter::TreeCursor;

use crate::invariant_violation;
use crate::mdschema::validation::errors::ValidationError;
use crate::mdschema::validation::walkers::ValidationResult;
use crate::mdschema::validation::walkers::helpers::compare_node_kinds::compare_node_kinds;
use crate::mdschema::validation::walkers::validators::containers::ContainerVsContainerValidator;
use crate::mdschema::validation::walkers::validators::{Validator, ValidatorImpl};
use crate::mdschema::validation::ts_types::*;
use crate::mdschema::validation::ts_utils::waiting_at_end;
use crate::mdschema::validation::validator_walker::ValidatorWalker;

/// Validate two headings.
///
/// Checks that they are the same kind of heading, and and then delegates to
/// `TextualContainerVsTextualContainerValidator::validate`.
#[derive(Default)]
pub(super) struct HeadingVsHeadingValidator;

impl ValidatorImpl for HeadingVsHeadingValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut result =
            ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());

        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        // Both should be the start of headings
        #[cfg(feature = "invariant_violations")]
        if !is_heading_node(&schema_cursor.node()) || !is_heading_node(&input_cursor.node()) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "heading validation expects atx_heading nodes"
            );
        }

        // This also checks the *type* of heading that they are at
        if let Some(error) = compare_node_kinds(
            &schema_cursor,
            &input_cursor,
            walker.schema_str(),
            walker.input_str(),
        ) {
            if waiting_at_end(got_eof, walker.input_str(), &input_cursor)
                && both_are_headings(&schema_cursor.node(), &input_cursor.node())
            {
            } else {
                result.add_error(error);
                return result;
            }
        };
        trace!("Node kinds mismatched");

        // Go to the actual heading content
        {
            let mut failed_to_walk_to_heading = false;
            let input_had_heading_content = match ensure_at_heading_content(&mut input_cursor) {
                Ok(had_content) => had_content,
                Err(error) => {
                    result.add_error(error);
                    failed_to_walk_to_heading = true;
                    false
                }
            };
            let schema_had_heading_content = match ensure_at_heading_content(&mut schema_cursor) {
                Ok(had_content) => had_content,
                Err(error) => {
                    result.add_error(error);
                    failed_to_walk_to_heading = true;
                    false
                }
            };
            if failed_to_walk_to_heading
                || !(schema_had_heading_content && input_had_heading_content)
            {
                return result;
            }
        }
        result.sync_cursor_pos(&schema_cursor, &input_cursor); // save progress

        // Both should be at markers
        #[cfg(feature = "invariant_violations")]
        if !is_textual_container_node(&schema_cursor.node())
            || !is_textual_container_node(&input_cursor.node())
        {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "heading validation expects textual container nodes"
            );
        }

        // Now that we're at the heading content, use `validate_text_vs_text`
        ContainerVsContainerValidator::default()
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof)
    }
}

fn ensure_at_heading_content(cursor: &mut TreeCursor) -> Result<bool, ValidationError> {
    // Headings look like this:
    //
    // (atx_heading)
    // │  ├─ (atx_h2_marker)
    // │  └─ (heading_content)
    // │     └─ (text)
    if is_heading_node(&cursor.node()) {
        cursor.goto_first_child();
        ensure_at_heading_content(cursor)
    } else if is_marker_node(&cursor.node()) {
        if cursor.goto_next_sibling() {
            #[cfg(feature = "invariant_violations")]
            if !is_heading_content_node(&cursor.node()) {
                invariant_violation!(cursor, cursor, "expected heading_content node");
            }
            Ok(is_heading_content_node(&cursor.node()))
        } else {
            Ok(false)
        }
    } else {
        #[cfg(feature = "invariant_violations")]
        invariant_violation!(
            cursor,
            cursor,
            "Expected to be at heading content, but found node kind: {}",
            cursor.node().kind()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_utils::ValidatorTester;
    use super::*;
    use crate::mdschema::validation::{
        errors::{NodeContentMismatchKind, SchemaViolationError},
        node_pos_pair::NodePosPair,
        ts_utils::parse_markdown,
    };
    use serde_json::json;

    #[test]
    fn test_ensure_at_heading_content() {
        // Test starting from root of heading
        let input_str = "# test heading";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert!(is_heading_node(&input_cursor.node()));

        ensure_at_heading_content(&mut input_cursor).unwrap();
        assert!(is_heading_content_node(&input_cursor.node()));

        // Test starting from marker node
        let input_str = "## test heading";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        input_cursor.goto_first_child();
        assert!(is_marker_node(&input_cursor.node()));

        ensure_at_heading_content(&mut input_cursor).unwrap();
        assert!(is_heading_content_node(&input_cursor.node()));
    }

    #[test]
    fn test_validate_heading_vs_heading_simple_headings_so_far_wrong_type() {
        let schema_str = "### Heading `foo:/test/`";
        let input_str = "#";

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_headings(s, i)))
            .validate_incomplete();

        assert!(result.errors().is_empty()); // no errors yet
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_heading_vs_heading_headings_three_children() {
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

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .with_schema_cursor(|c| c.goto_descendant(13))
            .with_input_cursor(|c| c.goto_descendant(9))
            .validate_complete();

        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(18, 12)
        );
        assert!(result.errors().is_empty()); // no errors yet
        assert_eq!(result.value(), &json!({"c": "c"}));
    }

    #[test]
    fn test_validate_heading_vs_heading_with_link() {
        let schema_str = "# [test]({test:/test/})";
        let input_str = "# [test](test)";

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_headings(s, i)))
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(9, 9));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_heading_vs_heading_with_link_and_prefix() {
        let schema_str = "# Heading [test]({test:/test/})";
        let input_str = "# Heading [test](test)";

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_headings(s, i)))
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(9, 9));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_heading_vs_heading_with_link_and_prefix_and_matcher() {
        let schema_str = "# Heading [test]({a:/a/}) `b:/b/`";
        let input_str = "# Heading [test](a) b";

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_headings(s, i)))
            .validate_complete();

        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(12, 10)
        );
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"a": "a", "b": "b"}));
    }

    #[test]
    fn test_validate_heading_vs_heading_with_link_and_prefix_and_wrong_text() {
        let schema_str = "# Heading [test]({a:/a/}) foo";
        let input_str = "# Heading [test](a) bar";

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_headings(s, i)))
            .validate_complete();

        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(10, 10)
        );
        assert_eq!(
            *result.errors(),
            [ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: 10,
                    input_index: 10,
                    expected: " foo".to_string(),
                    actual: " bar".to_string(),
                    kind: NodeContentMismatchKind::Literal,
                }
            )]
        );
        assert_eq!(result.value(), &json!({"a": "a"}));
    }

    #[test]
    fn test_validate_heading_vs_heading_with_matcher() {
        let schema_str = "# Heading `test:/test/`";
        let input_str = "# Heading test";

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_headings(s, i)))
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(6, 4));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({"test": "test"}));
    }

    #[test]
    fn test_validate_heading_vs_heading_simple_headings() {
        let schema_str = "# Heading";
        let input_str = "# Heading";

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_headings(s, i)))
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert!(result.errors().is_empty());
        assert_eq!(result.value(), &json!({})); // No real match content
    }

    #[test]
    fn test_validate_heading_vs_heading_wrong_heading_kind() {
        let schema_str = "# Heading";
        let input_str = "## Heading";

        let result = ValidatorTester::<HeadingVsHeadingValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_headings(s, i)))
            .validate_complete();

        assert_eq!(
            result.errors(),
            &[ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: 1,
                    input_index: 1,
                    expected: "atx_heading(atx_h1_marker)".to_string(),
                    actual: "atx_heading(atx_h2_marker)".to_string(),
                }
            )]
        );
        assert_eq!(result.value(), &json!({}));
    }
    // TODO: tests for got_eof=false
}
