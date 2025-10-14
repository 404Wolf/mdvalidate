use std::sync::Mutex;

use tree_sitter::{Parser, Tree, TreeCursor};

use crate::mdschema::validator::Validator;
use crate::mdschema::{errors::ValidatorError, reports::ValidatorReport};

/// A Validator implementation that uses a zipper tree approach to validate
/// an input Markdown document against a markdown schema treesitter tree.
pub struct ValidationZipperTree {
    state: Mutex<ValidationZipperTreeState>,
    last_input_str: String,
}

struct ValidationZipperTreeState {
    input_tree: Tree,
    schema_tree: Tree,
    last_schema_tree_offset: usize,
    last_input_tree_offset: usize,
    errors: Vec<ValidatorError>,
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
            state: Mutex::new(ValidationZipperTreeState {
                input_tree,
                schema_tree,
                last_input_tree_offset: 0,
                last_schema_tree_offset: 0,
                errors: Vec::new(),
            }),
            last_input_str: input_str.to_string(),
        })
    }

    /// Read new input. Updates the input tree with a new input tree for the full new input.
    /// Does not update the schema tree or change the offsets. You will still
    /// need to call `validate` to validate until the end of the current input
    /// (which this updates).
    fn read_input(&self, input: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Failed to acquire lock on validator state")?;

        let mut input_parser = new_markdown_parser();
        state.input_tree = input_parser
            .parse(input, Some(&state.input_tree))
            .ok_or("Failed to parse updated input")?;

        Ok(())
    }

    /// Validate the input against the schema. Validates picking up from where
    /// we left off.
    fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Lock the state for the duration of validation
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Failed to acquire lock on validator state")?;

        // With our current understanding of state, validate until the end of the input
        let (new_schema_offset, new_input_offset) = self
            .validate_nodes_from_offset_to_end_of_input(
                &mut state.input_tree.walk(),
                &mut state.schema_tree.walk(),
                state.last_input_tree_offset,
                state.last_schema_tree_offset,
            );

        state.last_input_tree_offset = new_input_offset;
        state.last_schema_tree_offset = new_schema_offset;

        Ok(())
    }

    fn report(&self) -> ValidatorReport {
        ValidatorReport::new(
            self.state.lock().unwrap().errors.clone(),
            self.last_input_str.clone(),
        )
    }
}

impl ValidationZipperTree {
    /// Validate nodes and walk until the end of the input tree, starting from
    /// the given offsets.
    ///
    /// Returns the final offsets (schema_offset, input_offset).
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

        while last_input_tree_offset < self.last_input_str.len() {
            let (new_schema_offset, new_input_offset) =
                self.validate_node_and_walk(cursor, schema_cursor);

            last_schema_tree_offset = new_schema_offset;
            last_input_tree_offset = new_input_offset;
        }

        (last_schema_tree_offset, last_input_tree_offset)
    }

    /// Validate the next node forward from the most recent offsets.
    ///
    /// Returns the new offsets after validation (schema_offset, input_offset).
    fn validate_node_and_walk(
        &self,
        cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) -> (usize, usize) {
        // <actually validate the nodes to make sure they match>
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

        let validation_zipper_tree =
            ValidationZipperTree::new("# Heading\n\nSome **bold** text.", source).unwrap();

        let state = validation_zipper_tree.state.lock().unwrap();
        assert!(state.last_input_tree_offset == 0);
        drop(state);

        validation_zipper_tree.validate().unwrap();

        let state = validation_zipper_tree.state.lock().unwrap();
        assert!(state.last_input_tree_offset == source.len());
        drop(state);

        let report = validation_zipper_tree.report();
        assert!(report.errors.is_empty());
        assert_eq!(report.source_content, source);
    }
}
