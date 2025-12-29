use log::trace;
use serde_json::json;
use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::*,
    matcher::matcher::{Matcher, MatcherError, get_everything_after_special_chars},
    node_walker::{ValidationResult, node_vs_node::validate_node_vs_node},
    ts_utils::{
        both_are_text_nodes, get_node_and_next_node, is_last_node, is_textual_node, waiting_at_end,
    },
    utils::{compare_node_children_lengths, compare_node_kinds, compare_text_contents},
};

/// Validate a textual region of input against a textual region of schema.
///
/// Handles text nodes (emphasis, strong, text) and textual containers (paragraphs, headings).
/// When the schema contains a text-code-text pattern, those nodes form a "matcher group"
/// and are validated using `validate_matcher_vs_text` instead of literal comparison.
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    i = %input_cursor.descendant_index(),
    s = %schema_cursor.descendant_index(),
), ret)]
pub fn validate_text_vs_text(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    // Some invariants due to previous bugs
    debug_assert_ne!(input_cursor.node().kind(), "tight_list");
    debug_assert_ne!(schema_cursor.node().kind(), "tight_list");

    // If we are at a `code_span` and we haven't reached the EOF, if we don't
    // have a proceeding node, don't validate for now. We want to process *with*
    // extras if there are extras, otherwise we may erroneously error early.
    if input_cursor.node().kind() == "code_span"
        && !got_eof
        && !input_cursor.clone().goto_next_sibling()
    {
        trace!("At code_span without proceeding node and not at EOF, deferring validation");
        return result;
    }

    // Check if both nodes are textual nodes
    match matcher_count_in_children(&schema_cursor, schema_str) {
        Ok(count) => {
            if count > 1 {
                result.add_error(ValidationError::SchemaError(
                    SchemaError::MultipleMatchersInNodeChildren {
                        schema_index: schema_cursor.descendant_index(),
                        received: count,
                    },
                ));
                return result;
            } else {
                // If there is a code node, it must be the very first node
            }
        }
        Err(error) => {
            result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                error,
                schema_index: schema_cursor.descendant_index(),
            }));
            return result;
        }
    };

    match extract_matcher_nodes(&schema_cursor) {
        Some((prefix_node, matcher_node, suffix_node)) => {
            trace!("Found potential matcher nodes in schema");
            // Try to create a matcher from the nodes
            match try_from_code_and_text_node(matcher_node, suffix_node, schema_str) {
                // We got a matcher!
                Ok(matcher) => {
                    trace!("Successfully created matcher, delegating to validate_matcher_vs_text");

                    let matcher_result = validate_matcher_vs_text(
                        &input_cursor,
                        &schema_cursor,
                        schema_str,
                        input_str,
                        got_eof,
                        (prefix_node, (matcher, matcher_node), suffix_node),
                    );
                    result.join_other_result(&matcher_result);
                    result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);

                    validate_text_vs_text(
                        &input_cursor,
                        &schema_cursor,
                        schema_str,
                        input_str,
                        got_eof,
                    )
                }
                // We attempted to parse a matcher, but it turns out it was actually just a literal code span
                Err(MatcherError::WasLiteralCode) => {
                    trace!("Matcher parsing gave WasLiteralCode, treating as literal code span");

                    // Since we're at the paragraph (not the text or code span itself), walk down one
                    input_cursor.goto_first_child();
                    schema_cursor.goto_first_child();

                    // (paragraph)
                    // ├── (text) <-- optional. We (might) be here.
                    // ├── (code_span)
                    // │   └── (text)
                    // ├── (text) <-- input has a "!" to signal literal matcher (how we got here)
                    // └── <more nodes in the paragraph>

                    if both_are_text_nodes(&input_cursor.node(), &schema_cursor.node()) {
                        let prefix_text_result = validate_text_vs_text(
                            &input_cursor,
                            &schema_cursor,
                            schema_str,
                            input_str,
                            got_eof,
                        );
                        result.join_other_result(&prefix_text_result);
                        result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);
                    } else {
                        debug_assert_eq!(input_cursor.node().kind(), "code_span");
                        debug_assert_eq!(schema_cursor.node().kind(), "code_span");
                    }

                    // Now validate the actual codespan
                    {
                        let input_codespan_cursor = input_cursor.clone();
                        let schema_codespan_cursor = schema_cursor.clone();

                        input_cursor.goto_first_child();
                        schema_cursor.goto_first_child();

                        let matcher_result = validate_text_vs_text(
                            &input_codespan_cursor,
                            &schema_codespan_cursor,
                            schema_str,
                            input_str,
                            got_eof,
                        );

                        result.join_other_result(&matcher_result);

                        schema_cursor.reset_to(&schema_codespan_cursor);
                        input_cursor.reset_to(&input_codespan_cursor);
                        debug_assert_eq!(schema_cursor.node().kind(), "code_span");
                        debug_assert_eq!(input_cursor.node().kind(), "code_span");
                        input_cursor.goto_next_sibling();
                        debug_assert_eq!(input_cursor.node().kind(), "text");
                    }

                    // For the final text node, we treat it specially by calling `validate_textual_nodes` with
                    {
                        let mut schema_cursor = schema_cursor.clone();
                        schema_cursor.goto_next_sibling();
                        assert_eq!(schema_cursor.node().kind(), "text");

                        let intermediate_result = validate_textual_nodes(
                            &input_cursor,
                            &schema_cursor,
                            schema_str,
                            input_str,
                            got_eof,
                            true, // Strip off the ! when validating this text node
                        );
                        result.join_other_result(&intermediate_result);
                        if intermediate_result.has_errors() {
                            return result;
                        }
                    }

                    // Now go to the next sibling if there is one and validate the rest!
                    if input_cursor.goto_next_sibling() && schema_cursor.goto_next_sibling() {
                        return validate_text_vs_text(
                            &input_cursor,
                            &schema_cursor,
                            schema_str,
                            input_str,
                            got_eof,
                        );
                    } else {
                        // We're done!
                        return result;
                    }
                }
                // We got a matcher that's definitely a matcher, and is wrong
                Err(error @ MatcherError::MatcherInteriorRegexInvalid(_)) => {
                    trace!("Error: Invalid regex in matcher: {:?}", error);
                    result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                        error,
                        schema_index: schema_cursor.descendant_index(),
                    }));
                    return result;
                }
                Err(MatcherError::MatcherExtrasError(error)) => {
                    trace!("Error: Invalid matcher extras: {:?}", error);
                    result.add_error(ValidationError::SchemaError(
                        SchemaError::InvalidMatcherExtras {
                            schema_index: schema_cursor.descendant_index(),
                            error,
                        },
                    ));
                    return result;
                }
                Err(error) => {
                    trace!("Error: Matcher error: {:?}", error);
                    result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                        error,
                        schema_index: schema_cursor.descendant_index(),
                    }));
                    return result;
                }
            }
        }
        None => {
            trace!("No schema node found; attempting to evaluate as text pair.");

            let input_child_count = input_cursor.node().child_count();

            if let Some(error) =
                compare_node_children_lengths(&schema_cursor, &input_cursor, got_eof)
            {
                trace!("Error: Children length mismatch");
                result.add_error(error);
                return result;
            }

            if is_textual_node(&input_cursor.node()) && is_textual_node(&schema_cursor.node()) {
                trace!("Both nodes are textual, validating directly");
                // Both are textual nodes, validate them directly
                return validate_textual_nodes(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                    false,
                );
            }

            {
                let mut input_cursor = input_cursor.clone();
                let mut schema_cursor = schema_cursor.clone();

                // Move cursors to first child
                if !input_cursor.goto_first_child() || !schema_cursor.goto_first_child() {
                    trace!("No children to validate");
                    // No children to validate
                    result.sync_cursor_pos(&input_cursor, &schema_cursor);

                    return result;
                }

                // Recursively validate all of the level. If they weren't textual, that
                // means they're textual containers.
                let child_result = validate_textual_container_children(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                    input_child_count,
                );
                result.join_other_result(&child_result);
            }

            // Now that we're back at the parent, and if there are more siblings, recurse
            if input_cursor.goto_next_sibling() && schema_cursor.goto_next_sibling() {
                let recurse_result = validate_text_vs_text(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                );
                result.join_other_result(&recurse_result);
            }

            result
        }
    }
}

/// Check if there are too many matchers in the schema.
///
/// If we encounter a matcher that is invalid, error. If we encounter a literal
/// matcher, don't count it towards the final tally.
///
/// Returns the number of matchers if there are more than one, otherwise None.
fn matcher_count_in_children(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<usize, MatcherError> {
    let mut schema_cursor = schema_cursor.clone();
    schema_cursor.goto_first_child();

    let mut matcher_count = 0;

    loop {
        let schema_cursor_kind = schema_cursor.node().kind();
        if schema_cursor_kind != "code_span" {
            if !schema_cursor.goto_next_sibling() {
                break;
            }
            continue;
        }

        let Some((matcher_node, suffix_node)) = get_node_and_next_node(&schema_cursor) else {
            if !schema_cursor.goto_next_sibling() {
                break;
            }
            continue;
        };

        match try_from_code_and_text_node(matcher_node, suffix_node, schema_str) {
            Ok(_) => {
                matcher_count += 1;
            }
            Err(MatcherError::WasLiteralCode) => {
                if !schema_cursor.goto_next_sibling() {
                    break;
                }
            }
            Err(error) => return Err(error),
        }

        if !schema_cursor.goto_next_sibling() {
            break;
        }
    }

    Ok(matcher_count)
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
/// * `strip_extras` - Whether to strip matcher extras from the start of the
///   input string. For example, if the input string is "{1,2}! test", when
///   comparing, strip away until after the first space, only comparing "test".
fn validate_textual_nodes(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
    strip_extras: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);
    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

    trace!(
        "Validating textual nodes: input={:?}, schema={:?}, strip_extras={}",
        input_cursor.node().kind(),
        schema_cursor.node().kind(),
        strip_extras
    );

    // Check node kind first
    if let Some(error) = compare_node_kinds(&schema_cursor, &input_cursor, schema_str, input_str) {
        trace!("Error: Node kind mismatch");
        result.add_error(error);
        return result;
    }

    // Let's drill down into the type of textual node until we get a raw text.
    // For example, (bold (emphasis (text))) -> (text)

    if both_are_text_nodes(&input_cursor.node(), &schema_cursor.node()) {
        // Then compare text contents
        if let Some(error) = compare_text_contents(
            schema_str,
            input_str,
            &schema_cursor,
            &input_cursor,
            got_eof,
            strip_extras,
        ) {
            trace!("Error: Text content mismatch");
            result.add_error(error);
        } else {
            trace!("Text content matched, moving to next sibling");
            // Otherwise tick the cursors forward if successful!
            schema_cursor.goto_next_sibling();
            input_cursor.goto_next_sibling();

            result.sync_cursor_pos(&input_cursor, &schema_cursor);
        }

        result
    } else {
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        validate_textual_nodes(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            got_eof,
            strip_extras,
        )
    }
}

/// Validate children of text containers. This recurses into the children of two
/// text containers, and processes all of their siblings.
///
/// This is recursive. For structures like (bold (emphasis (text)) (text)) VS (bold (emphasis (text)) (text)) it will validate each chunk.
///
/// The schema and input cursors are advanced to the first child of the current
/// node, and then the siblings are walked in lock step checking each textual
/// node against the other.
fn validate_textual_container_children(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
    input_child_count: usize,
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    trace!(
        "Validating textual container children, child_count={}",
        input_child_count
    );

    let mut i = 0;
    loop {
        let is_last_input_node = i == input_child_count - 1;

        let schema_child = schema_cursor.node();
        let input_child = input_cursor.node();

        trace!(
            "Validating child #{}, input={:?}, schema={:?}",
            i,
            input_child.kind(),
            schema_child.kind()
        );

        // Check if both are textual nodes
        if is_textual_node(&input_child) && is_textual_node(&schema_child) {
            trace!("Both children are textual nodes, delegating to validate_textual_nodes");

            // Use validate_textual_nodes instead of directly comparing
            let textual_result = validate_textual_nodes(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                got_eof && is_last_input_node,
                false,
            );
            result.join_other_result(&textual_result);
            if !result.errors.is_empty() {
                trace!("Error: Textual node validation failed");
                return result;
            }

            // Move cursors forward after successful validation
            result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);
        } else {
            // If not both textual, we need to recurse into them
            trace!(
                "Recursing into non-textual nodes of kind input={:?} and schema={:?}",
                input_cursor.node().kind(),
                schema_cursor.node().kind()
            );

            // They could be lists, or really anything
            let child_result = validate_node_vs_node(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                got_eof && is_last_input_node,
            );
            result.join_other_result(&child_result);
            if !result.errors.is_empty() {
                trace!("Error: Validation failed during recursion");
                return result;
            }
        }

        // Move to next siblings
        let has_next_input = input_cursor.goto_next_sibling();
        let has_next_schema = schema_cursor.goto_next_sibling();

        if !has_next_input || !has_next_schema {
            trace!(
                "Reached end of siblings (has_next_input={}, has_next_schema={})",
                has_next_input, has_next_schema
            );
            break;
        }

        i += 1;
    }

    trace!(
        "Completed validation of {} textual container children",
        i + 1
    );
    result
}

/// Create a new Matcher from two tree-sitter nodes and a schema string.
///
/// - The first node should be a code_span node containing the matcher pattern.
/// - The second node (optional) should be a text node containing extras.
fn try_from_code_and_text_node(
    matcher_node: tree_sitter::Node,
    suffix_node: Option<tree_sitter::Node>,
    schema_str: &str,
) -> Result<Matcher, MatcherError> {
    let matcher_text = matcher_node.utf8_text(schema_str.as_bytes()).map_err(|_| {
        MatcherError::MatcherInteriorRegexInvalid("Invalid UTF-8 in matcher node".to_string())
    })?;

    let suffix_text = suffix_node
        .map(|node| node.utf8_text(schema_str.as_bytes()).ok())
        .flatten();

    Matcher::try_from_pattern_and_suffix_str(matcher_text, suffix_text)
}

/// Validate a sequence of nodes that includes a matcher node against a text
/// node. This is used for when we have 1-3 nodes, where there may be a center
/// node that is a code node that is a matcher.
///
/// The schema cursor should point at:
/// Validate text using a matcher pattern from the schema.
///
/// Called by `validate_text_vs_text` when a matcher group is detected in the schema.
/// A matcher group consists of text-code-text nodes where the code contains a pattern.
///
/// The matcher can match against input text and optionally capture the matched value.
/// Supports prefix/suffix matching and various pattern types (regex, literal, etc.).
pub fn validate_matcher_vs_text<'a>(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
    matcher_group: (Option<Node<'a>>, (Matcher, Node<'a>), Option<Node<'a>>),
) -> ValidationResult {
    let mut result = ValidationResult::from_cursors(schema_cursor, input_cursor);

    // Mutable cursors that we can walk forward as we validate
    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

    // Destructure to make it easier to work with
    let (schema_prefix_node, (matcher, _matcher_node), schema_suffix_node) = matcher_group;

    trace!(
        "Validating matcher vs text: matcher_id={:?}, has_prefix={}, has_suffix={}",
        matcher.id(),
        schema_prefix_node.is_some(),
        schema_suffix_node.is_some()
    );

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
                trace!(
                    "Prefix mismatch: expected '{}', got '{}'",
                    schema_prefix_str, input_prefix_str
                );
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
                result.sync_cursor_pos(&input_cursor, &schema_cursor);

                return result;
            }

            trace!("Prefix matched successfully");
            input_byte_offset += schema_prefix_node.byte_range().len();
        } else if is_last_node(input_str, &input_cursor.node()) {
            // If we're waiting at the end, we can't validate the prefix yet
            let best_prefix_input_we_can_do = &input_str[input_byte_offset..];
            let best_prefix_length = best_prefix_input_we_can_do.len();
            let schema_prefix_partial = &schema_prefix_str[..best_prefix_length];

            if waiting_at_end(got_eof, input_str, &input_cursor) {
                trace!("Input prefix not long enough, but waiting at end of input");

                if schema_prefix_partial != best_prefix_input_we_can_do {
                    trace!(
                        "Prefix partial mismatch at end: expected '{}', got '{}'",
                        schema_prefix_partial, best_prefix_input_we_can_do
                    );
                    result.add_error(ValidationError::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: schema_cursor_at_prefix.descendant_index(),
                            input_index: input_node_descendant_index,
                            expected: schema_prefix_str.into(),
                            actual: best_prefix_input_we_can_do.into(),
                            kind: NodeContentMismatchKind::Prefix,
                        },
                    ));
                } else {
                    trace!("Prefix partial match successful, deferring full validation");
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

            result.sync_cursor_pos(&input_cursor, &schema_cursor);
            return result;
        }
    }

    // Don't validate after the prefix if there isn't enough content
    if input_byte_offset >= input_str.len() {
        if got_eof {
            let schema_prefix_str = schema_prefix_node
                .map(|node| &schema_str[node.byte_range()])
                .unwrap_or("");

            let best_prefix_input_we_can_do = &input_str[input_cursor.node().byte_range().start..];

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

        return result;
    }

    // All input that comes after the expected prefix
    let input_after_prefix =
        input_str[input_byte_offset..input_cursor.node().byte_range().end].to_string();

    if !got_eof && input_after_prefix.contains("`") {
        return result;
    } else {
        trace!(
            "xAttempting to match the input \"{}\"'s prefix, which is {}",
            input_cursor.node().utf8_text(input_str.as_bytes()).unwrap(),
            input_after_prefix
        );
    }

    // Actually perform the match for the matcher
    match matcher.match_str(&input_after_prefix) {
        Some(matched_str) => {
            trace!(
                "Matcher successfully matched input: '{}' (length={})",
                matched_str,
                matched_str.len()
            );

            input_byte_offset += matched_str.len();

            // Good match! Add the matched node to the matches (if it has an id)
            if let Some(id) = matcher.id() {
                trace!("Storing match for id '{}': '{}'", id, matched_str);
                result.set_match(id, json!(matched_str));
            } else {
                trace!("Matcher has no id, not storing match");
            }
        }
        None => {
            trace!(
                "Matcher did not match input string: pattern={}, input='{}'",
                matcher.pattern().to_string(),
                input_after_prefix
            );
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
            result.sync_cursor_pos(&input_cursor, &schema_cursor);

            return result;
        }
    }

    // Validate suffix if there is one
    if let Some(schema_suffix_node) = schema_suffix_node {
        trace!("Validating suffix");
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
                schema_suffix, input_suffix
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
        } else {
            trace!("Suffix matched successfully");
        }
    } else {
        trace!("No suffix to validate");
    }

    result.sync_cursor_pos(&input_cursor, &schema_cursor);

    result
}

#[cfg(test)]
mod tests {
    use super::validate_matcher_vs_text as validate_matcher_vs_text_original;
    use serde_json::json;
    use tree_sitter::TreeCursor;

    use crate::{
        helpers::node_print::PrettyPrint,
        mdschema::validator::{
            errors::*,
            matcher::matcher::MatcherError,
            node_walker::{
                ValidationResult,
                validators::text::{
                    extract_matcher_nodes, matcher_count_in_children, validate_text_vs_text,
                    validate_textual_container_children, validate_textual_nodes,
                },
            },
            ts_utils::parse_markdown,
            validator_state::NodePosPair,
        },
    };

    fn validate_matcher_vs_text<'a>(
        input_cursor: &TreeCursor,
        schema_cursor: &TreeCursor,
        schema_str: &str,
        input_str: &str,
        got_eof: bool,
    ) -> ValidationResult {
        use super::try_from_code_and_text_node;

        match extract_matcher_nodes(&schema_cursor) {
            Some((prefix_node, matcher_node, suffix_node)) => {
                let matcher = try_from_code_and_text_node(matcher_node, suffix_node, schema_str)
                    .expect("test utility expects valid matcher");
                validate_matcher_vs_text_original(
                    input_cursor,
                    schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                    (prefix_node, (matcher, matcher_node), suffix_node),
                )
            }
            None => unreachable!(
                "this test utility is designed only for matchers and blows up for non matcher groups"
            ),
        }
    }

    #[test]
    fn test_matcher_count_in_children_no_matchers() {
        let schema_str = "Some *Literal* **Other**";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = schema_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = matcher_count_in_children(&schema_cursor, schema_str).unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_matcher_count_in_children_with_literal_matcher() {
        let schema_str = "Some *Literal* `**Other**`! `test:/est/`";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = schema_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = matcher_count_in_children(&schema_cursor, schema_str).unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_matcher_count_in_children_broken_matchers() {
        let schema_str = "Some *Literal* `**Other**` `test:/test/`{,} `foobar!!!test";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = schema_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result = matcher_count_in_children(&schema_cursor, schema_str);
        match result.as_ref().unwrap_err() {
            MatcherError::MatcherInteriorRegexInvalid(..) => {}
            _ => panic!("Unexpected error: {:?}", result),
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
                assert_eq!(*expected, ChildrenCount::SpecificCount(4)); // text, italic, text, strong
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
    fn test_try_from_code_and_text_node() {
        // Test successful matcher creation from nodes
        use super::try_from_code_and_text_node;
        use crate::mdschema::validator::ts_utils::new_markdown_parser;

        let schema_str = "`word:/\\w+/` suffix";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(schema_str, None).unwrap();
        let root = tree.root_node();
        let paragraph = root.child(0).unwrap();

        let mut cursor = paragraph.walk();
        cursor.goto_first_child(); // go to first child (text or code_span)

        // Find the code_span node
        let mut matcher_node = None;
        let mut suffix_node = None;

        for child in paragraph.children(&mut cursor) {
            if child.kind() == "code_span" {
                matcher_node = Some(child);
            } else if child.kind() == "text" && matcher_node.is_some() {
                suffix_node = Some(child);
            }
        }

        let matcher_node = matcher_node.expect("Should find code_span node");
        let matcher = try_from_code_and_text_node(matcher_node, suffix_node, schema_str).unwrap();

        assert_eq!(matcher.id(), Some("word"));
        assert_eq!(matcher.match_str("hello"), Some("hello"));
        assert_eq!(matcher.match_str("123"), Some("123"));
        assert_eq!(matcher.match_str("!@#"), None);
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

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_literal_codeblock_mismatch() {
        // Test that literal codeblock validation catches mismatches
        let schema_str = "Here is `test`! some text";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "Here is `different` some text";
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
                assert!(expected.contains("test"));
                assert!(actual.contains("different"));
            }
            _ => panic!("Expected a NodeContentMismatch error!"),
        }
    }

    #[test]
    fn test_validate_text_vs_text_with_matcher_and_more() {
        let schema_str = "`test:/hi there/` test *foo*";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "hi there test *foobar* ";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
                schema_index,
                input_index,
                expected,
                actual,
                ..
            }) => {
                assert_eq!(*schema_index, 2);
                assert_eq!(*input_index, 4);
                assert_eq!(*expected, ChildrenCount::SpecificCount(1));
                assert_eq!(*actual, 0);
            }
            _ => panic!(
                "Expected ChildrenLengthMismatch error, got: {:?}",
                result.errors[0]
            ),
        }
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_text_vs_text_with_literal_simple() {
        let schema_str = "`hi there`! test *foo*";
        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = "`hi there` test *foo* ";
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(
            result.errors.is_empty(),
            "Errors found: {:?}",
            result.errors
        );
        assert_eq!(result.value, json!({}));
    }

    #[test]
    fn test_validate_text_vs_text_with_literal_walks_correctly() {
        // Assert that when we run on:
        //
        // INPUT:
        // (document[0])
        // ├─ (paragraph[1])
        // │  ├─ (code_span[2])
        // │  │  └─ (text[3])
        // │  ├─ (text[4])
        //
        // SCHEMA
        // (document[0])
        // ├─ (paragraph[1])
        // │  ├─ (code_span[2])
        // │  │  └─ (text[3])
        // │  ├─ (text[4])
        //
        // That it validates and the indexes are correct.

        let schema_str = r#"`hi there`!"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = r#"`hi there` test"#;
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "paragraph");
        assert_eq!(schema_cursor.node().kind(), "paragraph");

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index,
                input_index,
                expected,
                actual,
                kind,
                ..
            }) => {
                assert_eq!(*schema_index, 4);
                assert_eq!(*input_index, 4);
                assert_eq!(expected, " ");
                assert_eq!(actual, " test");
                assert_eq!(*kind, NodeContentMismatchKind::Literal);
            }
            _ => panic!(
                "Expected NodeContentMismatch error, got: {:?}",
                result.errors[0]
            ),
        }
        assert_eq!(result.value, json!({}));
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 4))
    }

    #[test]
    fn test_walks_down_into_emphasis() {
        let schema_str = "*test*";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        // (document[0])
        // └─ (paragraph[1])
        //    └─ (emphasis[2])
        //       └─ (text[3])

        let input_str = "*test*";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        {
            let mut schema_cursor = schema_cursor.clone();
            let mut input_cursor = input_cursor.clone();
            schema_cursor.goto_first_child(); // paragraph -> emphasis
            input_cursor.goto_first_child(); // paragraph -> emphasis

            let textual_nodes_validation_result = validate_textual_nodes(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                true,  // got eof
                false, // don't strip extras; not dealing with post-matcher
            );
            assert_eq!(
                textual_nodes_validation_result.farthest_reached_pos(),
                NodePosPair::from_pos(3, 3)
            );

            let textual_nodes_validation_result = validate_textual_container_children(
                &input_cursor,
                &schema_cursor,
                schema_str,
                input_str,
                true, // got eof
                1,
            );
            assert_eq!(
                textual_nodes_validation_result.farthest_reached_pos(),
                NodePosPair::from_pos(3, 3)
            );
        }

        let text_vs_text_validation_result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);
        assert_eq!(
            text_vs_text_validation_result.farthest_reached_pos(),
            NodePosPair::from_pos(3, 3)
        )
    }

    #[test]
    fn test_validate_text_vs_text_with_literal_multi_line() {
        let schema_str = r#"
`hi there`! test *test*

# Test
"#;
        // (document[0])
        // ├─ (paragraph[1])
        // │  ├─ (code_span[2])
        // │  │  └─ (text[3])
        // │  ├─ (text[4])
        // │  └─ (emphasis[5])
        // │     └─ (text[6])
        // └─ (atx_heading[7])
        //    ├─ (atx_h1_marker[8])
        //    └─ (heading_content[9])
        //       └─ (text[10])

        let schema_tree = parse_markdown(schema_str).unwrap();

        let input_str = r#"
`hi there` test *test*

# Test"#;
        // (document[0])
        // ├─ (paragraph[1])
        // │  ├─ (code_span[2])
        // │  │  └─ (text[3])
        // │  ├─ (text[4])
        // │  └─ (emphasis[5])
        // │     └─ (text[6])
        // └─ (atx_heading[7])
        //    ├─ (atx_h1_marker[8])
        //    └─ (heading_content[9])
        //       └─ (text[10])

        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);
        // assert!(
        //     result.errors.is_empty(),
        //     "Errors found: {:?}",
        //     result.errors
        // );

        let (new_input_node_descendant_index, new_schema_node_descendant_index) =
            result.farthest_reached_pos().to_pos_tuple();

        // TODO: ensure these indexes are correct
        assert_eq!(
            new_input_node_descendant_index,
            // stops at end of first paragraph
            6
        );
        assert_eq!(
            new_schema_node_descendant_index,
            // stops at end of first paragraph
            6
        );
        // assert!(
        //     result.errors.is_empty(),
        //     "Errors found: {:?}",
        //     result.errors
        // );

        let errors = result.errors;
        let value = result.value;

        // assert_eq!(errors, vec![]);
        assert_eq!(value, json!({}));
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

        assert_eq!(errors, vec![]);
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

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({"test": "test"}));
    }

    #[test]
    fn test_validate_text_vs_text_header_content() {
        let schema_str = "# Test Wolf";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "# Test Wolf";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        // (document[0])
        // └─ (atx_heading[1])
        //    ├─ (atx_h1_marker[2])
        //    └─ (heading_content[3])
        //       └─ (text[4])

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();
        assert_eq!(input_cursor.node().kind(), "heading_content");
        assert_eq!(schema_cursor.node().kind(), "heading_content");

        let result = validate_text_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 4));
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_text_vs_text_header_content_and_matcher() {
        let schema_str = "# Test `name:/[a-zA-Z]+/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "# Test Wolf";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();
        // (document[0])
        // └─ (atx_heading[1])
        //    ├─ (atx_h1_marker[2])
        //    └─ (heading_content[3])
        //       └─ (text[4])

        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        input_cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();
        assert_eq!(input_cursor.node().kind(), "heading_content");
        assert_eq!(schema_cursor.node().kind(), "heading_content");

        let result = validate_text_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            true,
        );

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(result.farthest_reached_pos(), NodePosPair::from_pos(4, 5));
        assert_eq!(value, json!({"name": "Wolf"}));
    }

    #[test]
    fn test_validate_text_vs_text_with_incomplete_matcher() {
        let schema_str = "prefix `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "prefix `test:/te";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        let result = validate_text_vs_text(
            &mut input_cursor,
            &mut schema_cursor,
            schema_str,
            input_str,
            false, // we are allowed to have a broken matcher if it is the last com
        );

        let errors = result.errors.clone();
        let value = result.value.clone();

        assert_eq!(errors, vec![]);
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_not_long_enough() {
        let schema_str = "prefix that is longer than input `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        let input_str = "prefix";
        let input_tree = parse_markdown(input_str).unwrap();
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
    fn test_invalid_regex_error() {
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

        assert!(!result.errors.is_empty());
        match result.errors.first().unwrap() {
            ValidationError::SchemaError(SchemaError::MatcherError {
                error,
                schema_index,
            }) => {
                // Validate that we got the expected error fields
                assert!(schema_index > &0);

                match error {
                    MatcherError::MatcherInteriorRegexInvalid(_) => {
                        // Validate that we got the expected error message
                        assert!(error.to_string().contains("Invalid matcher interior regex"));
                    }
                    _ => panic!("Unexpected error type: {:?}", error),
                }
            }
            _ => panic!(
                "Unexpected error type: {:?}",
                result.errors.first().unwrap()
            ),
        }
    }

    #[test]
    fn test_validate_list_item_with_nested_matcher() {
        // Schema with a list item containing a matcher for "foo\d"
        let schema_str = r#"
- `test:/foo\d/`
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();

        // Input with a list item containing multiple lines and nested list items
        let input_str = r#"
- bar
  - foo1
  - foo2
 - baz
 "#;
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> tight_list
        input_cursor.goto_first_child(); // document -> tight_list
        assert_eq!(schema_cursor.node().kind(), "tight_list");
        assert_eq!(input_cursor.node().kind(), "tight_list");

        schema_cursor.goto_first_child(); // tight_list -> list_item
        input_cursor.goto_first_child(); // tight_list -> list_item
        assert_eq!(schema_cursor.node().kind(), "list_item");
        assert_eq!(input_cursor.node().kind(), "list_item");

        schema_cursor.goto_first_child(); // list_item -> list_marker
        input_cursor.goto_first_child(); // list_item -> list_marker

        schema_cursor.goto_next_sibling(); // list_marker -> paragraph
        input_cursor.goto_next_sibling(); // list_marker -> paragraph

        let result = validate_text_vs_text(
            &input_cursor,
            &schema_cursor,
            schema_str,
            input_str,
            true, // eof is true
        );

        // The test should fail with a NodeContentMismatch error
        assert_eq!(
            result.errors.len(),
            1,
            "Expected one error, got {}",
            result.errors.len()
        );
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                actual,
                kind,
                ..
            }) => {
                assert_eq!(expected, "^foo\\d");
                assert_eq!(actual, "bar");
                assert_eq!(*kind, NodeContentMismatchKind::Matcher);
            }
            e => panic!("Expected a NodeContentMismatch error! Got {:?}", e),
        }
    }

    #[test]
    fn test_validate_text_vs_text_multiple_matchers() {
        // The schema becomes a paragraph with multiple code nodes
        let schema_str = "`id:/test/` `id:/example/`";
        let input_str = "test example";

        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let result =
            validate_text_vs_text(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(!result.errors.is_empty());
        match result.errors.first().unwrap() {
            ValidationError::SchemaError(SchemaError::MultipleMatchersInNodeChildren {
                received,
                ..
            }) => {
                assert_eq!(*received, 2);
            }
            _ => panic!(
                "Expected MultipleMatchersInNodeChildren error but got: {:?}",
                result.errors.first().unwrap()
            ),
        }
    }
}
