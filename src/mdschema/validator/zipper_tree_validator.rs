use tree_sitter::{Parser, Tree, TreeCursor};

use crate::mdschema::{
    reports::{errors::ValidatorError, validation_report::ValidatorReport},
    validator::validator::Validator,
};

/// A Validator implementation that uses a zipper tree approach to validate
/// an input Markdown document against a markdown schema treesitter tree.
pub struct ValidationZipperTree {
    /// The current input tree. When read_input is called, this is replaced with a new tree.
    input_tree: Tree,
    /// The schema tree, which does not change after initialization.
    schema_tree: Tree,
    /// The last byte offset we validated up to in the schema tree.
    last_schema_tree_offset: usize,
    /// The last byte offset we validated up to in the input tree.
    last_input_tree_offset: usize,
    /// Any errors encountered during validation.
    errors: Vec<ValidatorError>,
    /// The full input string as last read. Not used internally but useful for
    /// debugging or reporting.
    last_input_str: String,
    /// The full schema string. Does not change.
    schema_str: String,
    /// Whether we have received the end of the input. This means that last
    /// input tree offset is at the end of the input.
    got_eof: bool,
}

impl Validator for ValidationZipperTree {
    /// Create a new ValidationZipperTree with the given schema and input strings.
    fn new(
        schema_str: &str,
        input_str: &str,
        eof: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema_str, None)
            .ok_or("Failed to parse schema")?;

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input_str, None)
            .ok_or("Failed to parse input")?;

        Ok(ValidationZipperTree {
            input_tree,
            schema_tree,
            last_input_tree_offset: 0,
            last_schema_tree_offset: 0,
            errors: Vec::new(),
            last_input_str: input_str.to_string(),
            schema_str: schema_str.to_string(),
            got_eof: eof,
        })
    }

    /// Read new input. Updates the input tree with a new input tree for the full new input.
    ///
    /// Does not update the schema tree or change the offsets. You will still
    /// need to call `validate` to validate until the end of the current input
    /// (which this updates).
    fn read_input(&mut self, input: &str, eof: bool) -> Result<(), Box<dyn std::error::Error>> {
        self.last_input_str = input.to_string();
        self.got_eof = eof;

        let mut input_parser = new_markdown_parser();
        self.input_tree = input_parser
            .parse(input, Some(&self.input_tree))
            .ok_or("Failed to parse updated input")?;

        Ok(())
    }

    /// Validate the input against the schema. Validates picking up from where
    /// we left off.
    fn validate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // With our current understanding of state, validate until the end of the input
        self.validate_nodes_from_offset_to_end_of_input(
            &mut self.input_tree.clone().walk(),
            &mut self.schema_tree.clone().walk(),
        );

        Ok(())
    }

    fn report(&self) -> crate::mdschema::reports::validation_report::ValidatorReport {
        return ValidatorReport::new(self.errors.clone(), self.last_input_str.clone());
    }
}

impl ValidationZipperTree {
    /// Validate nodes and walk until the end of the input tree, starting from
    /// the current offsets.
    ///
    /// Uses `validate_node` to validate each node and move the cursors forward.
    /// Directly mutates the last_offsets in the struct.
    fn validate_nodes_from_offset_to_end_of_input(
        &mut self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) {
        // Move cursors to the starting offsets
        _ = schema_cursor
            .goto_first_child_for_byte(self.last_schema_tree_offset)
            .is_some();

        // Walk up until the end. `self.last_input_str` will not change while
        // this is running since this blocks the thread.
        while self.last_input_tree_offset < self.last_input_str.len() {
            // We may cause a shift to the current treecursors inside ourself by
            // calling this, but it is important that we "commit" the change by
            // actually updating the offsets after validating.
            self.validate_node(input_cursor, schema_cursor);

            // Update the offsets as we make progress towards the end.
            self.last_schema_tree_offset = schema_cursor.node().byte_range().end;
            self.last_input_tree_offset = input_cursor.node().byte_range().end;
        }
    }

    /// Validate a single node using the corresponding schema node.
    /// Then walk the cursors to the next nodes. Mutates self to walk cursors
    /// and record errors.
    fn validate_node(&mut self, input_cursor: &mut TreeCursor, schema_cursor: &mut TreeCursor) {
        println!("Validating node: {}", input_cursor.node().kind());

        let input_node = input_cursor.node();
        let schema_node = schema_cursor.node();

        // If there are no children, check if the literal matches
        if input_node.child_count() == 0 {
            let input_literal = &self.last_input_str[input_node.byte_range()];
            let schema_literal = &self.schema_str[schema_node.byte_range()];

            if input_literal != schema_literal {
                let error = ValidatorError::from_offset(
                    format!(
                        "Literal mismatch: expected '{}', found '{}'",
                        schema_literal, input_literal
                    ),
                    input_node.start_byte(),
                    input_node.end_byte(),
                    &self.last_input_str,
                );
                self.errors.push(error);
            }

            // Move cursors to the next nodes
            self.goto_next_node(input_cursor);
            self.goto_next_node(schema_cursor);
        }
    }

    fn goto_next_node(&self, cursor: &mut TreeCursor) {
        if !cursor.goto_next_sibling() {
            cursor.goto_parent();
            cursor.goto_next_sibling();
        }
    }
}

/// Create a new Tree-sitter parser for Markdown.
fn new_markdown_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(tree_sitter_markdown::language())
        .unwrap();
    parser
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_walks_on_validate() {
        let source = "# Heading\n\nSome **bold** text.";

        let mut validation_zipper_tree =
            ValidationZipperTree::new("# Heading\n\nSome **bold** text.", source, true).unwrap();

        assert!(validation_zipper_tree.last_input_tree_offset == 0);

        validation_zipper_tree.validate().unwrap();

        assert!(validation_zipper_tree.last_input_tree_offset == source.len());

        let report = validation_zipper_tree.report();

        assert!(report.errors.is_empty());
        assert_eq!(report.source_content, source);
    }

    #[test]
    fn test_detects_literal_match() {
        let schema_str = "**strong**";
        let input_str = "**strong**";

        let mut validation_zipper_tree =
            ValidationZipperTree::new(schema_str, input_str, true).unwrap();
        validation_zipper_tree.validate().unwrap();
        let report = validation_zipper_tree.report();
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_detects_literal_mismatch() {
        let schema_str = "**strong**";
        let input_str = "**bold**";
        let mut validation_zipper_tree =
            ValidationZipperTree::new(schema_str, input_str, true).unwrap();

        validation_zipper_tree.validate().unwrap();
        let report = validation_zipper_tree.report();
        assert!(!report.errors.is_empty());
        assert_eq!(
            report.errors[0].message,
            "Literal mismatch: expected '**strong**', found '**bold**'"
        );
        assert_eq!(report.source_content, input_str);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].byte_start, 0);
        assert_eq!(report.errors[0].byte_end, 8);
        assert_eq!(
            report.errors[0].message,
            "Literal mismatch: expected '**strong**', found '**bold**'"
        );
    }
}
