use std::sync::Mutex;

use tree_sitter::{Parser, Tree, TreeCursor};

use crate::mdschema::{reports::ValidatorReport, errors::ValidatorError};
use crate::mdschema::validator::Validator;

/// A Validator implementation that uses a zipper tree approach to validate
/// an input Markdown document against a markdown schema treesitter tree.
pub struct ValidationZipperTree {
    state: Mutex<ValidationZipperTreeState>,
    input_content: String,
    filename: String,
}

struct ValidationZipperTreeState {
    input_tree: Tree,
    schema_tree: Tree,
    last_schema_tree_offset: usize,
    last_input_tree_offset: usize,
    errors: Vec<ValidatorError>,
}

impl super::Validator for ValidationZipperTree {
    /// Create a new ValidationZipperTree with the given schema and input strings.
    fn new(schema_str: &str, input_str: &str) -> Self {
        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema_str, None).unwrap();

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input_str, None).unwrap();

        ValidationZipperTree {
            state: Mutex::new(ValidationZipperTreeState {
                input_tree,
                schema_tree,
                last_input_tree_offset: 0,
                last_schema_tree_offset: 0,
                errors: Vec::new(),
            }),
            input_content: input_str.to_string(),
            filename: "input.md".to_string(),
        }
    }

    /// Read new input and update the input tree.
    fn read_input(&self, input: &str) {
        let mut state = self.state.lock().unwrap();

        let mut input_parser = new_markdown_parser();
        state.input_tree = input_parser.parse(input, Some(&state.input_tree)).unwrap();
        state.last_input_tree_offset = input.len();
    }

    /// Validate the input against the schema.
    fn validate(&self) -> crate::mdschema::ValidatorReport {
        self.validate_to_most_recent_offset();
        let state = self.state.lock().unwrap();
        crate::mdschema::ValidatorReport::new(
            state.errors.clone(),
            self.input_content.clone(),
            self.filename.clone(),
        )
    }
}

impl ValidationZipperTree {
    fn validate_node(&self) {
        self.walk_to_most_recent_offset();
    }

    fn validate_to_most_recent_offset(&self) {
        // For now we just assume no validation errors and walk to the end.
        self.walk_to_most_recent_offset();
    }

    /// Walk the input and schema trees to the most recent offsets.
    fn walk_to_most_recent_offset(&self) {
        let state = self.state.lock().unwrap();

        let mut input_tree_cursor = state.input_tree.walk();
        goto_tree_offset(&mut input_tree_cursor, state.last_input_tree_offset);

        let mut schema_tree_cursor = state.schema_tree.walk();
        goto_tree_offset(&mut schema_tree_cursor, state.last_schema_tree_offset);
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
    use crate::mdschema::Validator;

    #[test]
    fn test_goto_tree_offset() {
        let source = "# Heading\n\nSome **bold** text.";
        let mut parser = new_markdown_parser();
        let tree = parser.parse(source, None).unwrap();
        let mut cursor = tree.walk();

        assert!(goto_tree_offset(&mut cursor, 2)); // Inside "Heading"
        assert_eq!(cursor.node().kind(), "atx_heading");

        assert!(goto_tree_offset(&mut cursor, 15)); // Inside "Some"
        assert_eq!(cursor.node().kind(), "paragraph");

        assert!(goto_tree_offset(&mut cursor, 22)); // Inside "**bold**"
        assert_eq!(cursor.node().kind(), "strong_emphasis");

        assert!(!goto_tree_offset(&mut cursor, 100)); // Out of bounds
    }

    #[test]
    fn test_validator_with_matching_schema_and_input() {
        let input = "# Title\n\nbody";
        let schema = "# Title\n\nbody";
        
        let validator = ValidationZipperTree::new(schema, input);
        let report = validator.validate();
        
        assert!(report.is_valid, "Validator should report as valid when input matches schema");
        assert!(report.errors.is_empty(), "No errors should be present when input matches schema");
        assert_eq!(report.error_count(), 0, "Error count should be zero");
        assert_eq!(report.warning_count(), 0, "Warning count should be zero");
    }
}
