use log::trace;
use serde_json::json;
use tracing::instrument;
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::{Error, NodeContentMismatchKind, SchemaError, SchemaViolationError},
    matcher::{extract_text_matcher, get_everything_after_special_chars, ExtractorError, Matcher},
    node_walker::ValidationResult,
    utils::{is_last_node, waiting_at_end},
};

/// Validate a matcher node against a text node.
///
/// The schema cursor should point at:
/// - A text node, followed by a code node, maybe followed by a text node
/// - A code node, maybe followed by a text node
/// - A code node only
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_matcher_vs_text(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut matches = json!({});
    let mut errors = Vec::new();

    // Mutable cursors that we can walk forward as we validate
    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

    // How far along we've validated the input. We'll update this as we go
    let mut input_byte_offset = input_cursor.node().byte_range().start;

    let schema_nodes = schema_cursor
        .node()
        .children(&mut schema_cursor.clone())
        .collect::<Vec<Node>>();

    // Descendant index of the input node, specifically the paragraph (not the interior text)
    let input_node_descendant_index = input_cursor.descendant_index();
    input_cursor.goto_first_child();

    //      Hello [`name:/\w+/`] World
    //      |---|  |---------|   |---|
    //        ^         ^          ^
    //        |         |          |
    //        |         |    schema_suffix_node
    //        |         |
    //        |    schema_matcher_node
    //        |
    //    schema_matcher_node

    let (schema_prefix_node, _, schema_suffix_node) = match extract_matcher_nodes(&schema_nodes) {
        Some(t) => t,
        None => {
            // TODO: add test
            errors.push(Error::SchemaError(SchemaError::MissingMatcher {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
            }));

            return (matches, errors);
        }
    };

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
    if let Some(prefix_node) = schema_prefix_node {
        trace!("Validating prefix before matcher");

        let schema_prefix_str = &schema_str[prefix_node.byte_range()];
        let schema_prefix_len = schema_prefix_str.len();
        let input_potential_prefix_str = &input_str[input_byte_offset..];

        // Check that the input extends enough that we can cover the full prefix.
        if input_potential_prefix_str.len() >= schema_prefix_len {
            // Note we define prefix_input here because we first must make sure there is enough of it!
            let prefix_input = &input_str[input_byte_offset..input_byte_offset + schema_prefix_len];

            // Do the actual prefix comparison
            if schema_prefix_str != prefix_input {
                errors.push(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor_at_prefix.descendant_index(),
                        input_index: input_node_descendant_index,
                        expected: schema_prefix_str.into(),
                        actual: prefix_input.into(),
                        kind: NodeContentMismatchKind::Prefix,
                    },
                ));

                // If prefix validation fails don't try to validate further.
                // TODO: In the future we could attempt to validate further anyway!
                return (matches, errors);
            }
        } else if is_last_node(input_str, &input_cursor.node()) {
            // If we're waiting at the end, we can't validate the prefix yet
            let best_prefix_input_we_can_do = &input_str[input_byte_offset..];
            let best_prefix_length = best_prefix_input_we_can_do.len();
            let schema_prefix_partial = &schema_prefix_str[..best_prefix_length];

            if waiting_at_end(got_eof, input_str, &input_cursor) {
                trace!("Input prefix not long enough, but waiting at end of input");

                if schema_prefix_partial != best_prefix_input_we_can_do {
                    errors.push(Error::SchemaViolation(
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

                errors.push(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor_at_prefix.descendant_index(),
                        input_index: input_node_descendant_index,
                        expected: schema_prefix_str.into(),
                        actual: best_prefix_input_we_can_do.into(),
                        kind: NodeContentMismatchKind::Prefix,
                    },
                ));
            }

            return (matches, errors);
        }

        // Move the input cursor forward by the amount of prefix bytes that
        // we just validated
        input_byte_offset += schema_prefix_len;

    }

    // All input that comes after the expected prefix
    let input_after_prefix = input_str[input_byte_offset..].to_string();

    let matcher =
        match extract_text_matcher_into_schema_err(&schema_cursor, &input_cursor, schema_str) {
            Ok(m) => m,
            Err(e) => {
                trace!("Error extracting matcher: {:?}", e);
                errors.push(e);
                return (matches, errors);
            }
        };

    // If the matcher is for a ruler, we should expect the entire input node to be a ruler
    if matcher.is_ruler() {
        trace!("Matcher is for a ruler, validating node type");

        if input_cursor.node().kind() != "thematic_break" {
            trace!("Input node is not a ruler, reporting type mismatch error");

            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: "thematic_break".into(),
                    actual: input_cursor.node().kind().into(),
                },
            ));
            return (matches, errors);
        } else {
            // It's a ruler, no further validation needed
            return (matches, errors);
        }
    }

    // Actually perform the match for the matcher
    match matcher.match_str(&input_after_prefix) {
        Some(matched_str) => {
            trace!("Matcher matched input string: {}", matched_str);

            input_byte_offset += matched_str.len();

            // Good match! Add the matched node to the matches (if it has an id)
            if let Some(id) = matcher.id() {
                matches[id] = json!(matched_str);
            }
        }
        None => {
            trace!("Matcher did not match input string, reporting mismatch error");

            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: matcher.pattern().to_string(),
                    actual: input_after_prefix.into(),
                    kind: NodeContentMismatchKind::Matcher,
                },
            ));

            // TODO: should we validate further when we fail to match the matcher?
            return (matches, errors)
        }
    }

    // Validate suffix if there is one
    if schema_suffix_node.is_some() {
        schema_cursor.goto_next_sibling(); // code_span -> text
        debug_assert_eq!(schema_cursor.node().kind(), "text");

        // Everything that comes after the matcher
        let schema_suffix = {
            let text_node_after_code_node_str_contents =
                &schema_str[schema_cursor.node().byte_range()];
            // All text after the matcher node and maybe the text node right after it ("extras")
            get_everything_after_special_chars(text_node_after_code_node_str_contents).unwrap()
        };

        let input_suffix = &input_str[input_byte_offset..];

        if schema_suffix != input_suffix {
            trace!(
                "Suffix mismatch: expected '{}', got '{}'",
                schema_suffix,
                input_suffix
            );

            errors.push(Error::SchemaViolation(
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

    (matches, errors)
}

/// Given a list of nodes of variable length, return the index of the matcher
/// node, whether there is a prefix node, and whether there is a suffix node.
///
/// The matcher node is the first code node in the list, the prefix node is a
/// potential text node that comes before it, and the suffix node is a potential
/// node that comes at the end.
///
/// Returns the prefix node, matcher node, and suffix node. There should always
/// be a matcher node. If there is no matcher node, the entire Option is None.
fn extract_matcher_nodes<'a>(
    schema_nodes: &'a [Node<'a>],
) -> Option<(Option<Node<'a>>, Node<'a>, Option<&'a Node<'a>>)> {
    let mut has_prefix = true;
    let matcher_node_index = if schema_nodes.get(0)?.kind() == "code_span" {
        has_prefix = false;
        0
    } else if schema_nodes.len() > 1
        && schema_nodes.get(0)?.kind() == "text"
        && schema_nodes.get(1)?.kind() == "code_span"
    {
        1
    } else {
        // No valid matcher found
        return None;
    };

    let prefix_node = if has_prefix {
        Some(schema_nodes[0].clone())
    } else {
        None
    };

    let matcher_node = &schema_nodes[matcher_node_index];

    let suffix_node = {
        let after_matcher_index = matcher_node_index + 1;
        if after_matcher_index < schema_nodes.len()
            && schema_nodes
                .get(after_matcher_index)
                .map(|n| n.kind() == "text")
                .unwrap_or(false)
        {
            Some(&schema_nodes[after_matcher_index])
        } else {
            None
        }
    };

    Some((prefix_node, matcher_node.clone(), suffix_node))
}

/// Extracts a text matcher from the schema cursor and converts any errors to schema errors.
///
/// Returns `Some(Matcher)` if extraction succeeds, or `None` if an error occurs.
/// Errors are added to the error vector.
fn extract_text_matcher_into_schema_err(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<Matcher, Error> {
    match extract_text_matcher(schema_cursor, schema_str) {
        Ok(m) => Ok(m),
        Err(ExtractorError::MatcherError(e)) => {
            Err(Error::SchemaError(SchemaError::MatcherError {
                error: e,
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
            }))
        }
        Err(ExtractorError::UTF8Error(_)) => Err(Error::SchemaError(SchemaError::UTF8Error {
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
        })),
        Err(ExtractorError::InvariantError) => unreachable!("we should know it's a code node"),
    }
}

#[cfg(test)]
mod tests {
    use std::panic;

    use crate::mdschema::validator::{
        errors::{Error, NodeContentMismatchKind, SchemaViolationError},
        node_walker::matcher_vs_text::extract_matcher_nodes,
        utils::parse_markdown,
    };

    use super::validate_matcher_vs_text;
    use serde_json::json;

    #[test]
    fn test_extract_matcher_nodes_code_span_only() {
        let schema = "`name:/\\w+/`";
        let schema_tree = parse_markdown(schema).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<_>>();

        let result = extract_matcher_nodes(&nodes);
        assert!(result.is_some());
        let (prefix, matcher, suffix) = result.unwrap();
        assert!(prefix.is_none());
        assert_eq!(matcher.kind(), "code_span");
        assert!(suffix.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_with_prefix_only() {
        let schema = "Hello `name:/\\w+/`";
        let schema_tree = parse_markdown(schema).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<_>>();

        let result = extract_matcher_nodes(&nodes);
        assert!(result.is_some());
        let (prefix, matcher, suffix) = result.unwrap();
        assert!(prefix.is_some());
        assert_eq!(prefix.unwrap().kind(), "text");
        assert_eq!(matcher.kind(), "code_span");
        assert!(suffix.is_none());
    }

    #[test]
    fn test_extract_matcher_nodes_with_prefix_and_suffix() {
        let schema = "Hello `name:/\\w+/`!";
        let schema_tree = parse_markdown(schema).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<_>>();

        let result = extract_matcher_nodes(&nodes);
        assert!(result.is_some());
        let (prefix, matcher, suffix) = result.unwrap();
        assert!(prefix.is_some());
        assert_eq!(prefix.unwrap().kind(), "text");
        assert_eq!(matcher.kind(), "code_span");
        assert!(suffix.is_some());
        assert_eq!(suffix.unwrap().kind(), "text");
    }

    #[test]
    fn test_extract_matcher_nodes_no_matcher_code_node() {
        let schema = "Hello world!";
        let schema_tree = parse_markdown(schema).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<_>>();

        let result = extract_matcher_nodes(&nodes);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_matcher_nodes_empty_list() {
        let nodes: Vec<tree_sitter::Node> = vec![];
        let result = super::extract_matcher_nodes(&nodes);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_matcher_nodes_no_code_span() {
        let schema = "Just plain text";
        let schema_tree = parse_markdown(schema).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> paragraph

        let nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<_>>();

        let result = super::extract_matcher_nodes(&nodes);
        assert_eq!(result, None);
    }
    #[test]
    fn test_validate_matcher_vs_text_with_no_prefix_or_suffix() {
        let schema = "`name:/\\w+/`";
        let input = "Wolf";

        let schema_tree = parse_markdown(schema).unwrap();
        let input_tree = parse_markdown(input).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, true);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(matches, json!({"name": "Wolf"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_no_suffix() {
        let schema = "Hello `name:/\\w+/`";
        let input = "Hello Wolf";

        let schema_tree = parse_markdown(schema).unwrap();
        let input_tree = parse_markdown(input).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, true);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(matches, json!({"name": "Wolf"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_prefix_and_suffix() {
        let schema = "# Hello `name:/\\w+/`!";
        let input = "# Hello Wolf!";

        let schema_tree = parse_markdown(schema).unwrap();
        let input_tree = parse_markdown(input).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the heading node
        schema_cursor.goto_first_child(); // document -> atx_heading
        input_cursor.goto_first_child(); // document -> atx_heading
        assert_eq!(schema_cursor.node().kind(), "atx_heading");
        assert_eq!(input_cursor.node().kind(), "atx_heading");
        schema_cursor.goto_first_child(); // atx_heading -> atx_h1_marker
        input_cursor.goto_first_child(); // atx_heading -> atx_h1_marker
        assert_eq!(schema_cursor.node().kind(), "atx_h1_marker");
        assert_eq!(input_cursor.node().kind(), "atx_h1_marker");
        schema_cursor.goto_next_sibling(); // atx_heading -> heading_content
        input_cursor.goto_next_sibling(); // atx_heading -> heading_content
        assert_eq!(schema_cursor.node().kind(), "heading_content");
        assert_eq!(input_cursor.node().kind(), "heading_content");
        // Now we're set. We need to make sure we call it once we're at a
        // "content" node (e.g. text, heading_content, etc)

        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, true);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(matches, json!({"name": "Wolf"}));
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_not_long_enough() {
        // (document (paragraph (text) (code_span (text)) (text)))
        let schema = r#"Hello `name:/\w+/`!"#;

        // (document (paragraph (text)))
        let input = "Hell Wolf";

        let schema_tree = parse_markdown(schema).unwrap();
        let input_tree = parse_markdown(input).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, true);

        assert!(matches.as_object().unwrap().is_empty());

        assert_eq!(errors.len(), 1);
        let prefix_len = 6; // "Hello " is 6 characters

        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index,
                input_index,
                expected,
                actual,
                kind,
            }) => {
                assert_eq!(*schema_index, 2); // document -> paragraph -> text (the prefix)
                assert_eq!(*input_index, 1); // document -> text
                assert_eq!(expected, "Hello ");
                assert_eq!(actual, &input[..prefix_len]); // It eats 6 characters forward
                                                          // (as far as it can into the input)
                assert_eq!(*kind, NodeContentMismatchKind::Prefix);
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_good_so_far() {
        // (document (paragraph (text) (code_span (text)) (text)))
        let schema = "Hello `name:/\\w+/`!";

        // (document (paragraph (text)))
        let input = "Hell"; // partial state, but good so far

        let schema_tree = parse_markdown(schema).unwrap();
        let input_tree = parse_markdown(input).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, false);
        // But no errors or matches if we haven't reached EOF
        assert!(matches.as_object().unwrap().is_empty());
        assert!(errors.is_empty(), "Errors found: {:?}", errors);

        // When we reach EOF with partial input, we should get an error
        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, true);
        assert!(matches.as_object().unwrap().is_empty());
        assert_eq!(errors.len(), 1);

        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index,
                input_index,
                expected,
                actual,
                kind,
            }) => {
                assert_eq!(*schema_index, 2);
                assert_eq!(*input_index, 1);
                assert_eq!(expected, "Hello ");
                assert_eq!(actual, "Hell");
                assert_eq!(*kind, NodeContentMismatchKind::Prefix);
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_partial_but_bad_prefix() {
        // (document (paragraph (text) (code_span (text)) (text)))
        let schema = "Hello `name:/\\w+/`!";

        // (document (paragraph (text)))
        let input = "Badd"; // partial state, but already bad!

        let schema_tree = parse_markdown(schema).unwrap();
        let input_tree = parse_markdown(input).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        let (matches, errors) =
            // Even though eof=false, we still get an error, since we are already giving a bad prefix
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, false);
        assert!(matches.as_object().unwrap().is_empty());
        assert_eq!(errors.len(), 1);

        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index,
                input_index,
                expected,
                actual,
                kind,
            }) => {
                assert_eq!(*schema_index, 2);
                assert_eq!(*input_index, 1);
                assert_eq!(expected, "Hello ");
                assert_eq!(actual, "Badd");
                assert_eq!(*kind, NodeContentMismatchKind::Prefix);
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_validate_matcher_vs_text_with_input_prefix_empty() {
        // (document (paragraph (text) (code_span (text)) (text)))
        let schema = "Hello `name:/\\w+/`!";
        // (document)
        let input = ""; // empty input

        let schema_tree = parse_markdown(schema).unwrap();
        let input_tree = parse_markdown(input).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the paragraph node
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> paragraph

        // If we've got the EOF we didn't receive enough text for it to be correct
        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, true);
        assert!(matches.as_object().unwrap().is_empty());
        assert_eq!(errors.len(), 1);

        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index,
                input_index,
                expected,
                actual,
                kind,
            }) => {
                assert_eq!(*schema_index, 2);
                assert_eq!(*input_index, 0); // there is no text node!
                assert_eq!(expected, "Hello ");
                assert_eq!(actual, "");
                assert_eq!(*kind, NodeContentMismatchKind::Prefix);
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_validate_matcher_vs_text_with_ruler() {
        let schema = "`ruler`";
        let input = "---";

        let schema_tree = parse_markdown(schema).unwrap();
        let input_tree = parse_markdown(input).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the thematic_break node for input, paragraph for schema
        schema_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // document -> thematic_break
        assert_eq!(schema_cursor.node().kind(), "paragraph");
        assert_eq!(input_cursor.node().kind(), "thematic_break");

        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no errors, but got: {:?}",
            errors
        );
        assert!(matches.as_object().unwrap().is_empty());

        let bad_input = "Hello";
        let input_tree = parse_markdown(bad_input).unwrap();
        let mut input_cursor = input_tree.walk();
        input_cursor.goto_first_child(); // document -> paragraph
        input_cursor.goto_first_child(); // paragraph -> text

        let (matches, errors) =
            validate_matcher_vs_text(&input_cursor, &schema_cursor, schema, bad_input, true);
        assert_eq!(errors.len(), 1, "Expected 1 error, but got: {:?}", errors);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeTypeMismatch {
                schema_index,
                input_index,
                expected,
                actual,
            }) => {
                assert_eq!(*schema_index, 2);
                assert_eq!(*input_index, 2);
                assert_eq!(expected, "thematic_break");
                assert_eq!(actual, "text");
            }
            _ => panic!("Expected NodeTypeMismatch error"),
        }
        assert!(matches.as_object().unwrap().is_empty());
    }
}
