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

        print!("{}", self);
        // atx_heading[0..13]: "# Test `test`"
        //   atx_h1_marker[0..1]: "#"
        //   heading_content[1..13]: " Test `test`"
        //     text[1..7]: " Test "
        //     code_span[7..13]: "`test`"
        //       text[8..12]: "test"

        // If the current node is "text" then we check for literal match

        let input_node = self.input_cursor.node();
        let schema_node = self.schema_cursor.node();

        if schema_node.kind() == "text" {
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
        } else {
            // If the current node has children that include `code_span` AND
            // `text` then we must handle this specially.
        }

        let new_schema_offset = schema_node.byte_range().end;
        let new_input_offset = input_node.byte_range().end;

        (errors, (new_input_offset, new_schema_offset))
    }

    fn node_to_string_recursive(&self, node: tree_sitter::Node, depth: usize) -> String {
        let indent = "  ".repeat(depth);
        let mut result = format!(
            "{}{}[{}..{}]",
            indent,
            node.kind(),
            node.byte_range().start,
            node.byte_range().end
        );

        if node.child_count() == 0 {
            let text = &self.input_str[node.byte_range()];
            result.push_str(&format!(": {:?}", text));
        }

        result.push('\n');

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                result.push_str(&self.node_to_string_recursive(cursor.node(), depth + 1));
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        result
    }
}

impl std::fmt::Display for BiNodeValidator<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let input_node = self.input_cursor.node();
        let schema_node = self.schema_cursor.node();

        writeln!(
            f,
            "Input Node:\n{}",
            self.node_to_string_recursive(input_node, 0)
        )?;
        writeln!(
            f,
            "Schema Node:\n{}",
            self.node_to_string_recursive(schema_node, 0)
        )?;

        Ok(())
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
