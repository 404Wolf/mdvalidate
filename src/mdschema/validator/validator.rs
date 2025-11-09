use line_col::LineColLookup;
use log::debug;
use tree_sitter::Tree;

use crate::mdschema::{
    reports::errors::{Error, ParserError, SchemaViolationError},
    validator::{node_validators::validate_text_node, utils::new_markdown_parser},
};

/// A Validator implementation that uses a zipper tree approach to validate
/// an input Markdown document against a markdown schema treesitter tree.
pub struct Validator {
    /// The current input tree. When read_input is called, this is replaced with a new tree.
    pub input_tree: Tree,
    /// The schema tree, which does not change after initialization.
    pub schema_tree: Tree,
    /// The last descendant index we validated up to in the schema tree. In preorder.
    last_schema_descendant_index: usize,
    /// The last descendant index we validated up to in the input tree. In preorder.
    last_input_descendant_index: usize,
    /// Any errors encountered during validation.
    errors: Vec<Error>,
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

    /// Get all the errors that we have encountered
    pub fn errors(&self) -> Vec<Error> {
        self.errors.clone()
    }

    /// Read new input. Updates the input tree with a new input tree for the full new input.
    ///
    /// Does not update the schema tree or change the descendant indices. You will still
    /// need to call `validate` to validate until the end of the current input
    /// (which this updates).
    pub fn read_input(&mut self, input: &str, eof: bool) -> Result<(), Error> {
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
            return Err(Error::ParserError(ParserError::ReadAfterGotEOF));
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
            // If we're now at EOF, reset indices so we re-validate text nodes
            // that were previously skipped due to not being at EOF
            if eof {
                // TODO: This feels wrong
                debug!("EOF reached with no new content, resetting indices for final validation");
                self.last_input_descendant_index = 0;
                self.last_schema_descendant_index = 0;
            }
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
            None => Err(Error::ParserError(ParserError::TreesitterError)),
        }
    }

    /// Validates the input markdown against the schema by traversing both trees
    /// in parallel to the ends.
    ///
    /// This method performs a breadth-first traversal of both the input and
    /// schema trees simultaneously, comparing nodes at each level. It uses a
    /// work queue of (input_index, schema_index) pairs to track which nodes
    /// need validation. For each pair:
    ///
    /// 1. **Text nodes** (base case): If schema node is text, directly compare it with input using `validate_text_node`
    /// 2. **Parent nodes**: Collect all child pairs and add them to the validation queue
    /// 3. **Mismatch detection**: Reports errors when child counts differ (only if EOF received)
    /// 4. **Progressive validation**: Starts from the last validated position (`last_input_descendant_index`,
    ///    `last_schema_descendant_index`) and continues until all nodes are processed
    ///
    /// The method mutates `self.errors` to accumulate validation errors and updates the descendant
    /// indices to track validation progress, enabling incremental validation on subsequent calls.
    ///
    /// - Uses tree cursors positioned at the last validated descendant indices
    /// - Maintains a stack of (input_idx, schema_idx) pairs representing nodes to validate
    /// - When child counts mismatch, only reports error if `got_eof` is true (allowing partial validation)
    /// - Updates `last_input_descendant_index` and `last_schema_descendant_index` after completion
    pub fn validate(&mut self) {
        // Important! These are constructed from the root, so if we get
        // descendant index off of them, it should be 0.
        let mut input_cursor = self.input_tree.walk();
        let input_root_node = input_cursor.node();
        input_cursor.goto_descendant(self.last_input_descendant_index);

        let mut schema_cursor = self.schema_tree.walk();
        let schema_root_node = schema_cursor.node();
        schema_cursor.goto_descendant(self.last_schema_descendant_index);

        // Start with the root nodes
        let mut child_pairs_to_validate = vec![(
            input_cursor.descendant_index(),
            schema_cursor.descendant_index(),
        )];

        while let Some((input_idx, schema_idx)) = child_pairs_to_validate.pop() {
            input_cursor.reset(input_root_node);
            schema_cursor.reset(schema_root_node);

            input_cursor.goto_descendant(input_idx);
            schema_cursor.goto_descendant(schema_idx);

            debug!(
                "Validating node pair: input_index={} [{}], schema_index={} [{}]",
                input_cursor.descendant_index(),
                input_cursor.node().kind(),
                schema_cursor.descendant_index(),
                schema_cursor.node().kind()
            );

            let input_node = input_cursor.node();
            let schema_node = schema_cursor.node();

            // If they are both text, directly compare them. This is a "base
            // case," where we do not need to do any special logic.
            if schema_node.kind() == "text" {
                self.errors.extend(validate_text_node(
                    &input_node,
                    input_cursor.descendant_index(),
                    &schema_node,
                    &self.last_input_str,
                    &self.schema_str,
                    self.got_eof,
                    &input_node,
                ));

                continue;
            }

            // Otherwise, look at their children;
            // If the children of the schema node contains a matcher among
            // text nodes, and the input node is just text, we validate the
            // matcher using our matcher helper. It takes care of prefix/suffix
            // matching as well.
            let schema_children_has_code_node = schema_node
                .children(&mut schema_cursor.clone())
                .any(|child| child.kind() == "code_span");

            if schema_children_has_code_node && input_node.kind() == "text" {
                debug!(
                    "Validating matcher node at input_index={}, schema_index={}",
                    input_cursor.descendant_index(),
                    schema_cursor.descendant_index()
                );

                // Collect schema node children for validation
                let schema_children: Vec<_> = schema_node.children(&mut schema_cursor.clone()).collect();
                schema_cursor.goto_parent(); // Reset cursor after children iteration

                // Validate the input text against the matchers in the schema
                self.errors.extend(
                    crate::mdschema::validator::node_validators::validate_matcher_node(
                        &input_node,
                        input_cursor.descendant_index(),
                        &schema_children,
                        &self.last_input_str,
                        &self.schema_str,
                    ),
                );

                continue;
            }

            // If there are no code nodes in the schema children, then it
            // may be a mix of nodes we must recurse on.
            // iterate over the children of both the schema and input nodes
            // in order using the walker, and push them to

            // Note that we crawl the input and schema nodes at the same
            // pace, and can zip them since we made sure the schema node
            // had no matchers in it.

            // We store the descendant indices of the nodes we will need to
            // validate, relative to the root nodes.

            // At this point, if the number of children differ, we can already
            // raise an error - but only if we've received EOF. Otherwise, we're
            // still waiting for more input.
            if input_node.child_count() != schema_node.child_count() {
                if self.got_eof {
                    self.errors.push(Error::SchemaViolation(
                        SchemaViolationError::ChildrenLengthMismatch(
                            input_cursor.descendant_index(),
                            schema_cursor.descendant_index(),
                        ),
                    ));
                }
                // But we can still try to validate the common children
            }

            debug!(
                "Currently at input_index={}, schema_index={}: input_child_count={}, schema_child_count={}",
                input_cursor.descendant_index(),
                schema_cursor.descendant_index(),
                input_node.child_count(),
                schema_node.child_count()
            );

            // Collect children to validate
            if input_cursor.goto_first_child() && schema_cursor.goto_first_child() {
                debug!(
                    "Queued first child pair for validation: input_index={}, schema_index={}",
                    input_cursor.descendant_index(),
                    schema_cursor.descendant_index()
                );

                // Add first child pair
                child_pairs_to_validate.push((
                    input_cursor.descendant_index(),
                    schema_cursor.descendant_index(),
                ));

                // Then crawl their siblings and collect pairs
                loop {
                    let input_had_sibling = input_cursor.goto_next_sibling();
                    let schema_had_sibling = schema_cursor.goto_next_sibling();

                    if input_had_sibling && schema_had_sibling {
                        child_pairs_to_validate.push((
                            input_cursor.descendant_index(),
                            schema_cursor.descendant_index(),
                        ));
                        debug!(
                            "Queued child pair for validation: input_index={}, schema_index={}",
                            input_cursor.descendant_index(),
                            schema_cursor.descendant_index()
                        );
                    } else {
                        // One or both have no more siblings, stop
                        debug!("No more siblings to process in current nodes");
                        break;
                    }
                }

                // Go back to parent for next iteration
                input_cursor.goto_parent();
                schema_cursor.goto_parent();
            }
        }

        // Update the last descendant indices to the end of the trees
        self.last_input_descendant_index = input_cursor.descendant_index();
        self.last_schema_descendant_index = schema_cursor.descendant_index();
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

        validator.validate();

        let errors = validator.errors();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_initial_validate_without_eof_incomplete_text_node() {
        let input = "Hello Wo";
        let schema = "Hello World";

        let mut validator =
            Validator::new(schema, input, false).expect("Failed to create validator");

        validator.validate();

        let errors = validator.errors();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_initially_empty_then_read_input_then_validate() {
        let initial_input = "";
        let schema = "Hello\n\nWorld";

        let mut validator =
            Validator::new(schema, initial_input, false).expect("Failed to create validator");

        // First validate with empty input
        validator.validate();
        let errors = validator.errors();
        eprintln!(
            "Errors after first validate (should be empty): {:?}",
            errors
        );
        assert!(errors.is_empty());

        // Now read more input to complete it
        validator
            .read_input("Hello\n\nTEST World", true)
            .expect("Failed to read input");

        // Validate again
        validator.validate();

        let report = validator.errors();
        eprintln!(
            "Errors after second validate (should have errors): {:?}",
            report
        );
        assert!(
            !report.is_empty(),
            "Expected validation errors, but found none"
        );
    }

    #[test]
    fn test_validate_then_read_input_then_validate_again() {
        let initial_input = "Hello Wo";
        let schema = "Hello World";

        let mut validator =
            Validator::new(schema, initial_input, false).expect("Failed to create validator");

        // First validate with incomplete input
        validator.validate();
        let report = validator.errors();
        assert!(report.is_empty());

        // Now read more input to complete it
        validator
            .read_input("Hello World", true)
            .expect("Failed to read input");

        // Validate again
        validator.validate();

        let errors = validator.errors();
        assert!(
            errors.is_empty(),
            "Expected no validation errors, but found {:?}",
            errors
        );
    }

    #[test]
    fn test_validation_should_fail_with_mismatched_content() {
        let schema = "# Test\n\nfooobar\n\ntest\n";
        let input = "# Test\n\nfooobar\n\ntestt\n";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");

        validator.validate();

        let errors = validator.errors();
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, _)) => {}
            _ => panic!("Expected TextMismatch error, got {:?}", errors[0]),
        }
    }

    #[test]
    fn test_validation_passes_with_different_whitespace() {
        let schema = "# Test\n\nfooobar\n\ntest\n";
        let input = "# Test\n\n\nfooobar\n\n\n\ntest\n\n";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");

        validator.validate();

        let errors = validator.errors();
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

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");

        validator.validate();

        let errors = validator.errors();
        assert!(
            !errors.is_empty(),
            "Expected validation errors but found none"
        );
    }

    #[test]
    fn test_when_different_node_counts_and_got_eof_reports_error() {
        let schema = "# Test\n\nfooobar\n\ntest\n";
        let input = "# Test\n\nfooobar\n";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");

        validator.validate();

        let errors = validator.errors();
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch(_, _)) => {}
            _ => panic!("Expected ChildrenLengthMismatch error, got {:?}", errors[0]),
        }
    }

    #[test]
    fn test_two_lists_where_second_item_has_different_content_than_schema() {
        let schema = "- Item 1\n- Item 2\n";
        let input = "- Item 1\n- Item X\n";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");

        validator.validate();

        let errors = validator.errors();
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, _)) => {}
            _ => panic!("Expected NodeContentMismatch error, got {:?}", errors[0]),
        }
    }
}
