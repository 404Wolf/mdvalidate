use crate::mdschema::{reports::errors::ValidatorError, validator::utils::node_to_str};
use log::debug;
use tree_sitter::{Node, TreeCursor};

/// A validator for individual tree nodes that compares input nodes against schema nodes.
pub struct BiNodeValidator<'a> {
    initial_input_cursor: &'a TreeCursor<'a>,
    initial_schema_cursor: &'a TreeCursor<'a>,
    input_str: &'a str,
    schema_str: &'a str,
    pub errors: Vec<ValidatorError>,
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

    /// Validate a single node using the corresponding schema node.
    /// Mutates the internal errors and descendant index fields.
    pub fn validate(&mut self) {
        debug!("Starting node validation");

        // If the current node is "text" then we check for literal match

        let input_cursor = self.initial_input_cursor;
        let schema_cursor = self.initial_schema_cursor;

        let mut nodes_to_validate: Vec<(TreeCursor, TreeCursor)> =
            vec![(input_cursor.clone(), schema_cursor.clone())];

        let mut last_input_cursor = input_cursor.clone();
        let mut last_schema_cursor = schema_cursor.clone();

        while let Some((mut input_cursor, mut schema_cursor)) = nodes_to_validate.pop() {
            let input_node = input_cursor.node();
            let schema_node = schema_cursor.node();

            // Track the last cursors we process
            last_input_cursor = input_cursor.clone();
            last_schema_cursor = schema_cursor.clone();

            debug!(
                "Validating node pair: input='{}' vs schema='{}'",
                input_node.kind(),
                schema_node.kind()
            );

            if schema_node.kind() == "text" {
                debug!("Validating text node");
                self.errors
                    .extend(self.validate_text_node(&input_node, &schema_node));
            } else {
                debug!(
                    "Validating non-text node: {} children vs {} schema children",
                    input_node.child_count(),
                    schema_node.child_count()
                );

                let input_node_children =
                    input_node.children(&mut input_cursor).collect::<Vec<_>>();

                let schema_node_children =
                    schema_node.children(&mut schema_cursor).collect::<Vec<_>>();

                debug!(
                    "Input node has {} children, schema node has {} children",
                    input_node_children.len(),
                    schema_node_children.len()
                );

                let schema_node_code_children = schema_node
                    .children(&mut schema_cursor.clone())
                    .filter(|n| n.kind() == "code_span")
                    .collect::<Vec<_>>();

                if schema_node_code_children.is_empty() {
                    for (input_child, schema_child) in
                        // Check that they are the same node type
                        input_node_children.iter().zip(schema_node_children.iter())
                    {
                        debug!(
                            "Validating child node pair: input='{}' vs schema='{}'",
                            input_child.kind(),
                            schema_child.kind()
                        );

                        debug!(
                            "Current trees for nodes we are appending,\nINPUT:\n{}\nSCHEMA:\n{}\n",
                            node_to_str(*input_child, self.input_str),
                            node_to_str(*schema_child, self.schema_str)
                        );

                        self.errors
                            .extend(self.validate_child_nodes(input_child, schema_child));

                        nodes_to_validate.push((input_child.walk(), schema_child.walk()));
                    }
                } else {
                    todo!(
                        "Non-text node validation with code_span children is not yet implemented"
                    );
                }
            }
        }

        self.input_descendant_index = last_input_cursor.descendant_index();
        self.schema_descendant_index = last_schema_cursor.descendant_index();
        print!(
            "Final indices: input_descendant_index={}, schema_descendant_index={}\n",
            self.input_descendant_index, self.schema_descendant_index
        );

        // If EOF is false, we should move back to the previous descendant in the schema
        // TODO: Is this right?
        if !self.eof {
            // Move the schema cursor back to the beginning of the current node
            if last_schema_cursor.goto_parent() {
                self.schema_descendant_index = last_schema_cursor.descendant_index();
            }
        }
    }

    fn validate_child_nodes(&self, input_node: &Node, schema_node: &Node) -> Vec<ValidatorError> {
        let mut errors = Vec::new();

        if input_node.kind() != schema_node.kind() {
            errors.push(ValidatorError::from_offset(
                format!(
                    "Node mismatch: expected '{}', found '{}'",
                    schema_node.kind(),
                    input_node.kind()
                ),
                input_node.byte_range().start,
                input_node.byte_range().end,
                self.input_str,
            ));
        }

        errors
    }

    fn validate_text_node(&self, input_node: &Node, schema_node: &Node) -> Vec<ValidatorError> {
        debug!("Validating text node content");

        if (input_node.byte_range().end == self.initial_input_cursor.node().byte_range().end)
            && self.eof == false
        {
            // Incomplete text node, skip validation for now
            debug!("Skipping text validation - incomplete node at EOF");
            return Vec::new();
        }

        let mut errors = Vec::new();

        let schema_text = &self.schema_str[schema_node.byte_range()];
        let input_text = &self.input_str[input_node.byte_range()];

        debug!(
            "Comparing text: schema='{}' vs input='{}'",
            schema_text, input_text
        );

        if schema_text != input_text {
            debug!("Text mismatch found");
            errors.push(ValidatorError::from_offset(
                format!(
                    "Literal mismatch: expected \"{}\", found \"{}\"",
                    schema_text, input_text
                ),
                input_node.byte_range().start + 1,
                input_node.byte_range().end,
                self.input_str,
            ));
        }

        errors
    }
}

/// Validate a single node using the corresponding schema node.
/// Then walk the cursors to the next nodes. Returns errors and new descendant indices.
///
/// This function is kept for backward compatibility. Consider using BiNodeValidator::validate() instead.
pub fn validate_a_node(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    last_input_str: &str,
    schema_str: &str,
    eof: bool,
) -> (Vec<ValidatorError>, (usize, usize)) {
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
        assert_eq!(
            errors[0].message,
            "Literal mismatch: expected 'Hello, everyone!', found 'Hello, world!'"
        );
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

        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("Node mismatch")),
            "Expected a node mismatch error but did not find one"
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
        let (errors, (_input_index, _schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, false);

        assert!(errors.is_empty());
        // When eof is false, schema should move back, so indices may differ
        // The exact difference depends on the tree structure

        // And now pass with eof: true and make sure it fails
        let (errors, (input_index, schema_index)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema, true);

        assert!(!errors.is_empty());
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
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("mismatch")),
            "Expected a mismatch error but did not find one. Errors: {:?}",
            errors
        );
        // Both trees should end at the same relative descendant position
        assert_eq!(input_index, schema_index);
    }
}
