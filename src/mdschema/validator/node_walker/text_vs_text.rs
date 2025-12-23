use log::trace;
use serde_json::json;
use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::*, matcher::matcher::{Matcher, MatcherError, get_everything_after_special_chars}, node_walker::ValidationResult, ts_utils::{
        compare_node_kinds, compare_text_contents, is_last_node, is_textual_node, waiting_at_end,
    }
};

/// Validate a textual region of input against a textual region of schema.
///
/// Both the input cursor and schema cursor should either:
/// - Both point to textual nodes, like "emphasis", "text", or similar.
/// - Both point to textual containers, like "heading_content", "paragraph", or similar.
///
/// If the schema cursor points to a text node, followed by a code node, maybe
/// followed by a text node, those three nodes are delegated as a "matcher"
/// group, and for that chunk of three text nodes, `matcher_vs_text` will be
/// used for validation.
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_text_vs_text(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    // Check if both nodes are textual nodes
    let input_node = input_cursor.node();
    let schema_node = schema_cursor.node();

    match extract_matcher_nodes(&schema_cursor) {
        Some((prefix_node, matcher_node, suffix_node)) => {
            // Try to create a matcher from the nodes
            match Matcher::try_from_nodes(matcher_node, suffix_node, schema_str) {
                // We got a matcher!
                Ok(matcher) => validate_matcher_vs_text(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                    (prefix_node, (matcher, matcher_node), suffix_node),
                ),
                // We attempted to parse a matcher, but it turns out it was actually just a literal code span
                Err(MatcherError::WasLiteralCode) => {
                    // If it's a regex error, treat it as regular textual content.
                    // So it's just another textual node, and we compare it directly, just like italic or any other textual node.
                    validate_textual_nodes(
                        &input_cursor,
                        &schema_cursor,
                        schema_str,
                        input_str,
                        got_eof,
                    )
                }
                // We got a matcher that's definitely a matcher, and is wrong
                Err(MatcherError::MatcherInteriorRegexInvalid(_)) => {
                    result.add_error(ValidationError::SchemaError(
                        SchemaError::InvalidMatcherContents {
                            schema_index: input_cursor.descendant_index(),
                            input_index: input_cursor.descendant_index(),
                        },
                    ));
                    return result;
                }
                Err(MatcherError::MatcherExtrasError(error)) => {
                    result.add_error(ValidationError::SchemaError(
                        SchemaError::InvalidMatcherExtras {
                            schema_index: input_cursor.descendant_index(),
                            input_index: input_cursor.descendant_index(),
                            error,
                        },
                    ));
                    return result;
                }
            }
        }
        None => {
            if is_textual_node(&input_node) && is_textual_node(&schema_node) {
                // Both are textual nodes, validate them directly
                return validate_textual_nodes(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                );
            }

            // First, count the children to check for length mismatches
            let input_child_count = input_cursor.node().child_count();
            let schema_child_count = schema_cursor.node().child_count();

            // Handle node mismatches
            {
                // If we have reached the EOF:
                //   No difference in the number of children
                // else:
                //   We can have less input children
                //
                let children_len_mismatch_err = ValidationError::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_cursor.descendant_index(),
                        expected: schema_child_count,
                        actual: input_child_count,
                    },
                );
                if got_eof {
                    // At EOF, children count must match exactly
                    if input_child_count != schema_child_count {
                        result.add_error(children_len_mismatch_err);
                        return result;
                    }
                } else {
                    // Not at EOF: input can have fewer children, but not more
                    if input_child_count > schema_child_count {
                        result.add_error(children_len_mismatch_err);
                        return result;
                    }
                }
            }

            // Move cursors to first child
            if !input_cursor.goto_first_child() || !schema_cursor.goto_first_child() {
                // No children to validate
                result.schema_descendant_index = schema_cursor.descendant_index();
                result.input_descendant_index = input_cursor.descendant_index();
                return result;
            }

            // Recursively validate children. If they weren't textual, that means they're textual containers.
            let child_result = validate_textual_container_children(
                &mut input_cursor,
                &mut schema_cursor,
                schema_str,
                input_str,
                got_eof,
                input_child_count,
            );

            result.join_other_result(&child_result);

            // Move cursors back to parent and then to next sibling if needed
            if !got_eof && schema_cursor.goto_next_sibling() && !input_cursor.goto_next_sibling() {
                // If we haven't gotten EOF, and the schema has more siblings and the input
                // doesn't, then just leave cursors where they are, since more siblings will
                // need to be validated.
            } else {
                input_cursor.goto_parent();
                schema_cursor.goto_parent();
                input_cursor.goto_next_sibling();
                schema_cursor.goto_next_sibling();
            }

            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();
            result
        }
    }
}

/// Actually perform textual node comparison. Used by `validate_text_vs_text` to
/// ensure that two SPECIFIC text nodes are the same.
///
/// # Arguments
/// * `input_cursor` - The cursor pointing to the input node, which must be a text node.
/// * `schema_cursor` - The cursor pointing to the schema node, which must be a text node.
/// * `schema_str` - The string representation of the schema node.
/// * `input_str` - The string representation of the input node.
/// * `got_eof` - Whether the end of file has been reached.
fn validate_textual_nodes(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    let schema_node = schema_cursor.node();
    let input_node = input_cursor.node();

    // Check node kind first
    if let Some(error) = compare_node_kinds(&schema_node, &input_node, schema_cursor, input_cursor)
    {
        result.add_error(error);
        return result;
    }

    // Then compare text contents
    if let Some(error) = compare_text_contents(
        &schema_node,
        &input_node,
        schema_str,
        input_str,
        schema_cursor,
        input_cursor,
        got_eof,
    ) {
        result.add_error(error);
        return result;
    }

    result
}

/// Validate children of text containers. This recurses into the children of two
/// text containers, and processes all of their siblings.
///
/// The schema and input cursors are advanced to the first child of the current
/// node, and then the siblings are walked in lock step checking each textual
/// node against the other.
fn validate_textual_container_children(
    input_cursor: &mut TreeCursor,
    schema_cursor: &mut TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
    input_child_count: usize,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    let mut i = 0;
    loop {
        let is_last_input_node = i == input_child_count - 1;

        let schema_child = schema_cursor.node();
        let input_child = input_cursor.node();

        // Check if both are textual nodes
        if is_textual_node(&input_child) && is_textual_node(&schema_child) {
            // Both are textual, compare them directly
            if let Some(error) =
                compare_node_kinds(&schema_child, &input_child, schema_cursor, input_cursor)
            {
                result.add_error(error);
                return result;
            }

            if let Some(error) = compare_text_contents(
                &schema_child,
                &input_child,
                schema_str,
                input_str,
                schema_cursor,
                input_cursor,
                is_last_input_node && !got_eof,
            ) {
                result.add_error(error);
                return result;
            }
        } else {
            // If not both textual, we need to recurse into them
            let child_result = validate_text_vs_text(
                input_cursor,
                schema_cursor,
                schema_str,
                input_str,
                got_eof && is_last_input_node,
            );
            result.join_other_result(&child_result);
            if !result.errors.is_empty() {
                return result;
            }
        }

        // Move to next siblings
        let has_next_input = input_cursor.goto_next_sibling();
        let has_next_schema = schema_cursor.goto_next_sibling();

        if !has_next_input || !has_next_schema {
            break;
        }

        i += 1;
    }

    result
}

/// Validate a sequence of nodes that includes a matcher node against a text
/// node. This is used for when we have 1-3 nodes, where there may be a center
/// node that is a code node that is a matcher.
///
/// The schema cursor should point at:
/// - A text node, followed by a code node, maybe followed by a text node
/// - A code node, maybe followed by a text node
/// - A code node only
///
/// You should not call this function directly. Instead, use the
/// `validate_text_vs_text` function with the cursors pointing to two text
/// nodes, and that may end up using this to do the matcher validation.
///
/// # Arguments
///
/// * `input_cursor` - The cursor pointing to the input text node.
/// * `schema_cursor` - The cursor pointing to the schema text node.
/// * `schema_str` - The string representation of the schema text node.
/// * `input_str` - The string representation of the input text node.
/// * `got_eof` - Whether the input text node has reached the end of the file.
/// * `matcher_group` - The optional prefix, matcher, and suffix nodes. This is
///   obtained by calling `get_matcher_group` on the schema cursor. We do this
///   ahead of time so we don't need to call it multiple times. If that function
///   returns `None` that means that we are not dealing with a matcher group.
///
/// # Returns
///
/// A `ValidationResult` indicating the result of the validation.
pub fn validate_matcher_vs_text<'a>(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
    matcher_group: (Option<Node<'a>>, (Matcher, Node<'a>), Option<Node<'a>>),
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        schema_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    // Mutable cursors that we can walk forward as we validate
    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

    // Destructure to make it easier to work with
    let (schema_prefix_node, (matcher, _matcher_node), schema_suffix_node) = matcher_group;

    // How far along we've validated the input. We'll update this as we go
    let mut input_byte_offset = input_cursor.node().byte_range().start;

    // Descendant index of the input node, specifically the paragraph (not the interior text)
    let input_node_descendant_index = input_cursor.descendant_index();
    input_cursor.goto_first_child();

    // Preserve the cursor where it's pointing at the prefix node for error reporting
    let mut schema_cursor_at_prefix = schema_cursor.clone();
    schema_cursor_at_prefix.goto_first_child(); // paragraph -> text

    // Walk the schema cursor forward one if we had a prefix, since
    // extract_text_matcher requires the cursor to be located at a code node
    if schema_prefix_node.is_some() {
        schema_cursor.goto_first_child(); // paragraph -> text
        debug_assert_eq!(schema_cursor.node().kind(), "text");
        schema_cursor.goto_next_sibling(); // code_span
    } else {
        schema_cursor.goto_first_child(); // paragraph -> code_span
    }
    debug_assert_eq!(schema_cursor.node().kind(), "code_span");

    // Only do prefix verification if there is a prefix
    if let Some(schema_prefix_node) = schema_prefix_node {
        trace!("Validating prefix before matcher");

        let schema_prefix_str = &schema_str[schema_prefix_node.byte_range()];
        let input_prefix_str =
            input_str.get(input_byte_offset..input_byte_offset + schema_prefix_str.len());

        // Check that the input extends enough that we can cover the full prefix.
        if let Some(input_prefix_str) = input_prefix_str {
            // Do the actual prefix comparison
            if schema_prefix_str != input_prefix_str {
                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor_at_prefix.descendant_index(),
                        input_index: input_node_descendant_index,
                        expected: schema_prefix_str.into(),
                        actual: input_prefix_str.into(),
                        kind: NodeContentMismatchKind::Prefix,
                    },
                ));

                // If prefix validation fails don't try to validate further.
                // TODO: In the future we could attempt to validate further anyway!
                result.schema_descendant_index = schema_cursor.descendant_index();
                result.input_descendant_index = input_cursor.descendant_index();
                return result;
            }

            input_byte_offset += schema_prefix_node.byte_range().len();
        } else if is_last_node(input_str, &input_cursor.node()) {
            // If we're waiting at the end, we can't validate the prefix yet
            let best_prefix_input_we_can_do = &input_str[input_byte_offset..];
            let best_prefix_length = best_prefix_input_we_can_do.len();
            let schema_prefix_partial = &schema_prefix_str[..best_prefix_length];

            if waiting_at_end(got_eof, input_str, &input_cursor) {
                trace!("Input prefix not long enough, but waiting at end of input");

                if schema_prefix_partial != best_prefix_input_we_can_do {
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: schema_cursor_at_prefix.descendant_index(),
                            input_index: input_node_descendant_index,
                            expected: schema_prefix_str.into(),
                            actual: best_prefix_input_we_can_do.into(),
                            kind: NodeContentMismatchKind::Prefix,
                        },
                    ));
                }
            } else {
                trace!("Input node is complete but no more input left, reporting mismatch error");

                result.add_error(ValidationError::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor_at_prefix.descendant_index(),
                        input_index: input_node_descendant_index,
                        expected: schema_prefix_str.into(),
                        actual: best_prefix_input_we_can_do.into(),
                        kind: NodeContentMismatchKind::Prefix,
                    },
                ));
            }

            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();
            return result;
        }
    }

    // All input that comes after the expected prefix
    let input_after_prefix = input_str[input_byte_offset..].to_string();

    // If the matcher is for a ruler, we should expect the entire input node to be a ruler
    if matcher.is_ruler() {
        trace!("Matcher is for a ruler, validating node type");

        if input_cursor.node().kind() != "thematic_break" {
            trace!("Input node is not a ruler, reporting type mismatch error");

            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: "thematic_break".into(),
                    actual: input_cursor.node().kind().into(),
                },
            ));
            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();

            return result;
        } else {
            // It's a ruler, no further validation needed
            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();

            return result;
        }
    }

    // Actually perform the match for the matcher
    match matcher.match_str(&input_after_prefix) {
        Some(matched_str) => {
            trace!("Matcher matched input string: {}", matched_str);

            input_byte_offset += matched_str.len();

            // Good match! Add the matched node to the matches (if it has an id)
            if let Some(id) = matcher.id() {
                trace!("Matcher matched input string: {}", matched_str);
                result.set_match(id, json!(matched_str));
            }
        }
        None => {
            trace!("Matcher did not match input string, reporting mismatch error");

            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: matcher.pattern().to_string(),
                    actual: input_after_prefix.into(),
                    kind: NodeContentMismatchKind::Matcher,
                },
            ));

            // TODO: should we validate further when we fail to match the matcher?
            result.schema_descendant_index = schema_cursor.descendant_index();
            result.input_descendant_index = input_cursor.descendant_index();
            return result;
        }
    }

    // Validate suffix if there is one
    if let Some(schema_suffix_node) = schema_suffix_node {
        schema_cursor.goto_next_sibling(); // code_span -> text
        debug_assert_eq!(schema_cursor.node().kind(), "text");

        // Everything that comes after the matcher
        let schema_suffix = {
            let text_node_after_code_node_str_contents =
                &schema_str[schema_suffix_node.byte_range()];
            // All text after the matcher node and maybe the text node right after it ("extras")
            get_everything_after_special_chars(text_node_after_code_node_str_contents).unwrap()
        };

        // Seek forward from the current input byte offset by the length of the suffix
        let input_suffix = &input_str[input_byte_offset..input_byte_offset + schema_suffix.len()];

        if schema_suffix != input_suffix {
            trace!(
                "Suffix mismatch: expected '{}', got '{}'",
                schema_suffix,
                input_suffix
            );

            result.add_error(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: schema_suffix.into(),
                    actual: input_suffix.into(),
                    kind: NodeContentMismatchKind::Suffix,
                },
            ));
        }
    }

    result.schema_descendant_index = schema_cursor.descendant_index();
    result.input_descendant_index = input_cursor.descendant_index();
    result
}

type SplitMatcherNodes<'a> = (Option<Node<'a>>, Node<'a>, Option<Node<'a>>);

/// Extracts the matcher node and optional prefix/suffix nodes from the list of schema nodes.
///
/// Returns a tuple of (prefix_node, matcher_node, suffix_node) where prefix and suffix can be None.
///
/// - `prefix_node`: A text node that comes before the matcher (optional)
/// - `matcher_node`: The code_span node that contains the matcher (required)
/// - `suffix_node`: A text node that comes after the matcher (optional)
///
/// The children must be in one of these forms:
/// - code_span (matcher only)
/// - text, code_span (prefix + matcher)
/// - code_span, text (matcher + suffix)
/// - text, code_span, text (prefix + matcher + suffix)
fn extract_matcher_nodes<'a>(schema_cursor: &TreeCursor<'a>) -> Option<SplitMatcherNodes<'a>> {
    let schema_nodes = schema_cursor.node().children(&mut schema_cursor.clone()).collect::<Vec<_>>();

    if schema_nodes.is_empty() {
        return None;
    }

    // Find code_span (should be one of the first two)
    let code_span_index = schema_nodes
        .iter()
        .position(|node| node.kind() == "code_span")?;

    let matcher_node = schema_nodes[code_span_index];

    let prefix_node = if code_span_index > 0 {
        Some(schema_nodes[0])
    } else {
        None
    };

    let suffix_node = if code_span_index + 1 < schema_nodes.len() {
        Some(schema_nodes[code_span_index + 1])
    } else {
        None
    };

    Some((prefix_node, matcher_node, suffix_node))
}

#[cfg(test)]
mod tests {
    use super::validate_matcher_vs_text as validate_matcher_vs_text_original;
    use serde_json::json;
    use tree_sitter::{TreeCursor};

    use crate::mdschema::validator::{
        errors::*, matcher::matcher::Matcher, node_walker::{
            ValidationResult, text_vs_text::{extract_matcher_nodes, validate_text_vs_text}
        }, ts_utils::parse_markdown
    };

    fn validate_matcher_vs_text<'a>(
        input_cursor: &TreeCursor,
        schema_cursor: &TreeCursor,
        schema_str: &str,
        input_str: &str,
        got_eof: bool,
    ) -> ValidationResult {
        match extract_matcher_nodes(&schema_cursor) {
            Some((prefix_node, matcher_node, suffix_node)) => {
                let matcher = Matcher::try_from_nodes(matcher_node, suffix_node, schema_str)
                    .expect("test utility expects valid matcher");
                validate_matcher_vs_text_original(input_cursor, schema_cursor, schema_str, input_str, got_eof, (prefix_node, (matcher, matcher_node), suffix_node))
            },
            None => unreachable!("this test utility is designed only for matchers and blows up for non matcher groups")
        }
    }

    #[test]
    fn test_text_vs_text_with_text_nodes() {
        let schema_str = "Some *Literal* **Other**";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some *Different* **Other**";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // Expect a NodeContentMismatch error for "Literal" vs "Different"
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert!(expected.contains("Literal"));
                assert!(actual.contains("Different"));
            }
            _ => panic!("Expected a NodeContentMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_different_node_count() {
        // Schema has more nodes than input when eof is true
        let schema_str = "Some *Literal* **Other**";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some **Other**";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
                expected,
                actual,
                ..
            }) => {
                assert_eq!(*expected, 4); // text, italic, text, strong
                assert_eq!(*actual, 2); // text, strong
            }
            _ => panic!("Expected a ChildrenLengthMismatch error!"),
        }

        // When eof is false, it's okay if input has fewer nodes (still waiting for input)
        let schema_str = "Some *Literal* **Other**";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some *Literal*";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // eof is false
        );

        // Should not error because fewer nodes is okay when not at EOF
        assert!(
            result.errors.is_empty(),
            "Expected no errors when input has fewer nodes and eof=false"
        );

        // But if input has MORE nodes than schema when eof is false, it should error
        let schema_str = "Some *Literal*";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Some *Literal* **Other**";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            false, // eof is false
        );

        // Should error because input has more nodes than schema
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
                ..
            }) => {
                // This is what we expect
            }
            _ => panic!("Expected a ChildrenLengthMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_matching_paragraphs() {
        let schema_str = "This is a paragraph with some *emphasis* and **bold** text.";
        let input_str = "This is a paragraph with some *emphasis* and **bold** text.";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            result.errors.is_empty(),
            "Expected no errors for matching paragraphs"
        );
    }

    #[test]
    fn test_text_vs_text_with_mismatched_paragraphs_not_at_end() {
        let schema_str = "This is a paragraph with *emphasis* and some trailing text.";
        let input_str = "This is a paragraph with *different* and some trailing text.";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert!(expected.contains("emphasis"));
                assert!(actual.contains("different"));
            }
            _ => panic!("Expected a NodeContentMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_mismatched_paragraphs() {
        let schema_str = "Hello world";
        let input_str = "Goodbye world";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert_eq!(expected, "Hello world");
                assert_eq!(actual, "Goodbye world");
            }
            _ => panic!("Expected a NodeContentMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_bold_mismatch() {
        let schema_str = "This has **bold** text";
        let input_str = "This has *italic* text";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeTypeMismatch {
                expected,
                actual,
                ..
            }) => {
                assert_eq!(expected, "strong_emphasis");
                assert_eq!(actual, "emphasis");
            }
            _ => panic!("Expected a NodeTypeMismatch error!"),
        }
    }

    #[test]
    fn test_text_vs_text_with_identical_bold_paragraphs() {
        let schema_str = "this is **bold** text.";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "this is **bold** text.";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate directly to the paragraph nodes
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // Should have no errors for identical content
        assert!(
            result.errors.is_empty(),
            "Expected no errors for identical paragraphs, got: {:?}",
            result.errors
        );
    }
    #[test]
    fn test_extract_matcher_nodes_code_span_only() {
        let schema_str = "`test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let result = extract_matcher_nodes(&schema_cursor);
        assert!(result.is_some());
        let (prefix, matcher_node, suffix) = result.unwrap();
        assert!(prefix.is_none());
        assert_eq!(matcher_node.kind(), "code_span");
        assert!(suffix.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_with_prefix_only() {
        let schema_str = "prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let result = extract_matcher_nodes(&schema_cursor);
        assert!(result.is_some());
        let (prefix, matcher_node, suffix) = result.unwrap();
        assert!(prefix.is_some());
        assert_eq!(prefix.unwrap().kind(), "text");
        assert_eq!(matcher_node.kind(), "code_span");
        assert!(suffix.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_with_prefix_and_suffix() {
        let schema_str = "prefix `test:/test/` suffix";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let result = extract_matcher_nodes(&schema_cursor);
        assert!(result.is_some());
        let (prefix, matcher_node, suffix) = result.unwrap();
        assert!(prefix.is_some());
        assert_eq!(prefix.unwrap().kind(), "text");
        assert_eq!(matcher_node.kind(), "code_span");
        assert!(suffix.is_some());
        assert_eq!(suffix.unwrap().kind(), "text");
    }

    #[test]
    fn test_extract_matcher_nodes_no_matcher_code_node() {
        let schema_str = "just text";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let result = extract_matcher_nodes(&schema_cursor);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_empty_list() {
        let schema_tree = parse_markdown("").unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document

        let result = extract_matcher_nodes(&schema_cursor);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_no_code_span() {
        let schema_str = "text only";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let result = extract_matcher_nodes(&schema_cursor);
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_matcher_vs_text_with_no_prefix_or_suffix() {
        let schema_str = "`test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "test";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix() {
        let schema_str = "prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_and_suffix() {
        let schema_str = "prefix `test:/test/` suffix";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix test suffix";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_not_long_enough() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        // When EOF is not set, we're waiting for more input
        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        // When waiting for more input without EOF, we shouldn't report errors yet
        // (The validation is incomplete)
        assert!(
            errors.is_empty(),
            "Should not have errors when waiting for more input: {:?}",
            errors
        );
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_good_so_far() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "prefix that is lo";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        // When EOF is not set, we're waiting for more input
        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        // When waiting for more input without EOF and prefix matches so far, we shouldn't report errors yet
        assert!(
            errors.is_empty(),
            "Should not have errors when waiting for more input: {:?}",
            errors
        );
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_but_bad_prefix() {
        let schema_str = "good prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "bad p";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        // When EOF is not set, we're waiting for more input
        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();
        drop(result);

        // Even though we're waiting for more input, if the prefix doesn't match what we have,
        // we should report an error
        assert!(
            !errors.is_empty(),
            "Should have errors when prefix doesn't match even while waiting for more input"
        );
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_empty() {
        let schema_str = "prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document

        // When EOF is not set and input is empty, we're waiting for more input
        // When EOF is not set, we're waiting for more input
        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false,
        );

        // When waiting for more input without EOF, we shouldn't report errors yet
        assert!(
            result.errors.is_empty(),
            "Should not have errors when waiting for more input with empty input: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_ruler() {
        let schema_str = "`ruler`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "---";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> thematic_break

        let result = validate_matcher_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        assert!(
            result.errors.is_empty(),
            "Errors found: {:?}",
            result.errors
        );
        // Rulers don't capture matches
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_invalid_regex_treated_as_textual() {
        // Test that invalid regex patterns are treated as regular textual content
        let schema_str = "`invalid:[regex/`"; // Invalid regex pattern (unclosed bracket)
        let input_str = "`invalid:[regex/`"; // Same invalid pattern

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        // Should succeed without errors because it's treated as textual content
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_literal_codeblock_fallback() {
        // Test that when a matcher has ! (literal codeblock), it falls back to textual validation
        let schema_str = "Here is `test:/\\w+/`! some text";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Here is `test:/\\w+/`! some text";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // Should succeed because it falls back to textual validation when ! is present
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn test_literal_codeblock_mismatch() {
        // Test that literal codeblock validation catches mismatches
        let schema_str = "Here is `test:/\\w+/`! some text";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Here is `different:/\\d+/`! some text";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // Should have an error because the literal codeblocks don't match
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                ..
            }) => {
                assert!(expected.contains("test:/\\w+/"));
                assert!(actual.contains("different:/\\d+/"));
            }
            _ => panic!("Expected a NodeContentMismatch error!"),
        }
    }

    #[test]
    fn test_mixed_extras_fallback() {
        // Test that mixed literal and non-literal extras fall back to textual validation
        let schema_str = "Here is `test:/\\w+/`!{2,3} some text";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Here is `test:/\\w+/`!{2,3} some text";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // Should succeed because it falls back to textual validation when mixed extras are invalid
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn test_invalid_regex_fallback() {
        // Test that invalid regex patterns fall back to textual validation
        let schema_str = "Here is `test:[unclosed` some text";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Here is `test:[unclosed` some text";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // Should succeed because it falls back to textual validation when regex is invalid
        assert_eq!(result.errors.len(), 0);
    }
}
