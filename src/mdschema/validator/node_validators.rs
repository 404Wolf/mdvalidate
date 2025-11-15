use log::debug;
use tree_sitter::Node;

use crate::mdschema::{
    reports::errors::{Error, SchemaViolationError},
    validator::matcher::Matcher,
};

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
    initial_input_node: &Node<'b>,
) -> Vec<Error> {
    debug!("Validating text node content");

    if (input_node.byte_range().end == initial_input_node.byte_range().end) && !eof {
        // Incomplete text node, skip validation for now
        debug!("Skipping text validation - incomplete node at EOF");
        return Vec::new();
    }

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

    errors
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
    initial_input_node: &Node<'b>,
) -> Vec<Error> {
    let is_incomplete =
        (input_node.byte_range().end == initial_input_node.byte_range().end) && !eof;

    debug!(
        "validate_matcher_node: input_node range={:?}, input_text='{}', eof={}, is_incomplete={}",
        input_node.byte_range(),
        &input_str[input_node.byte_range()],
        eof,
        is_incomplete
    );

    let mut errors = Vec::new();

    let code_nodes: Vec<_> = schema_nodes
        .iter()
        .filter(|n| n.kind() == "code_span")
        .collect();

    if code_nodes.len() > 1 {
        return vec![Error::SchemaViolation(
            SchemaViolationError::NodeContentMismatch(
                input_node_descendant_index,
                "Multiple matchers in single node".into(),
            ),
        )];
    }

    let code_node = code_nodes[0];
    let matcher_text = &schema_str[code_node.byte_range()];

    let matcher = match Matcher::new(matcher_text) {
        Ok(m) => m,
        Err(_) => {
            return vec![Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    matcher_text.into(),
                ),
            )];
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
            return errors;
        }
    }

    // Skip matcher and suffix validation if node is incomplete
    if is_incomplete {
        debug!("Skipping matcher and suffix validation - incomplete node");
        return errors;
    }

    let input_start = input_node.byte_range().start + matcher_start;
    let input_to_match = &input_str[input_start..];

    // If the matcher is for a ruler, we should expect the entire input node to be a ruler
    if matcher.is_ruler() {
        if input_node.kind() != "thematic_break" {
            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch(
                    input_node_descendant_index,
                    input_node_descendant_index, // should be the same as the schema's.
                                                 // TODO: is this really true though?
                ),
            ));
            return errors;
        } else {
            // It's a ruler, no further validation needed
            return errors;
        }
    }

    // If this is the last node, don't validate it if we haven't reached EOF,
    // since the matcher might be incomplete.
    match matcher.match_str(input_to_match) {
        Some(matched_str) => {
            // Validate suffix
            let schema_end = schema_nodes.last().unwrap().byte_range().end;
            let suffix_schema = &schema_str[schema_start + matcher_end..schema_end];
            let suffix_start = input_start + matched_str.len();
            let input_end = input_node.byte_range().end;

            // Ensure suffix_start doesn't exceed input_end
            if suffix_start > input_end {
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

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::utils::new_markdown_parser;

    #[test]
    fn test_different_text_content_nodes_mismatch() {
        let schema = "Hello world";
        let input = "Hello there";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node().child(0).unwrap();

        let errors = validate_text_node(
            &input_node,
            0, // placeholder
            &schema_node,
            input,
            schema,
            true,
            &input_node,
        );

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

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node().child(0).unwrap();

        let errors = validate_text_node(
            &input_node,
            0, // placeholder
            &schema_node,
            input,
            schema,
            true,
            &input_node,
        );

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

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        let input_root = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .unwrap()
            .children(&mut schema_cursor)
            .collect();

        let errors = validate_matcher_node(
            &input_node,
            0,
            &schema_nodes,
            input,
            schema,
            true,
            &input_root,
        );

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

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        let input_root = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .unwrap()
            .children(&mut schema_cursor)
            .collect();

        let errors = validate_matcher_node(
            &input_node,
            0,
            &schema_nodes,
            input,
            schema,
            true,
            &input_root,
        );

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

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        let input_root = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .unwrap()
            .children(&mut schema_cursor)
            .collect();

        let errors = validate_matcher_node(
            &input_node,
            0,
            &schema_nodes,
            input,
            schema,
            true,
            &input_root,
        );

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

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        let input_root = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .unwrap()
            .children(&mut schema_cursor)
            .collect();

        let errors = validate_matcher_node(
            &input_node,
            0,
            &schema_nodes,
            input,
            schema,
            true,
            &input_root,
        );

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_no_prefix_or_suffix() {
        let schema = "`id:/test/`";
        let input = "test";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        let input_root = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .unwrap()
            .children(&mut schema_cursor)
            .collect();

        let errors = validate_matcher_node(
            &input_node,
            0,
            &schema_nodes,
            input,
            schema,
            true,
            &input_root,
        );

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

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        let input_root = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .unwrap()
            .children(&mut schema_cursor)
            .collect();

        let errors = validate_matcher_node(
            &input_node,
            0,
            &schema_nodes,
            input,
            schema,
            true,
            &input_root,
        );

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, expected)) => {
                assert_eq!(expected, "Hello ");
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_ruler() {
        let schema = "`ruler`";
        let input = "---";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        let input_root = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .unwrap()
            .children(&mut schema_cursor)
            .collect();

        let errors = validate_matcher_node(
            &input_node,
            0,
            &schema_nodes,
            input,
            schema,
            true,
            &input_root,
        );

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }
}
