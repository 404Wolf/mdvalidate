use log::debug;
use serde_json::{json, Value};
use tree_sitter::Node;

use crate::mdschema::validator::{
    errors::{Error, SchemaError, SchemaViolationError},
    matcher::Matcher,
    utils::is_last_node,
};

pub type NodeValidationResult = (Vec<Error>, Value);

/// Validate a text node against the schema text node.
///
/// This is a node that is just a simple literal text node. We validate that
/// the text content is identical.
pub fn validate_text_node<'b>(
    input_node: &Node<'b>,
    input_node_descendant_index: usize,
    schema_node: &Node<'b>,
    input_str: &'b str,
    schema_str: &'b str,
    eof: bool,
) -> NodeValidationResult {
    let mut errors = Vec::new();

    let schema_text = &schema_str[schema_node.byte_range()];
    let input_text = &input_str[input_node.byte_range()];

    debug!(
        "Comparing text: schema='{}' vs input='{}'",
        schema_text, input_text
    );

    if schema_text != input_text {
        debug!("Text mismatch found");
        errors.push(Error::SchemaViolation(
            SchemaViolationError::NodeContentMismatch(
                input_node_descendant_index,
                schema_text.into(),
            ),
        ));
    }

    if !eof && is_last_node(input_str, input_node) {
        debug!("Skipping error reporting, incomplete last node");
        (vec![], json!({}))
    } else {
        (errors, json!({}))
    }
}

/// Validate a matcher node against the children of a list input node.
///
/// This works by re-running the validation using validate_matcher_node on each input node in the
/// list.
pub fn validate_matcher_node_list<'b>(
    input_node: &Node<'b>,
    input_node_descendant_index: usize,
    schema_nodes: &[Node<'b>],
    input_str: &'b str,
    schema_str: &'b str,
    eof: bool,
) -> NodeValidationResult {
    let matcher = schema_nodes.iter().find(|n| n.kind() == "code_span");

    match matcher {
        None => {
            return (
                vec![Error::SchemaError(
                    SchemaError::NoMatcherInListNodeChildren(input_node_descendant_index),
                )],
                json!({}),
            );
        }
        Some(matcher) => {
            let mut errors = Vec::new();
            let mut matches_array = Vec::new();

            for child in input_node.children(&mut input_node.walk().clone()) {
                // TODO: reuse the cursor that we already have
                let (child_errors, child_matches) = validate_matcher_node(
                    &child.child(1).unwrap(),
                    input_node_descendant_index,
                    schema_nodes,
                    input_str,
                    schema_str,
                    eof,
                );
                errors.extend(child_errors);

                if let Some(obj) = child_matches.as_object() {
                    // For each match object, extract the first value and add it to our array
                    if let Some((_, value)) = obj.iter().next() {
                        // TODO: Could we have multiple?
                        matches_array.push(value.clone());
                    }
                }
            }

            let matcher_text = &schema_str[matcher.byte_range()];
            let matcher_obj = match Matcher::new(matcher_text) {
                Ok(m) => m,
                Err(_) => {
                    return (
                        vec![Error::SchemaError(
                            SchemaError::NoMatcherInListNodeChildren(input_node_descendant_index),
                        )],
                        json!({}),
                    );
                }
            };

            let mut matches = json!({});
            matches[matcher_obj.id()] = serde_json::Value::Array(matches_array);

            (errors, matches)
        }
    }
}

/// Validate a matcher node against the input node.
///
/// A matcher node looks like `id:/pattern/` in the schema.
///
/// Pass the parent of the matcher node, and the corresponding input node.
pub fn validate_matcher_node<'b>(
    input_node: &Node<'b>,
    input_node_descendant_index: usize,
    schema_nodes: &[Node<'b>],
    input_str: &'b str,
    schema_str: &'b str,
    eof: bool,
) -> NodeValidationResult {
    let is_incomplete = !eof && is_last_node(input_str, input_node);

    let mut errors = Vec::new();
    let mut matches = json!({});

    let code_nodes: Vec<_> = schema_nodes
        .iter()
        .filter(|n| n.kind() == "code_span")
        .collect();

    if code_nodes.len() > 1 {
        return (
            vec![Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    "Multiple matchers in single node".into(),
                ),
            )],
            matches,
        );
    }

    let code_node = code_nodes[0];
    let matcher_text = &schema_str[code_node.byte_range()];

    let matcher = match Matcher::new(matcher_text) {
        Ok(m) => m,
        Err(_) => {
            return (
                vec![Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        matcher_text.into(),
                    ),
                )],
                matches,
            );
        }
    };

    let schema_start = schema_nodes[0].byte_range().start;
    let matcher_start = code_node.byte_range().start - schema_start;
    let matcher_end = code_node.byte_range().end - schema_start;

    // Always validate prefix, even for incomplete nodes
    let prefix_schema = &schema_str[schema_start..schema_start + matcher_start];

    // Check if we have enough input to validate the prefix
    let input_has_full_prefix = input_node.byte_range().len() >= matcher_start;

    if input_has_full_prefix {
        let prefix_input = &input_str
            [input_node.byte_range().start..input_node.byte_range().start + matcher_start];

        if prefix_schema != prefix_input {
            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    prefix_schema.into(),
                ),
            ));

            return (errors, matches);
        }
    }

    // Skip matcher and suffix validation if node is incomplete
    if is_incomplete {
        debug!("Skipping matcher and suffix validation - incomplete node");
        return (errors, matches);
    }

    let input_start = input_node.byte_range().start + matcher_start;
    let input_to_match = &input_str[input_start..];

    match matcher.match_str(input_to_match) {
        Some(matched_str) => {
            // Validate suffix
            let schema_end = schema_nodes.last().unwrap().byte_range().end;
            let suffix_schema = &schema_str[schema_start + matcher_end..schema_end];
            let suffix_start = input_start + matched_str.len();
            let input_end = input_node.byte_range().end;

            // Ensure suffix_start doesn't exceed input_end
            if suffix_start > input_end {
                // out of bounds
                errors.push(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        suffix_schema.into(),
                    ),
                ));
            } else {
                let suffix_input = &input_str[suffix_start..input_end];

                if suffix_schema != suffix_input {
                    errors.push(Error::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch(
                            input_node_descendant_index,
                            suffix_schema.into(),
                        ),
                    ));
                }
            }
            // Good match! Add the matched node to the matches
            matches[matcher.id()] = json!(matched_str);
        }
        None => {
            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    matcher_text.into(),
                ),
            ));
        }
    };

    // If this is the last node, don't validate it if we haven't reached EOF,
    // since the matcher might be incomplete.
    if !eof && is_incomplete {
        (vec![], matches)
    } else {
        (errors, matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::utils::new_markdown_parser;
    use tree_sitter::Node;

    /// Helper function to create parsers and nodes for text validation tests
    fn get_text_validator(schema: &str, input: &str, eof: bool) -> (Vec<Error>, Value) {
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_node = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input node");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let schema_node = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema node");

        validate_text_node(&input_node, 0, &schema_node, input, schema, eof)
    }

    fn get_matcher_validator(schema: &str, input: &str, eof: bool) -> (Vec<Error>, Value) {
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_node = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input node");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema root child")
            .children(&mut schema_cursor)
            .collect();

        validate_matcher_node(&input_node, 0, &schema_nodes, input, schema, eof)
    }

    fn get_list_matcher_validator(
        schema: &str,
        input: &str,
        eof: bool,
    ) -> (Vec<Error>, serde_json::Value) {
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");

        let input_node = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input node");
        assert_eq!(input_node.kind(), "tight_list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema root child")
            .children(&mut schema_cursor)
            .collect();
        // We want the schema node to be the matcher node inside the first list item in the schema
        assert_eq!(schema_nodes[0].kind(), "code_span");

        let (errors, matches) =
            validate_matcher_node_list(&input_node, 0, &schema_nodes, input, schema, eof);
        return (
            errors,
            serde_json::Value::Array(matches.get("test").unwrap().as_array().unwrap().clone()),
        );
    }

    #[test]
    fn test_different_text_content_nodes_mismatch() {
        let schema = "Hello world";
        let input = "Hello there";

        let (errors, _) = get_text_validator(schema, input, true);

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, expected)) => {
                assert_eq!(expected, "Hello world");
            }
            _ => panic!("Unexpected error type"),
        }
    }

    #[test]
    fn test_same_text_content_nodes_match() {
        let schema = "Hello world";
        let input = "Hello world";

        let (errors, _) = get_text_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_with_prefix_and_suffix() {
        let schema = "Hello `id:/foo/` world";
        let input = "Hello foo world";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_with_regex() {
        let schema = "Value: `num:/[0-9]+/`";
        let input = "Value: 12345";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_only_prefix() {
        let schema = "Start `id:/test/`";
        let input = "Start test";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_only_suffix() {
        let schema = "`id:/test/` end";
        let input = "test end";

        let (errors, matches) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        println!("matches: {:?}", matches);
        assert_eq!(matches.as_object().unwrap().len(), 1);
        assert_eq!(
            matches
                .as_object()
                .unwrap()
                .get("id")
                .unwrap()
                .as_str()
                .unwrap(),
            "test"
        );
    }

    #[test]
    fn test_validate_matcher_no_prefix_or_suffix() {
        let schema = "`id:/test/`";
        let input = "test";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_fails_on_prefix_mismatch() {
        let schema = "Hello `id:/foo/` world";
        let input = "Goodbye foo world";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, expected)) => {
                assert_eq!(expected, "Hello ");
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_validate_matcher_list_expects_errors_on_pattern_mismatch() {
        let schema = "`test:/[0-9]/`";
        let input = "- 1\n- a\n- 3\n- b\n- 5";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(!errors.is_empty(), "Expected errors but got none");
        assert_eq!(errors.len(), 2, "Expected 2 errors for 'a' and 'b'");

        // Verify that valid matches were still captured
        let matches_obj = matches.as_object().unwrap();
        assert!(matches_obj.contains_key("num"));
    }

    #[test]
    fn test_validate_matcher_list_for_simple_digit_pattern() {
        let schema = "`test:/[0-9]/`";
        let input = "- 1\n- 2\n- 3\n- 4\n- 5";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 5);
        for i in 0..5 {
            assert_eq!(matches_arr[i].as_str().unwrap(), (i + 1).to_string());
        }
    }
}
