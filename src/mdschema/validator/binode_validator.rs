use crate::mdschema::reports::errors::ValidatorError;
use tree_sitter::{Node, TreeCursor};

/// A validator for individual tree nodes that compares input nodes against schema nodes.
pub struct BiNodeValidator<'a> {
    input_cursor: &'a TreeCursor<'a>,
    schema_cursor: &'a TreeCursor<'a>,
    input_str: &'a str,
    schema_str: &'a str,
    pub errors: Vec<ValidatorError>,
    pub input_offset: usize,
    pub schema_offset: usize,
}

impl<'a> BiNodeValidator<'a> {
    /// Create a new NodeValidator instance.
    pub fn new(
        input_cursor: &'a TreeCursor<'a>,
        schema_cursor: &'a TreeCursor<'a>,
        input_str: &'a str,
        schema_str: &'a str,
    ) -> Self {
        Self {
            input_cursor,
            schema_cursor,
            input_str,
            schema_str,
            errors: Vec::new(),
            input_offset: input_cursor.node().byte_range().end,
            schema_offset: schema_cursor.node().byte_range().end,
        }
    }

    /// Validate a single node using the corresponding schema node.
    /// Mutates the internal errors and offset fields.
    pub fn validate(&mut self) {
        // If the current node is "text" then we check for literal match

        let input_cursor = self.input_cursor;
        let schema_cursor = self.schema_cursor;

        let input_node = input_cursor.node();
        let schema_node = schema_cursor.node();

        let mut nodes_to_validate: Vec<(TreeCursor, TreeCursor)> =
            vec![(input_cursor.clone(), schema_cursor.clone())];

        while let Some((mut input_cursor, mut schema_cursor)) = nodes_to_validate.pop() {
            let input_node = input_cursor.node();
            let schema_node = schema_cursor.node();

            if schema_node.kind() == "text" {
                self.errors
                    .extend(self.validate_text_node(&input_node, &schema_node));
            } else {
                let input_node_children =
                    input_node.children(&mut input_cursor).collect::<Vec<_>>();

                let schema_node_children =
                    schema_node.children(&mut schema_cursor).collect::<Vec<_>>();

                let schema_node_code_children = schema_node
                    .children(&mut schema_cursor.clone())
                    .filter(|n| n.kind() == "code_span")
                    .collect::<Vec<_>>();

                if schema_node_code_children.is_empty() {
                    for (input_child, schema_child) in
                        // Check that they are the same node type
                        input_node_children.iter().zip(schema_node_children.iter())
                    {
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

        self.schema_offset = schema_node.byte_range().end;
        self.input_offset = input_node.byte_range().end;
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
        let mut errors = Vec::new();

        let schema_text = &self.schema_str[schema_node.byte_range()];
        let input_text = &self.input_str[input_node.byte_range()];

        if schema_text != input_text {
            errors.push(ValidatorError::from_offset(
                format!(
                    "Literal mismatch: expected '{}', found '{}'",
                    schema_text, input_text
                ),
                input_node.byte_range().start,
                input_node.byte_range().end,
                self.input_str,
            ));
        }

        errors
    }
}

/// Validate a single node using the corresponding schema node.
/// Then walk the cursors to the next nodes. Returns errors and new offsets.
///
/// This function is kept for backward compatibility. Consider using BiNodeValidator::validate() instead.
pub fn validate_a_node(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    last_input_str: &str,
    schema_str: &str,
) -> (Vec<ValidatorError>, (usize, usize)) {
    let mut validator =
        BiNodeValidator::new(input_cursor, schema_cursor, last_input_str, schema_str);
    validator.validate();
    (
        validator.errors,
        (validator.input_offset, validator.schema_offset),
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

        let (errors, (input_offset, schema_offset)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema);

        assert!(errors.is_empty());
        assert_eq!(input_offset, schema_offset);
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

        let (errors, (input_offset, schema_offset)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema);

        assert!(!errors.is_empty());
        assert_eq!(
            errors[0].message,
            "Literal mismatch: expected 'Hello, everyone!', found 'Hello, world!'"
        );
        assert_eq!(input_offset, input.len());
        assert_eq!(schema_offset, schema.len());
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

        let (errors, (input_offset, schema_offset)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema);

        assert!(errors.is_empty());
        assert_eq!(input_offset, schema_offset);
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

        let (errors, (input_offset, schema_offset)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema);

        assert!(errors.is_empty());
        assert_eq!(input_offset, schema_offset);
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

        let (errors, (input_offset, schema_offset)) =
            validate_a_node(&input_cursor, &schema_cursor, input, schema);

        println!("Errors: {:#?}", errors);

        assert!(
            !errors.is_empty(),
            "Expected validation errors but found none"
        );
        assert_eq!(input_offset, input.len());
        assert_eq!(schema_offset, schema.len());

        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("Node mismatch")),
            "Expected a node mismatch error but did not find one"
        );
    }
}
