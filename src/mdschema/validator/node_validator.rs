use crate::mdschema::reports::errors::ValidatorError;
use tree_sitter::TreeCursor;

/// A validator for individual tree nodes that compares input nodes against schema nodes.
pub struct BiNodeValidator<'a> {
    input_cursor: &'a TreeCursor<'a>,
    schema_cursor: &'a TreeCursor<'a>,
    input_str: &'a str,
    schema_str: &'a str,
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
        }
    }

    /// Validate a single node using the corresponding schema node.
    /// Returns validation errors and new byte offsets for both input and schema.
    pub fn validate(&self) -> (Vec<ValidatorError>, (usize, usize)) {
        let mut errors = Vec::new();

        // If the current node is "text" then we check for literal match

        let input_node = self.input_cursor.node();
        let schema_node = self.schema_cursor.node();

        if schema_node.kind() == "text" {
            errors.extend(self.validate_text_node());
        } else {
            // If the current node has children that include `code_span` AND
            // `text` then we must handle this specially.
        }

        let new_schema_offset = schema_node.byte_range().end;
        let new_input_offset = input_node.byte_range().end;

        (errors, (new_input_offset, new_schema_offset))
    }

    fn validate_text_node(&self) -> Vec<ValidatorError> {
        let mut errors = Vec::new();

        let input_node = self.input_cursor.node();
        let schema_node = self.schema_cursor.node();

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
/// This function is kept for backward compatibility. Consider using NodeValidator::validate() instead.
pub fn validate_a_node(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    last_input_str: &str,
    schema_str: &str,
) -> (Vec<ValidatorError>, (usize, usize)) {
    let validator = BiNodeValidator::new(input_cursor, schema_cursor, last_input_str, schema_str);
    validator.validate()
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
}
