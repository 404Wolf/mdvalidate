use tree_sitter::{Parser, Tree, TreeCursor};

use crate::mdschema::validator::Validator;
use crate::mdschema::{errors::ValidatorError, reports::ValidatorReport};

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
}

impl Validator for ValidationZipperTree {
    /// Create a new ValidationZipperTree with the given schema and input strings.
    fn new(schema_str: &str, input_str: &str) -> Result<Self, Box<dyn std::error::Error>> {
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
        })
    }

    /// Read new input. Updates the input tree with a new input tree for the full new input.
    ///
    /// Does not update the schema tree or change the offsets. You will still
    /// need to call `validate` to validate until the end of the current input
    /// (which this updates).
    fn read_input(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.last_input_str = input.to_string();

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
        let (new_schema_offset, new_input_offset) = self
            .validate_nodes_from_offset_to_end_of_input(
                &mut self.input_tree.walk(),
                &mut self.schema_tree.walk(),
                self.last_input_tree_offset,
                self.last_schema_tree_offset,
            );

        self.last_input_tree_offset = new_input_offset;
        self.last_schema_tree_offset = new_schema_offset;

        Ok(())
    }

    fn report(&self) -> ValidatorReport {
        ValidatorReport::new(self.errors.clone(), self.last_input_str.clone())
    }
}

impl ValidationZipperTree {
    /// Validate nodes and walk until the end of the input tree, starting from
    /// the given offsets.
    ///
    /// Uses `validate_node_and_walk` to validate each node and move the cursors forward.
    ///
    /// Returns the final offsets (schema_offset, input_offset) after the "walk".
    fn validate_nodes_from_offset_to_end_of_input(
        &self,
        cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
        input_offset: usize,
        schema_offset: usize,
    ) -> (usize, usize) {
        let mut last_schema_tree_offset = schema_offset;
        let mut last_input_tree_offset = input_offset;

        // Move cursors to the starting offsets
        goto_tree_offset(cursor, input_offset);
        goto_tree_offset(schema_cursor, schema_offset);

        // Walk up until the end. `self.last_input_str` will not change while
        // this is running since this blocks the thread.
        while last_input_tree_offset < self.last_input_str.len() {
            let (new_schema_offset, new_input_offset) =
                self.validate_node_and_walk(cursor, schema_cursor);

            // Update the offsets as we make progress towards the end.
            last_schema_tree_offset = new_schema_offset;
            last_input_tree_offset = new_input_offset;
        }

        (last_schema_tree_offset, last_input_tree_offset)
    }

    /// Validate the next node forward from the most recent offsets.
    ///
    /// Returns the new offsets after validation (schema_offset, input_offset)
    /// after the "walk".
    fn validate_node_and_walk(
        &self,
        cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) -> (usize, usize) {
        // TODO: Actually validate the nodes to make sure they match

        cursor.goto_next_sibling();
        schema_cursor.goto_next_sibling();

        (get_tree_offset(schema_cursor), get_tree_offset(cursor))
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

/// Get the byte offset of the end of the current node.
fn get_tree_offset(tree: &TreeCursor) -> usize {
    tree.node().byte_range().end
}

/// Move the cursor to the node that contains the given byte offset.
/// Returns true if the cursor was moved, false otherwise.
fn goto_tree_offset(tree: &mut TreeCursor, offset: usize) -> bool {
    tree.goto_first_child_for_byte(offset).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goto_tree_offset() {
        let source = "# Heading\n\nSome **bold** text.";

        let mut validation_zipper_tree =
            ValidationZipperTree::new("# Heading\n\nSome **bold** text.", source).unwrap();

        {
            assert!(validation_zipper_tree.last_input_tree_offset == 0);
        }

        validation_zipper_tree.validate().unwrap();

        {
            assert!(validation_zipper_tree.last_input_tree_offset == source.len());
        }

        let report = validation_zipper_tree.report();
        assert!(report.errors.is_empty());
        assert_eq!(report.source_content, source);
    }
}
