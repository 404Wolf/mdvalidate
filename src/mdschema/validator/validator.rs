use std::collections::HashSet;

use line_col::LineColLookup;
use log::{debug, trace};
use serde_json::{json, Value};
use tree_sitter::Tree;

use crate::mdschema::validator::{
    errors::{Error, ParserError, SchemaError, SchemaViolationError},
    node_validators::{validate_matcher_node, validate_matcher_node_list, validate_text_node},
    utils::new_markdown_parser,
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
    /// The full input string as last read. Not used internally but useful for
    /// debugging or reporting.
    last_input_str: String,
    /// The full schema string. Does not change.
    schema_str: String,
    /// Whether we have received the end of the input. This means that last
    /// input tree descendant index is at the end of the input.
    got_eof: bool,
    /// Map of matches found so far.
    matches_so_far: Value,
    /// Any errors encountered during validation.
    errors_so_far: HashSet<Error>,
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
            // We revalidate some nodes multiple times if the cursor leaves off,
            // so we use a set to automatically weed out duplicates
            errors_so_far: HashSet::new(),
            last_input_str: input_str.to_string(),
            schema_str: schema_str.to_string(),
            got_eof: eof,
            matches_so_far: json!({}),
        })
    }

    /// Get all the errors that we have encountered
    pub fn errors(&self) -> Vec<Error> {
        self.errors_so_far.iter().cloned().collect()
    }

    /// Get all the matches that we have encountered
    pub fn matches(&self) -> Value {
        self.matches_so_far.clone()
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

        if eof {
            // After we got EOF, we want to validate from the parent of the latest offsets and down
            let input_tree_clone = self.input_tree.clone();
            let mut input_cursor = input_tree_clone.walk();
            let mut schema_cursor = self.schema_tree.walk();
            input_cursor.goto_descendant(self.last_input_descendant_index);
            schema_cursor.goto_descendant(self.last_schema_descendant_index);
            input_cursor.goto_parent();
            schema_cursor.goto_parent();
            self.last_input_descendant_index = input_cursor.descendant_index();
            self.last_schema_descendant_index = schema_cursor.descendant_index();
        }

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

        debug!(
            "Starting validation from input_index={} (type={}), schema_index={} (type={})",
            input_cursor.descendant_index(),
            input_cursor.node().kind(),
            schema_cursor.descendant_index(),
            schema_cursor.node().kind()
        );

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

            // Otherwise, look at their children;
            // If the children of the schema node contains a matcher among
            // text nodes, and the input node is just text OR the input node
            // has only text children, we validate the matcher using our matcher
            // helper. It takes care of prefix/suffix matching as well.
            let schema_children_code_node_count =
                children_code_node_count(&schema_node, &mut schema_cursor);

            // Schema is a list with a single entry which is a node that contains a code node.
            // This is for the case where we have `matcher`+ (with the + at the end) to indicate
            // that the matcher pattern applies for multiple consecutive list items.
            let schema_node_first_list_item_code_node_count = {
                schema_node
                    .child(0)
                    .map(|first_child| {
                        children_code_node_count(&first_child, &mut schema_cursor.clone())
                    })
                    .unwrap_or(0)
            };
            let is_schema_specified_list_node = schema_node.kind() == "tight_list"
                && schema_node.child_count() == 1
                && input_node.child_count() > 1; // When we hit the validate_matcher_list we
                                                 // are expecting multiple items, and if we
                                                 // don't get multiple items we say "hey, you
                                                 // should have used +"!
            debug!(
                "Schema node is a schema-specified list node: {}",
                is_schema_specified_list_node
            );

            // We don't allow multiple code_span children for the schema
            // since it would lead to ambiguity
            if schema_children_code_node_count > 1
                || (schema_node.kind() == "tight_list"
                    && schema_node_first_list_item_code_node_count > 0)
            {
                trace!("Schema node has multiple matcher children, reporting error");

                self.errors_so_far.insert(Error::SchemaError(
                    SchemaError::MultipleMatchersInNodeChildren(schema_children_code_node_count),
                ));
                continue;
            }

            // Check if input node is text or only has text children
            let input_is_text_only = input_node.kind() == "text"
                || (input_node.child_count() == 1
                    && input_node
                        .child(0)
                        .map(|c| c.kind() == "text")
                        .unwrap_or(false));
            trace!("Input node is text only: {}", input_is_text_only);

            // If the schema's current level's child nodes have a code node (a matcher)
            if schema_children_code_node_count == 1 && input_is_text_only {
                debug!(
                    "Validating matcher node at input_index={}, schema_index={}",
                    input_cursor.descendant_index(),
                    schema_cursor.descendant_index()
                );

                // Collect schema node children for validation
                let schema_children: Vec<_> =
                    schema_node.children(&mut schema_cursor.clone()).collect();
                schema_cursor.goto_parent(); // Reset cursor after children iteration

                // Get the actual text node to validate
                let text_node_to_validate = if input_node.kind() == "text" {
                    input_node
                } else {
                    input_node.child(0).unwrap()
                };

                // Validate the input text against the matchers in the schema
                let (errors, matches) = validate_matcher_node(
                    &mut input_cursor,
                    &mut schema_cursor,
                    &self.last_input_str,
                    &self.schema_str,
                    self.got_eof,
                );
                self.errors_so_far.extend(errors);
                self.matches_so_far
                    .as_object_mut()
                    .unwrap() // Safe unwrap since matches is always an object
                    .extend(matches.as_object().unwrap().clone());

                continue;
            } else if is_schema_specified_list_node {
                // move the cursor to the first list item
                schema_cursor.goto_first_child(); // list_item
                schema_cursor.goto_next_sibling(); // list_marker
                schema_cursor.goto_next_sibling(); // paragraph
                assert_eq!(schema_cursor.node().kind(), "paragraph");

                let (errors, matches) = validate_matcher_node_list(
                    &mut input_cursor,
                    &mut schema_cursor,
                    &self.last_input_str,
                    &self.schema_str,
                    self.got_eof,
                );

                // move the cursor back to the parent list node
                schema_cursor.goto_parent();
                assert_eq!(schema_cursor.node().kind(), "list");

                self.errors_so_far.extend(errors);

                // For list matchers, replace the entire array since validate_matcher_node_list
                // revalidates all items and returns the complete array
                for (key, new_value) in matches.as_object().unwrap() {
                    self.matches_so_far
                        .as_object_mut()
                        .unwrap()
                        .insert(key.clone(), new_value.clone());
                }

                continue;
            }
            // If they are both text, directly compare them. This is a "base
            // case," where we do not need to do any special logic.
            else if schema_node.kind() == "text" {
                debug!(
                    "Validating text node at input_index={}, schema_index={}",
                    input_cursor.descendant_index(),
                    schema_cursor.descendant_index()
                );

                let (errors, matches) = validate_text_node(
                    &mut input_cursor,
                    &mut schema_cursor,
                    &self.last_input_str,
                    &self.schema_str,
                    self.got_eof,
                );

                self.errors_so_far.extend(errors);
                self.matches_so_far
                    .as_object_mut()
                    .unwrap() // Safe unwrap since matches is always an object
                    .extend(matches.as_object().unwrap().clone());

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
                if is_schema_specified_list_node {
                    debug!(
                        "Skipping children length mismatch check for schema-specified list node"
                    );
                } else if self.got_eof {
                    debug!(
                        "Children length mismatch at input_index={}, schema_index={}: input_child_count={}, schema_child_count={}",
                        input_cursor.descendant_index(),
                        schema_cursor.descendant_index(),
                        input_node.child_count(),
                        schema_node.child_count()
                    );

                    self.errors_so_far.insert(Error::SchemaViolation(
                        SchemaViolationError::ChildrenLengthMismatch(
                            input_node.child_count(),
                            schema_node.child_count(),
                            input_node.descendant_count(),
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

        // Go back to parents if we have not gotten EOF yet
        if !self.got_eof {
            input_cursor.goto_parent();
            schema_cursor.goto_parent();
        }

        // Print errors so far and node indexes so far
        debug!(
            "Validation complete. Total errors so far: {}. Current input_index={}, schema_index={}",
            self.errors_so_far.len(),
            input_cursor.descendant_index(),
            schema_cursor.descendant_index()
        );

        // Update the last descendant indices to the end of the trees
        self.last_input_descendant_index = input_cursor.descendant_index();
        self.last_schema_descendant_index = schema_cursor.descendant_index();
    }
}

fn children_code_node_count(
    node: &tree_sitter::Node,
    cursor: &mut tree_sitter::TreeCursor,
) -> usize {
    node.children(&mut cursor.clone())
        .filter(|child| child.kind() == "code_span")
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a validator and run validation, returning errors
    /// Panics if validator creation fails
    fn do_validate(schema: &str, input: &str, eof: bool) -> (Vec<Error>, Value) {
        let mut validator = Validator::new(schema, input, eof).expect("Failed to create validator");
        validator.validate();
        (validator.errors(), validator.matches())
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

        let mut validator = get_validator_for_incremental(schema, initial_input, false);

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

        let (errors, _) = do_validate(schema, input, true);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, _)) => {}
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
            Error::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch(
                expected,
                actual,
                parent_index,
            )) => {
                assert_eq!(*expected, 2);
                assert_eq!(*actual, 3);
                assert_eq!(*parent_index, 7);
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
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, _)) => {}
            _ => panic!("Expected NodeContentMismatch error, got {:?}", errors[0]),
        }
    }

    #[test]
    fn test_repeated_list_matcher() {
        let schema = "- `item:/\\d+/`+\n";
        let input = "- 1\n- 2\n- 3\n";

        let (errors, matches) = do_validate(schema, input, true);
        println!("got matches {:?}", matches);
        assert!(
            errors.is_empty(),
            "expected no errors, but found {:?}",
            errors
        );
        // The matcher with + should collect all matches in an array
        let items = matches.get("item").unwrap().as_array().unwrap();
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

        println!("got matches {:?}", matches);
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
        let schema = "- Item 1\n  - Nested item\n- Item 2\n";
        let input = "- Item 1\n  - Nested item\n- Item 2\n";

        let (errors, _) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
    }

    #[test]
    fn test_nested_lists_with_mismatch() {
        let schema = "- Item 1\n  - Nested item\n- Item 2\n";
        let input = "- Item 1\n  - Wrong item\n- Item 2\n"; // "Wrong" instead of "Nested"

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
        assert!(validator.errors().is_empty());

        // Complete the input
        validator
            .read_input("# Hi Wolf\n", true)
            .expect("Failed to read input");

        validator.validate();
        let errors = validator.errors();
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
        assert!(validator.errors().is_empty());

        validator
            .read_input("# Title\n", false)
            .expect("Failed to read");
        validator.validate();
        assert!(validator.errors().is_empty());

        validator
            .read_input("# Title\n\nParagraph text\n", true)
            .expect("Failed to read");
        validator.validate();
        assert!(
            validator.errors().is_empty(),
            "Expected no errors but found {:?}",
            validator.errors()
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

        let errors = validator.errors();
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch(
                expected,
                actual,
                parent_index,
            )) => {
                assert_eq!(*expected, 3);
                assert_eq!(*actual, 2);
                assert_eq!(*parent_index, 9);
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
        assert_eq!(validator.errors().len(), 0);

        let input2 = "fhuaeifhwiuehfu";
        let mut validator =
            Validator::new(schema, input2, true).expect("Failed to create validator");
        validator.validate();
        assert_eq!(validator.errors().len(), 1);
    }

    #[test]
    fn test_multiple_matchers() {
        // The schema becomes a paragraph with multiple code nodes
        let schema = "`id:/test/` `id:/example/`";
        let input = "test example";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");
        validator.validate();
        let errors = validator.errors();

        match errors.first() {
            Some(Error::SchemaError(SchemaError::MultipleMatchersInNodeChildren(count))) => {
                println!("Got expected MultipleMatchers error with count: {}", count);
                assert_eq!(*count, 2, "Expected 2 matchers");
            }
            _ => panic!("Expected MultipleMatchers error but got: {:?}", errors),
        }
    }

    #[test]
    fn test_matcher_for_single_list_item() {
        let schema = "- `id:/item\\d/`\n- `id:/item2/`";
        let input = "- item1\n- item2";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");
        validator.validate();
        let errors = validator.errors();

        assert!(
            errors.is_empty(),
            "Expected no errors for matching list items but found {:?}",
            errors
        );
    }

    #[test]
    fn test_matcher_for_wrong_node_types() {
        let schema = "`id:/item1/`\n- `id:/item3/`";
        let input = "- item1\n- item2";

        let mut validator =
            Validator::new(schema, input, true).expect("Failed to create validator");
        validator.validate();
        let errors = validator.errors();

        match errors.first() {
            Some(Error::SchemaViolation(err)) => {
                println!("Got expected SchemaViolation error: {:?}", err);
            }
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
        let errors = validator.errors();

        match errors.first() {
            Some(Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(
                _,
                expected,
            ))) => {
                println!("Got expected NodeContentMismatch error for: {}", expected);
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

            let indices_before = (
                validator.last_input_descendant_index,
                validator.last_schema_descendant_index,
            );

            validator
                .read_input(chunk, is_eof)
                .expect("Failed to read input");
            validator.validate();

            let indices_after = (
                validator.last_input_descendant_index,
                validator.last_schema_descendant_index,
            );

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

        let errors = validator.errors();
        assert!(
            errors.is_empty(),
            "Expected no validation errors for matching content but found {:?}",
            errors
        );
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

    #[test]
    fn test_with_repeater() {
        let schema = r#"# Title

Content above repeater.

* `item:/\d/`+"#;
        let good_input = r#"# Title

Content above repeater.

* 1
* 2
* 3"#;
        let bad_input = r#"# Title

Content above repeater.

* 1
* two
* 3"#;

        // We should be able to extract some useful matches even though it fails
        let (errors, matches) = do_validate(schema, bad_input, true);
        assert!(
            !errors.is_empty(),
            "Expected validation errors for bad input but found none"
        );
        let items = matches.get("item").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], "1");
        assert_eq!(items[1], "3");

        let (errors, matches) = do_validate(schema, good_input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );
        let items = matches.get("item").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], "1");
        assert_eq!(items[1], "2");
        assert_eq!(items[2], "3");
    }

    #[test]
    fn test_nested_repeater_with_limited_size() {
        let schema = r#"
- `item1:/\d+/`{2,4}++
    - `item2:/\d+/`{2,4}+
"#;

        let input = r#"
- 1
    - 10
    - 20
- 2
    - 30
    - 40
    - 50
- 3
    - 60
    - 70
"#;

        let (errors, matches) = do_validate(schema, input, true);
        assert!(
            errors.is_empty(),
            "Expected no validation errors but found {:?}",
            errors
        );

        let item1 = matches.get("item1").unwrap().as_array().unwrap();
        assert_eq!(item1.len(), 6); // 3 strings + 3 objects with nested matches
        assert_eq!(item1[0], "1");
        assert_eq!(item1[2], "2");
        assert_eq!(item1[4], "3");

        let item2_first = item1[1].as_object().unwrap();
        let item2_second = item1[3].as_object().unwrap();
        let item2_third = item1[5].as_object().unwrap();
        let item2_first_array = item2_first.get("item2").unwrap().as_array().unwrap();
        let item2_second_array = item2_second.get("item2").unwrap().as_array().unwrap();
        let item2_third_array = item2_third.get("item2").unwrap().as_array().unwrap();
        assert_eq!(item2_first_array.len(), 2);
        assert_eq!(item2_first_array[0], "10");
        assert_eq!(item2_first_array[1], "20");
        assert_eq!(item2_second_array.len(), 3);
        assert_eq!(item2_second_array[0], "30");
        assert_eq!(item2_second_array[1], "40");
        assert_eq!(item2_second_array[2], "50");
        assert_eq!(item2_third_array.len(), 2);
        assert_eq!(item2_third_array[0], "60");
        assert_eq!(item2_third_array[1], "70");
    }
}
