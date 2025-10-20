use anyhow::{anyhow, Result};
use line_col::LineColLookup;
use tree_sitter::{Tree, TreeCursor};

use crate::mdschema::{
    reports::{errors::ValidatorError, validation_report::ValidatorReport},
    validator::{node_validator::validate_a_node, utils::new_markdown_parser},
};

/// A Validator implementation that uses a zipper tree approach to validate
/// an input Markdown document against a markdown schema treesitter tree.
pub struct Validator {
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

impl Validator {
    /// Create a new ValidationZipperTree with the given schema and input strings.
    pub fn new(schema_str: &str, input_str: &str, eof: bool) -> Option<Self> {
        let mut schema_parser = new_markdown_parser();
        let schema_tree = match schema_parser.parse(schema_str, None) {
            Some(tree) => tree,
            None => return None,
        };

        let mut input_parser = new_markdown_parser();
        let input_tree = match input_parser.parse(input_str, None) {
            Some(tree) => tree,
            None => return None,
        };

        Some(Validator {
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
    pub fn read_input(&mut self, input: &str, eof: bool) -> Result<()> {
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
            return Ok(());
        }

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
                Ok(())
            }
            None => Err(anyhow!("Failed to parse input")),
        }
    }

    /// Validate the input against the schema. Validates picking up from where
    /// we left off.
    pub fn validate(&mut self) -> Result<()> {
        // With our current understanding of state, validate until the end of the input
        self.validate_nodes_from_offset_to_end_of_input()
    }

    pub fn report(&self) -> crate::mdschema::reports::validation_report::ValidatorReport {
        return ValidatorReport::new(self.errors.clone(), self.last_input_str.clone());
    }

    /// Validate nodes and walk until the end of the input tree, starting from
    /// the current offsets.
    ///
    /// Uses `validate_node` to validate each node and move the cursors forward.
    /// Directly mutates the last_offsets in the struct.
    fn validate_nodes_from_offset_to_end_of_input(&mut self) -> Result<()> {
        // Walk up until the end. `self.last_input_str` will not change while
        // this is running since this blocks the thread.
        let last_input_str_len = self.last_input_str.len();

        let mut input_cursor =
            match Self::get_cursor_at_offset(&self.input_tree, self.last_input_tree_offset) {
                Some(cursor) => cursor,
                None => return Err(anyhow!("Failed to get input cursor at offset")),
            };

        let mut schema_cursor =
            match Self::get_cursor_at_offset(&self.schema_tree, self.last_schema_tree_offset) {
                Some(cursor) => cursor,
                None => return Err(anyhow!("Failed to get schema cursor at offset")),
            };

        while self.last_input_tree_offset < last_input_str_len {
            // We may cause a shift to the current treecursors inside ourself by
            // calling this, but it is important that we "commit" the change by
            // actually updating the offsets after validating.
            let (errors, (last_schema_tree_offset, last_input_tree_offset)) = validate_a_node(
                &mut input_cursor,
                &mut schema_cursor,
                &self.last_input_str,
                &self.schema_str,
            );

            self.last_schema_tree_offset = last_schema_tree_offset;
            self.last_input_tree_offset = last_input_tree_offset;

            self.errors.extend(errors);
        }

        Ok(())
    }

    /// Get a TreeCursor at the correct offset.
    fn get_cursor_at_offset(tree: &'_ Tree, offset: usize) -> Option<TreeCursor<'_>> {
        let mut cursor = tree.walk();

        // Move to the correct offset
        if cursor.goto_first_child_for_byte(offset).is_some() {
            Some(cursor)
        } else {
            None
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_walks_on_validate() {
//         let source = "# Heading\n\nSome **bold** text.";

//         let mut validation_zipper_tree =
//             Validator::new("# Heading\n\nSome **bold** text.", source, true).unwrap();

//         assert!(validation_zipper_tree.last_input_tree_offset == 0);

//         validation_zipper_tree.validate().unwrap();

//         assert!(validation_zipper_tree.last_input_tree_offset == source.len());

//         let report = validation_zipper_tree.report();

//         assert!(report.errors.is_empty());
//         assert_eq!(report.source_content, source);
//     }

//     #[test]
//     fn test_detects_literal_match() {
//         let schema_str = "**strong**";
//         let input_str = "**strong**";

//         let mut validation_zipper_tree = Validator::new(schema_str, input_str, true).unwrap();
//         validation_zipper_tree.validate().unwrap();
//         let report = validation_zipper_tree.report();
//         assert!(report.errors.is_empty());
//     }

//     #[test]
//     fn test_detects_literal_mismatch() {
//         let schema_str = "**strong**";
//         let input_str = "**bold**";
//         let mut validation_zipper_tree = Validator::new(schema_str, input_str, true).unwrap();

//         validation_zipper_tree.validate().unwrap();
//         let report = validation_zipper_tree.report();
//         assert!(!report.errors.is_empty());
//         assert_eq!(
//             report.errors[0].message,
//             "Literal mismatch: expected '**strong**', found '**bold**'"
//         );
//         assert_eq!(report.source_content, input_str);
//         assert_eq!(report.errors.len(), 1);
//         assert_eq!(report.errors[0].byte_start, 0);
//         assert_eq!(report.errors[0].byte_end, 8);
//         assert_eq!(
//             report.errors[0].message,
//             "Literal mismatch: expected '**strong**', found '**bold**'"
//         );
//     }
// }
