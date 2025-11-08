use core::panic;

use crate::mdschema::{
    reports::errors::{Error, SchemaViolationError},
    validator::utils::node_to_str,
};
use log::{debug, warn};
use tree_sitter::{Node, TreeCursor};

/// A validator for individual tree nodes that compares input nodes against schema nodes.
pub struct BiNodeValidator<'a> {
    initial_input_cursor: &'a TreeCursor<'a>,
    initial_schema_cursor: &'a TreeCursor<'a>,
    input_str: &'a str,
    schema_str: &'a str,
    pub errors: Vec<Error>,
    pub input_descendant_index: usize,
    pub schema_descendant_index: usize,
    pub eof: bool,
}

impl<'a> BiNodeValidator<'a> {
    /// Create a new NodeValidator instance.
    pub fn new(
        input_cursor: &'a TreeCursor<'a>,
        schema_cursor: &'a TreeCursor<'a>,
        input_str: &'a str,
        schema_str: &'a str,
        eof: bool,
    ) -> Self {
        debug!(
            "Creating BiNodeValidator: input_node='{}', schema_node='{}', eof={}",
            input_cursor.node().kind(),
            schema_cursor.node().kind(),
            eof
        );

        debug!(
            "Root trees for BiNodeValidator,\nINPUT:\n{}\nSCHEMA:\n{}\n",
            node_to_str(input_cursor.node(), input_str),
            node_to_str(schema_cursor.node(), schema_str)
        );

        Self {
            initial_input_cursor: input_cursor,
            initial_schema_cursor: schema_cursor,
            input_str,
            schema_str,
            errors: Vec::new(),
            input_descendant_index: input_cursor.descendant_index(),
            schema_descendant_index: schema_cursor.descendant_index(),
            eof,
        }
    }

    fn root_node(&self) -> Node<'a> {
        self.initial_input_cursor.node()
    }

    /// Validate a single node using the corresponding schema node.
    /// Mutates the internal errors and descendant index fields.
    pub fn validate(&mut self) {
        debug!("Starting node validation");

        // If the current node is "text" then we check for literal match

        let mut input_cursor = self.initial_input_cursor.clone();
        let mut schema_cursor = self.initial_schema_cursor.clone();

        let root_input_descendant_index = input_cursor.descendant_index();
        let root_schema_descendant_index = schema_cursor.descendant_index();

        let nodes_to_validate = Vec::new(); // index of nodes to validate

        let input_str = self.input_str;
        let schema_str = self.schema_str;
        let eof = self.eof;

        loop {
            let input_node = input_cursor.node();
            let schema_node = schema_cursor.node();

            // If they are both text, directly compare them
            if schema_node.kind() == "text" {
                debug!("Validating text node");
                self.errors.extend(Self::validate_text_node_static(
                    &input_node,
                    input_cursor.descendant_index(),
                    &schema_node,
                    input_str,
                    schema_str,
                    eof,
                    &input_node,
                ));
            } else { // Otherwise, look at their children
                 // If the children of the schema node contains a matcher among
                 // text nodes, and the input node is just text, we validate the
                 // matcher using our matcher helper. It takes care of prefix/suffix
                 // matching as well.


                 // If there are no code nodes in the schema children, then it
                 // may be a mix of nodes we must recurse on.
                 // iterate over the children of both the schema and input nodes
                 // in order using the walker, and push them to 
            }
        }

        // self.input_descendant_index = root_input_descendant_index + offset;
        // self.schema_descendant_index = root_schema_descendant_index + offset;
    }

    fn validate_child_nodes_static<'b>(
        input_node: &Node<'b>,
        schema_node: &Node<'b>,
    ) -> Vec<Error> {
        let mut errors = Vec::new();

        if input_node.kind() != schema_node.kind() {
            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch(
                    input_node.descendant_count(),
                    schema_node.descendant_count(),
                ),
            ));
        }

        errors
    }

    /// Validate a text node against the schema text node.
    ///
    /// This is a node that is just a simple literal text node. We validate that
    /// the text content is identical.
    fn validate_text_node_static<'b>(
        input_node: &Node<'b>,
        input_node_descendant_index: usize,
        schema_node: &Node<'b>,
        input_str: &'b str,
        schema_str: &'b str,
        eof: bool,
        initial_input_node: &Node<'b>,
    ) -> Vec<Error> {
        debug!("Validating text node content");

        if (input_node.byte_range().end == initial_input_node.byte_range().end) && eof == false {
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
}

/// Validate a single node using the corresponding schema node.
/// Then walk the cursors to the next nodes. Returns errors and new descendant indices.
///
/// This function is kept for backward compatibility. Consider using BiNodeValidator::validate() instead.
pub fn validate_a_node<'a>(
    input_cursor: &'a TreeCursor<'a>,
    schema_cursor: &'a TreeCursor<'a>,
    last_input_str: &'a str,
    schema_str: &'a str,
    eof: bool,
) -> (Vec<Error>, (usize, usize)) {
    let mut validator =
        BiNodeValidator::new(input_cursor, schema_cursor, last_input_str, schema_str, eof);

    validator.validate();

    (
        validator.errors,
        (
            validator.input_descendant_index,
            validator.schema_descendant_index,
        ),
    )
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::utils::new_markdown_parser;

    use super::*;

    #[test]
    fn test_validate_only_two_text_nodes() {
        let input = "Hello, world!";
        let schema = "Hello, world!";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap().child(0).unwrap();
        assert!(input_node.kind() == "text", "Got {}", input_node.kind());

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node().child(0).unwrap().child(0).unwrap();
        assert!(schema_node.kind() == "text", "Got {}", schema_node.kind());

        let input_cursor = input_node.walk();
        let schema_cursor = schema_node.walk();

        let (errors, (input_index, schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);

        assert!(errors.is_empty());
        assert_eq!(input_index, schema_index);
    }

    #[test]
    fn test_validate_two_different_text_nodes() {
        let input = "Hello, world!";
        let schema = "Hello, everyone!";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap().child(0).unwrap();
        assert!(input_node.kind() == "text", "Got {}", input_node.kind());

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node().child(0).unwrap().child(0).unwrap();
        assert!(schema_node.kind() == "text", "Got {}", schema_node.kind());

        let input_cursor = input_node.walk();
        let schema_cursor = schema_node.walk();

        let (errors, (input_index, schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);

        assert!(!errors.is_empty());
        println!("Errors: {:?}", errors);
        // Check that we have a NodeContentMismatch error
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(node, expected)) => {
                assert_eq!(*expected, "Hello, everyone!");
                assert_eq!(*node, input_index);
            }
            _ => std::panic!("Expected NodeContentMismatch error, got: {:?}", errors[0]),
        }
        // Descendant indices should be equal since both text nodes are at the same position in their trees
        assert_eq!(input_index, schema_index);
    }

    #[test]
    fn test_validate_two_paragraph_nodes_with_same_text() {
        let input = "This is a paragraph.";
        let schema = "This is a paragraph.";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        assert!(
            input_node.kind() == "paragraph",
            "Got {}",
            input_node.kind()
        );

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node().child(0).unwrap();
        assert!(
            schema_node.kind() == "paragraph",
            "Got {}",
            schema_node.kind()
        );

        let input_cursor = input_node.walk();
        let schema_cursor = schema_node.walk();

        let (errors, (input_index, schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);

        assert!(errors.is_empty());
        assert_eq!(input_index, schema_index);
    }

    #[test]
    fn test_validate_two_h1_paragraph_with_same_text() {
        let input = "# Heading\n\nThis is a paragraph.";
        let schema = "# Heading\n\nThis is a paragraph.";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        assert!(
            input_node.kind() == "atx_heading",
            "Got {}",
            input_node.kind()
        );

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node().child(0).unwrap();
        assert!(
            schema_node.kind() == "atx_heading",
            "Got {}",
            schema_node.kind()
        );

        let input_cursor = input_node.walk();
        let schema_cursor = schema_node.walk();

        let (errors, (input_index, schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);

        assert!(errors.is_empty());
        assert_eq!(input_index, schema_index);
        assert_eq!(input_index, 2); // We need to leave off at the next node
    }

    #[test]
    fn test_validate_two_different_headings_same_text() {
        let input = "# Heading";
        let schema = "## Heading";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();
        assert!(
            input_node.kind() == "atx_heading",
            "Got {}",
            input_node.kind()
        );

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node().child(0).unwrap();
        assert!(
            schema_node.kind() == "atx_heading",
            "Got {}",
            schema_node.kind()
        );

        let input_cursor = input_node.walk();
        let schema_cursor = schema_node.walk();

        let (errors, (input_index, schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);

        assert!(
            !errors.is_empty(),
            "Expected validation errors but found none"
        );
        // Descendant indices should be equal since both are at the same tree position
        assert_eq!(input_index, schema_index);

        // Check that we have a NodeTypeMismatch error
        assert!(
            errors.iter().any(|error| matches!(
                error,
                Error::SchemaViolation(SchemaViolationError::NodeTypeMismatch(_, _))
            )),
            "Expected a node type mismatch error but did not find one. Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_not_at_eof_final_chars_mismatch() {
        let input = "# Test\nHello, wor";
        let schema = "# Test\nHello, world";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node();

        let input_cursor = input_node.walk();
        let schema_cursor = schema_node.walk();

        // First pass with eof: false should pass without errors
        let (errors, (input_index, _schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, false);
        let (_, (input_index_eof, _)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);
        assert_eq!(input_index, 2); // We asserted the first node, but need to re-assert the second
        assert_eq!(input_index_eof, 3);
        assert!(
            errors.is_empty(),
            "Expected no errors but found: {:?}",
            errors
        );

        // When eof is false, schema should move back, so indices may differ
        // The exact difference depends on the tree structure

        // And now pass with eof: true and make sure it fails
        let (errors, (input_index, schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);

        assert!(
            !errors.is_empty(),
            "Expected validation errors at EOF but found none: {:?}",
            errors
        );
        // Both should end at the same descendant index in their respective trees
        assert_eq!(input_index, schema_index);
    }

    #[test]
    fn test_validate_a_node_with_mismatched_content() {
        let schema = "# Test

fooobar

test

";
        let input = "fooobar

testt

";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let schema_node = schema_tree.root_node();

        let input_cursor = input_node.walk();
        let schema_cursor = schema_node.walk();

        let (errors, (input_index, schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);

        assert!(
            !errors.is_empty(),
            "Expected validation errors but found none"
        );
        // Check for either NodeTypeMismatch or NodeContentMismatch errors
        assert!(
            errors.iter().any(|error| matches!(
                error,
                Error::SchemaViolation(SchemaViolationError::NodeTypeMismatch(_, _))
                    | Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, _))
            )),
            "Expected a mismatch error but did not find one. Errors: {:?}",
            errors
        );
        // Both trees should end at the same relative descendant position
        assert_eq!(input_index, schema_index);
    }
}
