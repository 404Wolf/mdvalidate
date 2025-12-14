use line_col::LineColLookup;
use serde_json::Value;
use tree_sitter::Tree;

use crate::mdschema::validator::{
    errors::{Error, ParserError},
    nodes::NodeValidator,
    state::ValidatorState,
    utils::new_markdown_parser,
};

/// A Validator implementation that uses a zipper tree approach to validate
/// an input Markdown document against a markdown schema treesitter tree.
pub struct Validator {
    /// The current input tree. When read_input is called, this is replaced with a new tree.
    pub input_tree: Tree,
    /// The schema tree, which does not change after initialization.
    pub schema_tree: Tree,
    /// The farthest reached descendant index pair (input_index, schema_index) we validated up to. In preorder.
    farthest_reached_descendant_index_pair: (usize, usize),
    state: ValidatorState,
}

impl Validator {
<<<<<<< Updated upstream
    /// Create a new ValidationZipperTree with the given schema and input strings.
    pub fn new(schema_str: &str, input_str: &str, eof: bool) -> Option<Self> {
<<<<<<< Updated upstream
=======
        debug!(
            "Creating new Validator with schema length: {}, input length: {}, eof: {}",
            schema_str.len(),
            input_str.len(),
            eof
        );

=======
    /// Create a new Validator with the given schema and input strings.
    fn new(schema_str: &str, input_str: &str, got_eof: bool) -> Option<Self> {
>>>>>>> Stashed changes
>>>>>>> Stashed changes
        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema_str, None)?;

        let mut input_parser = new_markdown_parser();
<<<<<<< Updated upstream
        let input_tree = input_parser.parse(input_str, None)?;

        let mut initial_state =
            ValidatorState::new(schema_str.to_string(), input_str.to_string(), eof);
        initial_state.set_got_eof(eof);
=======
<<<<<<< Updated upstream
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
=======
        let input_tree = input_parser.parse(input_str, None)?;

        let mut initial_state =
            ValidatorState::new(schema_str.to_string(), input_str.to_string(), got_eof);
        initial_state.set_got_eof(got_eof);
>>>>>>> Stashed changes
>>>>>>> Stashed changes

        Some(Validator {
            input_tree,
            schema_tree,
            state: initial_state,
            farthest_reached_descendant_index_pair: (0, 0),
        })
    }

<<<<<<< Updated upstream
    pub fn errors_so_far(&self) -> impl Iterator<Item = &Error> + std::fmt::Debug {
        self.state.errors_so_far().into_iter()
=======
<<<<<<< Updated upstream
    /// Get all the errors that we have encountered
    pub fn errors(&self) -> Vec<Error> {
        self.errors_so_far.iter().cloned().collect()
=======
    pub fn new_complete(schema_str: &str, input_str: &str) -> Option<Self> {
        Self::new(schema_str, input_str, true)
    }

    pub fn new_incomplete(schema_str: &str, input_str: &str) -> Option<Self> {
        Self::new(schema_str, input_str, false)
    }

    pub fn errors_so_far(&self) -> impl Iterator<Item = &Error> + std::fmt::Debug {
        self.state.errors_so_far().into_iter()
>>>>>>> Stashed changes
>>>>>>> Stashed changes
    }

    pub fn matches_so_far(&self) -> &Value {
        self.state.matches_so_far()
    }

    /// Read new input. Updates the input tree with a new input tree for the full new input.
    ///
    /// Does not update the schema tree or change the descendant indices. You will still
    /// need to call `validate` to validate until the end of the current input
    /// (which this updates).
<<<<<<< Updated upstream
    pub fn read_input(&mut self, input: &str, got_eof: bool) -> Result<(), Error> {
=======
<<<<<<< Updated upstream
    pub fn read_input(&mut self, input: &str, eof: bool) -> Result<(), Error> {
        debug!(
            "Reading new input: length={}, eof={}, current_index={}",
            input.len(),
            eof,
            self.last_input_descendant_index
        );

=======
    fn read_input(&mut self, input: &str, got_eof: bool) -> Result<(), Error> {
>>>>>>> Stashed changes
>>>>>>> Stashed changes
        // Update internal state of the last input string
        self.state.set_last_input_str(input.to_string());

        // If we already got EOF, do not accept more input
        if self.state.got_eof() {
            return Err(Error::ParserError(ParserError::ReadAfterGotEOF));
        }

        self.state.set_got_eof(got_eof);

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

        // We need to call edit() to inform the tree about changes in the source text
        // before reusing it for incremental parsing. This allows tree-sitter to
        // efficiently reparse only the modified portions of the tree. (it
        // requires the state to match the new text)
        self.input_tree.edit(&edit); // edit doesn't know about the new text content!

        let mut input_parser = new_markdown_parser();
        match input_parser.parse(input, Some(&self.input_tree)) {
            Some(parse) => {
                self.input_tree = parse;
                Ok(())
            }
            None => Err(Error::ParserError(ParserError::TreesitterError)),
        }
    }

    pub fn read_final_input(&mut self, input: &str) -> Result<(), Error> {
        self.read_input(input, true)
    }

    pub fn read_more_input(&mut self, input: &str) -> Result<(), Error> {
        self.read_input(input, false)
    }

    /// Validates the input markdown against the schema by traversing both trees
    /// in parallel to the ends, starting from where we last left off.
    pub fn validate(&mut self) {
        let mut input_cursor = self.input_tree.walk();
        input_cursor.goto_descendant(self.farthest_reached_descendant_index_pair.0);

        let mut schema_cursor = self.schema_tree.walk();
        schema_cursor.goto_descendant(self.farthest_reached_descendant_index_pair.1);

        let mut node_validator = NodeValidator::new(&mut self.state, input_cursor, schema_cursor);
        let (new_input_index, new_schema_index) = node_validator.validate();

        self.farthest_reached_descendant_index_pair = (new_input_index, new_schema_index);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::mdschema::validator::errors::{SchemaError, SchemaViolationError};

    use super::*;

    /// Helper function to create a validator and run validation, returning errors
    /// Panics if validator creation fails
    fn do_validate(schema: &str, input: &str, eof: bool) -> (Vec<Error>, Value) {
        let mut validator = Validator::new(schema, input, eof).expect("Failed to create validator");
        validator.validate();
        (
            validator.errors_so_far().cloned().collect(),
            validator.state.matches_so_far().clone(),
        )
    }

    /// Helper function to create a validator for incremental testing
    /// Returns the validator for further manipulation
    fn get_validator_for_incremental(schema: &str, input: &str, eof: bool) -> Validator {
        Validator::new(schema, input, eof).expect("Failed to create validator")
    }

    #[test]
    fn test_read_input_updates_last_input_str() {
        // Check that read_input updates the last_input_str correctly
        let mut validator = get_validator_for_incremental("# Schema", "Initial input", false);

        assert_eq!(validator.state.last_input_str(), "Initial input");

        validator
            .read_input("Updated input", false)
            .expect("Failed to read input");

        assert_eq!(validator.state.last_input_str(), "Updated input");

        // Check that it updates the tree correctly
        assert_eq!(
            validator
                .input_tree
                .root_node()
                .utf8_text(&validator.state.last_input_str().as_bytes())
                .expect("Failed to get input text"),
            "Updated input"
        );
    }

    #[test]
    fn test_initial_validate_with_eof_works() {
        let input = "Hello World";
        let schema = "Hello World";

        let (errors, value) = do_validate(schema, input, true);
        assert!(errors.is_empty());
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_initial_validate_without_eof_incomplete_text_node() {
        let input = "Hello Wo";
        let schema = "Hello World";

        let (errors, value) = do_validate(schema, input, false);
        assert!(errors.is_empty());
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_initially_empty_then_read_input_then_validate() {
        let initial_input = "";
        let schema = "Hello\n\nWorld";

        let mut validator = get_validator_for_incremental(schema, initial_input, false);

        // First validate with empty input
        validator.validate();
<<<<<<< Updated upstream
        let errors = validator.errors_so_far();
        assert_eq!(errors.count(), 0,);
=======
<<<<<<< Updated upstream
        let errors = validator.errors();
        eprintln!(
            "Errors after first validate (should be empty): {:?}",
            errors
        );
        assert!(errors.is_empty());
=======
        let errors = validator.errors_so_far();
        assert_eq!(errors.count(), 0);
>>>>>>> Stashed changes
>>>>>>> Stashed changes

        // Now read more input to complete it
        validator
            .read_input("Hello\n\nWorld", true)
            .expect("Failed to read input");

        // Validate again
        validator.validate();

<<<<<<< Updated upstream
        let errors = validator.errors_so_far();
        assert_eq!(errors.count(), 0,);
=======
<<<<<<< Updated upstream
        let report = validator.errors();
        eprintln!(
            "Errors after second validate (should have errors): {:?}",
            report
        );
        assert!(
            !report.is_empty(),
            "Expected validation errors, but found none"
        );
=======
        let errors = validator.errors_so_far();
        assert_eq!(errors.count(), 0);
>>>>>>> Stashed changes
>>>>>>> Stashed changes
    }

    #[test]
    fn test_validate_then_read_input_then_validate_again() {
        let initial_input = "Hello Wo";
        let schema = "Hello World";

        let mut validator = get_validator_for_incremental(schema, initial_input, false);

        // First validate with incomplete input
        validator.validate();
        let errors = validator.errors_so_far();
        assert_eq!(errors.count(), 0);

        // Now read more input to complete it
        validator
            .read_input("Hello World", true)
            .expect("Failed to read input");

        // Validate again
        validator.validate();

        let errors = validator.errors_so_far();
        assert_eq!(errors.count(), 0);
    }

    #[test]
    fn test_validation_should_fail_with_mismatched_content() {
        let schema = "# Test\n\nfooobar\n\ntest\n";
        let input = "# Test\n\nfooobar\n\ntestt\n";

        let (errors, _) = do_validate(schema, input, true);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch { .. }) => {}
            _ => panic!("Expected TextMismatch error, got {:?}", errors[0]),
        }
    }

    #[test]
    fn test_validation_passes_with_different_whitespace() {
        let schema = "# Test\n\nfooobar\n\ntest\n";
        let input = "# Test\n\n\nfooobar\n\n\n\ntest\n\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_validation_should_fail_with_mismatched_content_using_escaped_newlines() {
        let schema = "# Test\n\nfooobar\n\ntest\n";
        let input = "# Test\n\nfooobar\n\ntestt\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            !errors.is_empty(),
            "Expected validation errors but found none"
        );
    }

    #[test]
    fn test_when_different_node_counts_and_got_eof_reports_error() {
        let schema = "# Test\n\nfooobar\n\ntest\n";
        let input = "# Test\n\nfooobar\n";

        let (errors, _) = do_validate(schema, input, true);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
                schema_index,
                input_index: _,
                expected,
                actual,
            }) => {
                assert_eq!(*expected, 2);
                assert_eq!(*actual, 3);
<<<<<<< Updated upstream
                assert_eq!(*parent_index, 9); // TODO: is this right?
=======
<<<<<<< Updated upstream
                assert_eq!(*parent_index, 7);
=======
                assert_eq!(*schema_index, 9); // TODO: is this right?
>>>>>>> Stashed changes
>>>>>>> Stashed changes
            }
            _ => panic!("Expected ChildrenLengthMismatch error, got {:?}", errors[0]),
        }
    }

    #[test]
    fn test_two_lists_where_second_item_has_different_content_than_schema() {
        let schema = "- Item 1\n- Item 2\n";
        let input = "- Item 1\n- Item X\n";

        let (errors, _) = do_validate(schema, input, true);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch { .. }) => {}
            _ => panic!("Expected NodeContentMismatch error, got {:?}", errors[0]),
        }
    }

    #[test]
    fn test_repeated_list_matcher() {
        let schema = r"- `item:/\d+/`{,}";
        let input = r"
- 1
- 2
- 3
";

        let (errors, matches) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "expected no errors, but found {:?}",
            errors
        );

        // The matcher with + should collect all matches in an array
        let items = match matches.get("item") {
            Some(value) => match value.as_array() {
                Some(array) => array,
                None => panic!("Expected 'item' to be an array but got: {:?}", value),
            },
            None => panic!("Expected 'item' key in matches but got: {:?}", matches),
        };

        assert_eq!(items.len(), 3);
        assert_eq!(items[0], "1");
        assert_eq!(items[1], "2");
        assert_eq!(items[2], "3");
    }

    #[test]
    fn test_simple_matcher_validates_correctly() {
        let schema = "# Hi `name:/[A-Z][a-z]+/`\n";
        let input = "# Hi Wolf\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_matcher_fails_with_invalid_name() {
        let schema = "# Hi `name:/[A-Z][a-z]+/`\n";
        let input = "# Hi wolf\n";

        let (errors, matches) = do_validate(schema, input, true);
        assert!(
            !errors.is_empty(),
            "Expected validation error for lowercase name"
        );

        assert_eq!(matches.get("name"), None);
    }

    #[test]
    fn test_matcher_with_prefix_and_suffix() {
        let schema = r"Hello `name:/[A-Z][a-z]+/` there!
";
        let input = "Hello Wolf there!\n";

        let (errors, matches) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
        assert_eq!(matches.get("name").unwrap(), "Wolf");
    }

    #[test]
    fn test_matcher_with_prefix_and_suffix_and_number() {
        let schema = r"Hello `name:/[A-Z][a-z]+/` there!

Version: `ver:/[0-9]+\.[0-9]+\.[0-9]+/`
";
        let input = "Hello Wolf there!\n\nVersion: 1.2.3\n";

        let (errors, matches) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
        assert_eq!(matches.get("name").unwrap(), "Wolf");
        assert_eq!(matches.get("ver").unwrap(), "1.2.3");
    }

    #[test]
    fn test_matcher_with_prefix_and_suffix_and_number_with_prefix() {
        let schema = r"Hello `name:/[A-Z][a-z]+/` there!

Version: `ver:/[0-9]+\.[0-9]+\.[0-9]+/`
";
        let input = "Hello Wolf there!\n\nVersion: 1.2.3\n";

        let (errors, matches) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
        assert_eq!(matches.get("name").unwrap(), "Wolf");
        assert_eq!(matches.get("ver").unwrap(), "1.2.3");
    }

    #[test]
    fn test_matcher_with_wrong_suffix() {
        let schema = "Hello `name:/[A-Z][a-z]+/` there!\n";
        let input = "Hello Wolf here!\n"; // "here" instead of "there"

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            !errors.is_empty(),
            "Expected validation error for wrong suffix"
        );
    }

    #[test]
    fn test_matcher_in_heading_with_other_text() {
        let schema = "# Assignment `number:/\\d+/` test\n";
        let input = "# Assignment 1 test\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_nested_lists_validate() {
        let schema = r"- Item 1
  - Nested item
- Item 2
";
        let input = r"- Item 1
  - Nested item
- Item 2
";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_nested_lists_with_mismatch() {
        let schema = r"
- Item 1
  - Nested item
- Item 2
";
        let input = r"
- Item 1
  - Wrong item
- Item 2
"; // "Wrong" instead of "Nested"

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            !errors.is_empty(),
            "Expected validation error for nested list mismatch"
        );
    }

    #[test]
    fn test_multiple_headings() {
        let schema = "# Heading 1\n\n## Heading 2\n";
        let input = "# Heading 1\n\n## Heading 2\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_code_block_validation() {
        let schema = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\n";
        let input = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_code_block_with_different_content_fails() {
        let schema = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\n";
        let input = "```rust\nfn main() {\n    println!(\"World\");\n}\n```\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            !errors.is_empty(),
            "Expected validation error for different code content"
        );
    }

    #[test]
    fn test_blockquote_validation() {
        let schema = "> This is a blockquote\n";
        let input = "> This is a blockquote\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_link_validation() {
        let schema = "[Link text](https://example.com)\n";
        let input = "[Link text](https://example.com)\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_link_with_different_url_fails() {
        let schema = "[Link text](https://example.com)\n";
        let input = "[Link text](https://different.com)\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            !errors.is_empty(),
            "Expected validation error for different URL"
        );
    }

    #[test]
    fn test_empty_document() {
        let schema = "";
        let input = "";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_one_line_with_matchers() {
        let schema = "# Assignment `num:/\\d+/`\n";
        let input = "# Assignment 7\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_paragraph_with_inline_code() {
        let schema = "This is a `code` example.\n";
        let input = "This is a `code` example.\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }
    #[test]
    fn test_only_whitespace() {
        let schema = "\n\n\n";
        let input = "\n\n\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_incremental_reading_with_matcher() {
        let schema = "# Hi `name:/[A-Z][a-z]+/`\n";
        let initial_input = "# Hi ";

        let mut validator = get_validator_for_incremental(schema, initial_input, false);

        validator.validate();
        assert!(validator.errors_so_far().count() == 0);

        // Complete the input
        validator
            .read_input("# Hi Wolf\n", true)
            .expect("Failed to read input");

        validator.validate();
        let errors: Vec<_> = validator.errors_so_far().collect();
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_incremental_reading_multiple_steps() {
        let schema = "# Title\n\nParagraph text\n";

        let mut validator =
            Validator::new(schema, "# ", false).expect("Failed to create validator");
        validator.validate();
        assert!(validator.errors_so_far().count() == 0);

        validator
            .read_input("# Title\n", false)
            .expect("Failed to read");
        validator.validate();
        assert!(validator.errors_so_far().count() == 0);

        validator
            .read_input("# Title\n\nParagraph text\n", true)
            .expect("Failed to read");
        validator.validate();
        let errors: Vec<_> = validator.errors_so_far().collect();
        assert!(
            errors.is_empty(),
            "Expected no errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_cannot_read_after_eof() {
        let schema = "Test\n";
        let mut validator =
            Validator::new(schema, "Test\n", true).expect("Failed to create validator");

        let result = validator.read_input("More text\n", false);
        assert!(
            result.is_err(),
            "Should not be able to read input after EOF"
        );
    }

    #[test]
    fn test_matcher_at_start_of_line() {
        let schema = "`word:/\\w+/` is the first word\n";
        let input = "Hello is the first word\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_matcher_at_end_of_line() {
        let schema = "The last word is `word:/\\w+/`\n";
        let input = "The last word is Hello\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_mixed_formatting() {
        let schema = "This is **bold** and *italic* and `code`.\n";
        let input = "This is **bold** and *italic* and `code`.\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_matcher_with_optional_groups() {
        let schema = "`name:/[A-Z][a-z]+(\\s[A-Z][a-z]+)?/`\n";
        let input = "Wolf Mermelstein\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_matcher_entire_line() {
        let schema = "`line:/.+/`\n";
        let input = "This is the entire line content\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_matcher_entire_line_with_optional_groups() {
        let schema = "`line:/[A-Z][a-z]+(\\s[A-Z][a-z]+)?/`\n";
        let input = "Wolf Mermelstein\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_complex_document_with_wrong_list_shape() {
        let schema = r#"# Document Title

This is a paragraph with some content.

- First item is literal
- Second item ends with `name:/[A-Z][a-z]+/`
- Third item is just literal
- Fourth item has `num:/[0-9]+/` in it

Footer: `footer:/[a-z]+/`
"#;

        let input = r#"# Document Title

This is a paragraph with some content.

- First item is literal
- Second item ends with Alice
- Third item is just literal
- Fourth item has 22 in it
- Fourth item has 22 in it

Footer: goodbye
            "#;

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");

        validator.validate();

        let errors: Vec<_> = validator.errors_so_far().collect();
        match &errors[0] {
<<<<<<< Updated upstream
            Error::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch(
                actual,
                expected,
                _,
            )) => {
<<<<<<< Updated upstream
                assert_eq!(*expected, 4);
                assert_eq!(*actual, 5);
=======
                assert_eq!(*expected, 3);
                assert_eq!(*actual, 2);
                assert_eq!(*parent_index, 9);
=======
            Error::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
                actual,
                expected,
                ..
            }) => {
                assert_eq!(*expected, 4);
                assert_eq!(*actual, 5);
>>>>>>> Stashed changes
>>>>>>> Stashed changes
            }
            _ => panic!("Expected ChildrenLengthMismatch error, got {:?}", errors[0]),
        }
    }

    #[test]
    fn test_single_matcher_matches_bad_regex() {
        let schema = "`id:/test/`";
        let input = "test";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");
        validator.validate();
        assert_eq!(validator.errors_so_far().count(), 0);

        let input2 = "fhuaeifhwiuehfu";
        let mut validator =
            Validator::new(schema, input2, true).expect("Failed to create validator");
        validator.validate();
        assert_eq!(validator.errors_so_far().count(), 1);
    }

    #[test]
    fn test_multiple_matchers() {
        // The schema becomes a paragraph with multiple code nodes
        let schema = "`id:/test/` `id:/example/`";
        let input = "test example";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");
        validator.validate();

<<<<<<< Updated upstream
        let mut errors = validator.errors_so_far();
        match errors.next() {
            Some(Error::SchemaError(SchemaError::MultipleMatchersInNodeChildren(_, count))) => {
=======
<<<<<<< Updated upstream
        match errors.first() {
            Some(Error::SchemaError(SchemaError::MultipleMatchersInNodeChildren(count))) => {
                println!("Got expected MultipleMatchers error with count: {}", count);
>>>>>>> Stashed changes
                assert_eq!(*count, 2, "Expected 2 matchers");
=======
        let mut errors = validator.errors_so_far();
        match errors.next() {
            Some(Error::SchemaError(SchemaError::MultipleMatchersInNodeChildren {
                received,
                expected,
                ..
            })) => {
                assert_eq!(*expected, 2, "Expected 2 matchers");
                assert_eq!(*received, 2, "Received 2 matchers");
>>>>>>> Stashed changes
            }
            _ => panic!("Expected MultipleMatchers error but got: {:?}", errors),
        }
    }

    #[test]
    fn test_matcher_for_wrong_node_types() {
        let schema = "`id:/item1/`\n- `id:/item3/`";
        let input = "- item1\n- item2";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");
        validator.validate();
        let mut errors = validator.errors_so_far();

        match errors.next() {
            Some(Error::SchemaViolation(_)) => {}
            _ => panic!("Expected SchemaViolation error but got: {:?}", errors),
        }
    }

    #[test]
    fn test_mismatched_list_items() {
        let schema = "- `id:/item1/`\n- `id:/item3/`";
        let input = "- item1\n- item2";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");
        validator.validate();
        let mut errors = validator.errors_so_far();

<<<<<<< Updated upstream
        match errors.next() {
=======
<<<<<<< Updated upstream
        match errors.first() {
>>>>>>> Stashed changes
            Some(Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(
                _,
                expected,
            ))) => {
<<<<<<< Updated upstream
=======
                println!("Got expected NodeContentMismatch error for: {}", expected);
=======
        match errors.next() {
            Some(Error::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                expected,
                ..
            })) => {
>>>>>>> Stashed changes
>>>>>>> Stashed changes
                // The matcher pattern should be in the expected string
                assert!(
                    expected.contains("item3"),
                    "Expected error to mention 'item3' matcher"
                );
            }
            _ => panic!("Expected NodeContentMismatch error but got: {:?}", errors),
        }
    }

    #[test]
    fn test_incremental_validation_preserves_work_when_appending() {
        // This test verifies that when we incrementally add content,
        // we don't re-validate already-validated nodes (which would be wasteful)
        let schema = r#"# Title

## Section 1

Content for section 1.

## Section 2

Content for section 2.

## Section 3

Content for section 3."#;

        let input_complete = r#"# Title

## Section 1

Content for section 1.

## Section 2

Content for section 2.

## Section 3

Content for section 3."#;

        // Start with empty input
        let mut validator = Validator::new(schema, "", false).expect("Failed to create validator");

        // Incrementally add content in logical chunks
        let chunks = vec![
            "# Title\n\n",
            "# Title\n\n## Section 1\n\n",
            "# Title\n\n## Section 1\n\nContent for section 1.\n\n",
            "# Title\n\n## Section 1\n\nContent for section 1.\n\n## Section 2\n\n",
            "# Title\n\n## Section 1\n\nContent for section 1.\n\n## Section 2\n\nContent for section 2.\n\n",
            "# Title\n\n## Section 1\n\nContent for section 1.\n\n## Section 2\n\nContent for section 2.\n\n## Section 3\n\n",
            input_complete,
        ];

        for (i, chunk) in chunks.iter().enumerate() {
            let is_eof = i == chunks.len() - 1;

            let indices_before = validator.farthest_reached_descendant_index_pair;

            validator
                .read_input(chunk, is_eof)
                .expect("Failed to read input");
            validator.validate();

            let indices_after = validator.farthest_reached_descendant_index_pair;

            // Indices should advance (or stay the same if nothing new to validate)
            // They should NOT reset to 0
            if i > 0 && chunk.len() > chunks[i - 1].len() {
                // After the first chunk, indices should advance or stay the same
                assert!(
                    indices_after.0 >= indices_before.0,
                    "Input descendant index regressed after reading chunk"
                );
                assert!(
                    indices_after.1 >= indices_before.1,
                    "Schema descendant index regressed after reading chunk"
                );
            }
        }

        let errors = validator.errors_so_far();
        assert_eq!(errors.count(), 0,);
    }

    #[test]
    fn test_with_rulers() {
        let schema = "# Title\n\nSome content with a ruler below:\n\n---\n\nMore content.";
        let input = "# Title\n\nSome content with a ruler below:\n\n---\n\nMore content.";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_with_underscore_and_star_and_dash_ruler_in_same_file() {
        let schema = "# Title\n\nContent above rulers.\n\n***\n\nMore content.\n\n___\n\nEnd content.\n\n---";
        let input = "# Title\n\nContent above rulers.\n\n***\n\nMore content.\n\n___\n\nEnd content.\n\n---";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }
}
