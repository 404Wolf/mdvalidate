use anyhow::{anyhow, Result};
use line_col::LineColLookup;
use log::debug;
use tree_sitter::Tree;

use crate::mdschema::{
    reports::{errors::ValidatorError, validation_report::ValidatorReport},
    validator::{
        binode_validator::validate_a_node,
        utils::{get_total_descendants, new_markdown_parser, node_to_str},
    },
};

/// A Validator implementation that uses a zipper tree approach to validate
/// an input Markdown document against a markdown schema treesitter tree.
pub struct Validator {
    /// The current input tree. When read_input is called, this is replaced with a new tree.
    input_tree: Tree,
    /// The schema tree, which does not change after initialization.
    schema_tree: Tree,
    /// The last descendant index we validated up to in the schema tree.
    last_schema_descendant_index: usize,
    /// The last descendant index we validated up to in the input tree.
    last_input_descendant_index: usize,
    /// Any errors encountered during validation.
    errors: Vec<ValidatorError>,
    /// The full input string as last read. Not used internally but useful for
    /// debugging or reporting.
    last_input_str: String,
    /// The full schema string. Does not change.
    schema_str: String,
    /// Whether we have received the end of the input. This means that last
    /// input tree descendant index is at the end of the input.
    got_eof: bool,
}

impl Validator {
    /// Create a new ValidationZipperTree with the given schema and input strings.
    pub fn new(schema_str: &str, input_str: &str, eof: bool) -> Option<Self> {
        debug!(
            "Creating new Validator with schema length: {}, input length: {}, eof: {}",
            schema_str.len(),
            input_str.len(),
            eof
        );

        let mut schema_parser = new_markdown_parser();
        let schema_tree = match schema_parser.parse(schema_str, None) {
            Some(tree) => tree,
            None => {
                debug!("Failed to parse schema tree");
                return None;
            }
        };

        let mut input_parser = new_markdown_parser();
        let input_tree = match input_parser.parse(input_str, None) {
            Some(tree) => {
                debug!(
                    "Input tree parsed successfully with {} bytes",
                    tree.root_node().byte_range().end
                );
                tree
            }
            None => {
                debug!("Failed to parse input tree");
                return None;
            }
        };

        Some(Validator {
            input_tree,
            schema_tree,
            last_input_descendant_index: 0,
            last_schema_descendant_index: 0,
            errors: Vec::new(),
            last_input_str: input_str.to_string(),
            schema_str: schema_str.to_string(),
            got_eof: eof,
        })
    }

    /// Read new input. Updates the input tree with a new input tree for the full new input.
    ///
    /// Does not update the schema tree or change the descendant indices. You will still
    /// need to call `validate` to validate until the end of the current input
    /// (which this updates).
    pub fn read_input(&mut self, input: &str, eof: bool) -> Result<()> {
        debug!(
            "Reading new input: length={}, eof={}, current_index={}",
            input.len(),
            eof,
            self.last_input_descendant_index
        );

        // Update internal state of the last input string
        self.last_input_str = input.to_string();

        // If we already got EOF, do not accept more input
        if self.got_eof {
            return Err(anyhow!("Cannot accept more input after EOF"));
        }

        self.got_eof = eof;

        let mut input_parser = new_markdown_parser();
        // Calculate the range of new content
        let old_len = self.input_tree.root_node().byte_range().end;
        let new_len = input.len();

        // Only parse if there's actually new content
        if new_len <= old_len {
            debug!(
                "No new content to parse (new_len={}, old_len={})",
                new_len, old_len
            );
            return Ok(());
        }

        debug!(
            "Parsing incrementally: old_len={}, new_len={}",
            old_len, new_len
        );

        // Parse incrementally, providing the edit information
        let edit = tree_sitter::InputEdit {
            start_byte: old_len,
            old_end_byte: old_len,
            new_end_byte: new_len,
            start_position: self.input_tree.root_node().end_position(),
            old_end_position: self.input_tree.root_node().end_position(),
            new_end_position: {
                let lookup = LineColLookup::new(input);
                let (row, col) = lookup.get(new_len);
                tree_sitter::Point { row, column: col }
            },
        };

        self.input_tree.edit(&edit);

        match input_parser.parse(input, Some(&self.input_tree)) {
            Some(parse) => {
                self.input_tree = parse;
                // Reset both indices since the tree structure may have changed
                // We need to re-validate from the beginning
                self.last_input_descendant_index = 0;
                self.last_schema_descendant_index = 0;
                Ok(())
            }
            None => Err(anyhow!("Failed to parse input")),
        }
    }

    /// Validate the input against the schema. Validates picking up from where
    /// we left off.
    pub fn validate(&mut self) -> Result<()> {
        debug!(
            "Starting validation from input_index={}, schema_index={}",
            self.last_input_descendant_index, self.last_schema_descendant_index
        );

        // With our current understanding of state, validate until the end of the input
        let result = self.validate_nodes_from_offset_to_end_of_input();

        debug!("Validation completed. Errors found: {}", self.errors.len());
        result
    }

    pub fn report(&self) -> crate::mdschema::reports::validation_report::ValidatorReport {
        return ValidatorReport::new(self.errors.clone(), self.last_input_str.clone());
    }

    /// Validate nodes and walk until the end of the input tree, starting from
    /// the current descendant indices.
    ///
    /// Uses `validate_node` to validate each node and move the cursors forward.
    /// Directly mutates the last_descendant_indices in the struct.
    fn validate_nodes_from_offset_to_end_of_input(&mut self) -> Result<()> {
        // Walk up until the end. `self.last_input_str` will not change while
        // this is running since this blocks the thread.

        let input_tree_total_descendants = get_total_descendants(&self.input_tree);
        let schema_tree_total_descendants = get_total_descendants(&self.schema_tree);

        // Check if we've already validated everything we can
        if self.last_input_descendant_index >= input_tree_total_descendants
            && self.last_schema_descendant_index >= schema_tree_total_descendants
        {
            // Nothing to do
            debug!("No validation needed - already at end of both trees");
            return Ok(());
        }

        let mut input_cursor = self.input_tree.walk();
        input_cursor.goto_descendant(self.last_input_descendant_index);

        let mut schema_cursor = self.schema_tree.walk();
        schema_cursor.goto_descendant(self.last_schema_descendant_index);

        // Validate once starting from the current position
        print!(
            "Schema cursor expr {}",
            node_to_str(schema_cursor.node(), &self.schema_str)
        );
        print!(
            "Input cursor expr {}",
            node_to_str(input_cursor.node(), &self.last_input_str)
        );

        let (errors, (_last_input_descendant_index, _last_schema_descendant_index)) =
            validate_a_node(
                &mut input_cursor,
                &mut schema_cursor,
                &self.last_input_str,
                &self.schema_str,
                self.got_eof,
            );

        // Update to the end of the trees since we validated the entire tree
        self.last_input_descendant_index = input_tree_total_descendants;
        self.last_schema_descendant_index = schema_tree_total_descendants;
        self.errors.extend(errors);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_input_updates_last_input_str() {
        // Check that read_input updates the last_input_str correctly
        let mut validator =
            Validator::new("# Schema", "Initial input", false).expect("Failed to create validator");

        assert_eq!(validator.last_input_str, "Initial input");

        validator
            .read_input("Updated input", false)
            .expect("Failed to read input");

        assert_eq!(validator.last_input_str, "Updated input");

        // Check that it updates the tree correctly
        assert_eq!(
            validator
                .input_tree
                .root_node()
                .utf8_text(&validator.last_input_str.as_bytes())
                .expect("Failed to get input text"),
            "Updated input"
        );
    }

    #[test]
    fn test_initial_validate_with_eof_works() {
        let input = "Hello World";
        let schema = "Hello World";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");

        validator.validate().expect("Failed to validate");

        let report = validator.report();
        assert!(report.errors.is_empty());
        assert!(report.is_valid());
    }

    #[test]
    fn test_initial_validate_without_eof_incomplete_text_node() {
        let input = "Hello Wo";
        let schema = "Hello World";

        let mut validator =
            Validator::new(schema, input, false).expect("Failed to create validator");

        validator.validate().expect("Failed to validate");

        let report = validator.report();
        assert!(report.errors.is_empty());
        assert!(report.is_valid());
    }

    #[test]
    fn test_initially_empty_then_read_input_then_validate() {
        let initial_input = "";
        let schema = "Hello\n\nWorld";

        let mut validator =
            Validator::new(schema, initial_input, false).expect("Failed to create validator");

        // First validate with empty input
        validator.validate().expect("Failed to validate");
        let report = validator.report();
        assert!(report.errors.is_empty());
        assert!(report.is_valid());

        // Now read more input to complete it
        validator
            .read_input("Hello\n\nTEST World", true)
            .expect("Failed to read input");

        // Validate again
        validator
            .validate()
            .expect("Failed to validate after reading input");

        let report = validator.report();
        assert!(!report.is_valid());
    }

    #[test]
    fn test_validate_then_read_input_then_validate_again() {
        let initial_input = "Hello Wo";
        let schema = "Hello World";

        let mut validator =
            Validator::new(schema, initial_input, false).expect("Failed to create validator");

        // First validate with incomplete input
        validator.validate().expect("Failed to validate");
        let report = validator.report();
        assert!(report.errors.is_empty());
        assert!(report.is_valid());

        // Now read more input to complete it
        validator
            .read_input("Hello World", true)
            .expect("Failed to read input");

        // Validate again
        validator
            .validate()
            .expect("Failed to validate after reading input");

        let report = validator.report();
        assert!(
            report.errors.is_empty(),
            "Expected no validation errors, but found {:?}",
            report.errors
        );
        assert!(report.is_valid());
    }

    #[test]
    fn test_validation_should_fail_with_mismatched_content() {
        let schema = "# Test

    fooobar

    test

    ";
        let input = "# Test

    fooobar

    testt

    ";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");

        validator.validate().expect("Failed to validate");

        let report = validator.report();
        assert!(
            !report.errors.is_empty(),
            "Expected validation errors but found none"
        );
        assert!(
            !report.is_valid(),
            "Expected validation to fail but it passed"
        );
    }
}
