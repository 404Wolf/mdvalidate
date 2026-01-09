//! Textual container validator for node-walker comparisons.
//!
//! Types:
//! - `TextualContainerVsTextualContainerValidator`: walks inline children in
//!   paragraphs/emphasis and validates them with matcher support and link-aware
//!   handling.
use log::trace;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::node_walker::helpers::count_non_literal_matchers_in_children::count_non_literal_matchers_in_children;
use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::mdschema::validator::{
    errors::*,
    matcher::matcher::Matcher,
    node_walker::{
        ValidationResult,
        helpers::expected_input_nodes::expected_input_nodes,
        validators::{
            Validator, ValidatorImpl, links::LinkVsLinkValidator,
            textual::TextualVsTextualValidator,
        },
    },
    ts_types::*,
    ts_utils::count_siblings,
};
use crate::{compare_node_kinds_check, invariant_violation};

/// Validate a textual region of input against a textual region of schema.
///
/// Takes two cursors pointing at text containers in the schema and input, and
/// validates them. The input text container may have a single matcher, and
/// potentially many other types of nodes. For example:
///
/// Schema:
/// ```md
/// **Test** _*test*_ `test///`! `match:/test/` *foo*.
/// ```
///
/// Input:
/// ```md
/// **Test** _*test*_ `test///`! test *foo*.
///
/// # Algorithm
///
/// This works by:
///
/// 1. Count the number of top level matchers in the schema. Find the first
///    valid one. Then keep going, but if there are more than 1, error.
/// 2. Count the number of nodes for both the input and schema using special
///    utility that takes into account literal matchers.
/// 3. Walk the input and schema cursors at the same rate, and walk down ane
///    recurse, which takes us to our base case of directly validating the contents
///    and kind of the node. If the node we are at is a code node, look at it and
///    the next node. If the two nodes correspond to a literal matcher:
///    - Match the inside of the matcher against the corresponding code node in the input.
///    - Then if there is additional text in the subsequent text node after the code node,
///      check that there is a text node in the input, maybe error, and if there is,
///      validate that the contents of the rest of it is the same.
///    - Then move to the next node pair, hopping two nodes at once for the schema node.
pub(super) struct TextualContainerVsTextualContainerValidator;

impl ValidatorImpl for TextualContainerVsTextualContainerValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut result =
            ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());

        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        #[cfg(feature = "invariant_violations")]
        if !both_are_textual_containers(&schema_cursor.node(), &input_cursor.node()) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "expected textual container nodes"
            );
        }

        compare_node_kinds_check!(
            schema_cursor,
            input_cursor,
            walker.schema_str(),
            walker.input_str(),
            result
        );

        if is_repeated_matcher_paragraph(&schema_cursor, walker.schema_str()) {
            return ParagraphVsRepeatedMatcherParagraphValidator::validate(walker, got_eof);
        }

        match count_non_literal_matchers_in_children(&schema_cursor, walker.schema_str()) {
            Ok(non_repeating_matchers_count) if non_repeating_matchers_count > 1 && got_eof => {
                result.add_error(ValidationError::SchemaError(
                    SchemaError::MultipleMatchersInNodeChildren {
                        schema_index: schema_cursor.descendant_index(),
                        received: non_repeating_matchers_count,
                    },
                ))
            }
            Ok(_) => {
                // Exactly one non repeating matcher is OK!
            }
            Err(err) => {
                result.add_error(err);

                return result;
            }
        }

        let (expected_input_node_count, actual_input_node_count) = {
            let mut schema_cursor = schema_cursor.clone();
            schema_cursor.goto_first_child();

            let mut input_cursor = input_cursor.clone();
            input_cursor.goto_first_child();

            let expected_input_node_count =
                match expected_input_nodes(&schema_cursor, walker.schema_str()) {
                    Ok(expected_input_node_count) => expected_input_node_count,
                    Err(error) => {
                        result.add_error(error);
                        return result;
                    }
                };

            let actual_input_node_count = count_siblings(&input_cursor) + 1; // including the node we are currently at

            (expected_input_node_count, actual_input_node_count)
        };

        if (actual_input_node_count != expected_input_node_count) && got_eof {
            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::ChildrenLengthMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: ChildrenCount::from_specific(expected_input_node_count),
                    actual: actual_input_node_count,
                },
            ));
        }

        // Go from the container to the first child in the container, and then
        // iterate over the siblings at the same rate.
        match (
            input_cursor.goto_first_child(),
            schema_cursor.goto_first_child(),
        ) {
            (true, true) => {} // nothing to do
            (false, false) => {
                return result;
            }
            (true, false) => todo!(),
            (false, true) => todo!(),
        }

        loop {
            let pair_result = if both_are_link_nodes(&schema_cursor.node(), &input_cursor.node())
                || both_are_image_nodes(&schema_cursor.node(), &input_cursor.node())
            {
                LinkVsLinkValidator::validate(
                    &walker.with_cursors(&schema_cursor, &input_cursor),
                    got_eof,
                )
            } else {
                let new_result = TextualVsTextualValidator::validate(
                    &walker.with_cursors(&schema_cursor, &input_cursor),
                    got_eof,
                );
                new_result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);
                new_result
            };

            result.join_other_result(&pair_result);

            if !schema_cursor.goto_next_sibling() || !input_cursor.goto_next_sibling() {
                break;
            }
        }

        result
    }
}

/// We special case paragraphs that are just a single code node that is a
/// repeated matcher. This function attempts to match what we call a repeated
/// matcher paragraph.
///
/// If we see a paragraph that looks like:
///
/// ```md
/// # Hi there
///
/// `paragraphs:/.*/`{2,2}
/// ```
///
/// (Where the `` `test:/.*/`{,} `` is the paragraph)
///
/// Then we expect an input that has that many paragraphs, and we accumulate them into an array:
///
/// ```md
/// # Hi there
///
/// This is the first paragraph
///
/// This is the second paragraph
/// ```
///
/// And the output is
///
/// ```json
/// {
///     "paragraphs": [
///         "This is the first paragraph",
///         "This is the second paragraph"
///     ]
/// }
/// ```
pub(super) struct ParagraphVsRepeatedMatcherParagraphValidator;

impl ValidatorImpl for ParagraphVsRepeatedMatcherParagraphValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let result = ValidationResult::from_cursors(walker.schema_cursor(), &walker.input_cursor());

        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        #[cfg(feature = "invariant_violations")]
        if !both_are_paragraphs(
            &walker.schema_cursor().node(),
            &walker.input_cursor().node(),
        ) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "repeated containers are only possible for paragraphs"
            );
        }

        // Go from the container to the first child in the container, and then
        // iterate over the siblings at the same rate.
        match (
            input_cursor.goto_first_child(),
            schema_cursor.goto_first_child(),
        ) {
            (true, true) => {
                // Great, keep going
            }
            (false, false) => {
                // nothing to do
                return result;
            }
            (true, false) => todo!(),
            (false, true) => todo!(),
        }

        match Matcher::try_from_schema_cursor(&schema_cursor, walker.schema_str()) {
            Ok(matcher) if matcher.is_repeated() => {
                todo!()
            }
            _ => {
                #[cfg(feature = "invariant_violations")]
                invariant_violation!(
                    &schema_cursor,
                    &input_cursor,
                    "we should be at a repeated matcher"
                )
            }
        }
    }
}

/// Check whether a paragraph is a repeated paragraph matcher.
///
/// A paragraph is a repeated paragraph matcher if it has a single child, which
/// is a a repeated matcher.
///
/// For example,
///
/// ```
/// `test:/test/`{,}
/// ```
///
/// Contains a document with one child, which is a repeated paragraph matcher,
/// whereas
///
/// ```
/// `test:/test/` test
/// ```
///
/// Contains a document with one child, which is just a normal paragraph with a
/// matcher in it.
///
/// # Arguments
///
/// * `schema_cursor`: The cursor pointing to a paragraph that might be a repeated matcher paragraph.
/// * `schema_str`: The full input document (so far).
fn is_repeated_matcher_paragraph(schema_cursor: &TreeCursor, schema_str: &str) -> bool {
    // We must be at a paragraph node
    if !is_paragraph_node(&schema_cursor.node()) {
        trace!("is_repeated_matcher_paragraph: not a paragraph node, returning false");
        return false;
    }

    // All repeating matchers have a code span followed by text. This is a nonstarter.
    if schema_cursor.node().child_count() != 2 {
        trace!("is_repeated_matcher_paragraph: child count is not 2, returning false");
        return false;
    }

    let mut schema_cursor = schema_cursor.clone();
    schema_cursor.goto_first_child(); // note we know there is one because we checked above

    match Matcher::try_from_schema_cursor(&schema_cursor, schema_str) {
        Ok(matcher) if matcher.is_repeated() => true,
        Ok(_) => false,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{TextualContainerVsTextualContainerValidator, is_repeated_matcher_paragraph};
    use crate::mdschema::validator::{
        node_pos_pair::NodePosPair, node_walker::validators::test_utils::ValidatorTester,
        ts_types::*, ts_utils::parse_markdown,
    };

    #[test]
    fn test_is_repeated_matcher_paragraph_simple_non_paragraph() {
        let schema_str = "`test`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        assert!(is_inline_code_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_simple_non_repeating() {
        let schema_str = "this is just a normal paragraph";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_simple_repeating_matcher() {
        let schema_str = "`test:/test/`{,}";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_matcher_non_repeating() {
        let schema_str = "`test:/test/` test";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_matcher_invalid_matcher() {
        let schema_str = "`fjeiaofjioweajf` test";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_is_repeated_matcher_paragraph_matcher_valid_literal_matcher() {
        let schema_str = "`this is invalid`! test";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        assert!(is_paragraph_node(&schema_cursor.node()));
        assert!(!is_repeated_matcher_paragraph(&schema_cursor, schema_str));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_with_content_and_link() {
        let schema_str = "# Test Wolf [hi](https://example.com)";
        let input_str = "# Test Wolf [hi](https://foobar.com)";

        let result = ValidatorTester::<TextualContainerVsTextualContainerValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .goto_first_child_then_unwrap()
        .goto_next_sibling_then_unwrap()
        .peek_nodes(|(s, i)| assert!(is_heading_content_node(s) && is_heading_content_node(i)))
        .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(9, 9));
        assert!(!result.errors().is_empty());
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_header_content() {
        let schema_str = "# Test Wolf";
        let input_str = "# Test Wolf";

        let result = ValidatorTester::<TextualContainerVsTextualContainerValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .goto_first_child_then_unwrap()
        .goto_next_sibling_then_unwrap()
        .peek_nodes(|(s, i)| assert!(is_heading_content_node(s) && is_heading_content_node(i)))
        .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_header_content_and_matcher() {
        let schema_str = "# Test `name:/[a-zA-Z]+/`";
        let input_str = "# Test Wolf";

        let result = ValidatorTester::<TextualContainerVsTextualContainerValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .goto_first_child_then_unwrap()
        .goto_next_sibling_then_unwrap()
        .peek_nodes(|(s, i)| assert!(is_heading_content_node(s) && is_heading_content_node(i)))
        .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(6, 4));
        assert_eq!(result.errors(), &vec![]);
        assert_eq!(result.value(), &json!({"name": "Wolf"}));
    }

    #[test]
    fn test_validate_textual_container_vs_textual_container_link_then_bad_node() {
        let schema_str = "# Heading [test]({a:/a/}) `b:/b/`";
        let input_str = "# Heading [test](a) b";

        let result = ValidatorTester::<TextualContainerVsTextualContainerValidator>::from_strs(
            schema_str, input_str,
        )
        .walk()
        .goto_first_child_then_unwrap()
        .goto_first_child_then_unwrap()
        .goto_next_sibling_then_unwrap()
        .peek_nodes(|(s, i)| assert!(both_are_textual_containers(s, i)))
        .validate_complete();

        let errors = result.errors().to_vec();
        let value = result.value().clone();

        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(12, 10)
        );
        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"a": "a", "b": "b"}));
    }
}
