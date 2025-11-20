use log::debug;
use serde_json::{json, Value};
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::{Error, SchemaError, SchemaViolationError},
    matcher::{get_everything_after_special_chars, Matcher},
    state::ValidatorState,
    utils::is_last_node,
};

pub type NodeValidationResult = (Vec<Error>, Value);

/// A node validator that validates input nodes against schema nodes.
pub struct NodeValidator<'a> {
    state: &'a mut ValidatorState,
    input_cursor: TreeCursor<'a>,
    schema_cursor: TreeCursor<'a>,
    pairs_to_validate: Vec<(usize, usize)>,
}

impl<'a> NodeValidator<'a> {
    pub fn new(
        state: &'a mut ValidatorState,
        input_cursor: TreeCursor<'a>,
        schema_cursor: TreeCursor<'a>,
    ) -> Self {
        Self {
            state,
            input_cursor,
            schema_cursor,
            pairs_to_validate: Vec::new(),
        }
    }

    /// Validates nodes starting from the current cursor positions and walks them to completion.
    ///
    /// Returns the final descendant indices (input_index, schema_index).
    pub fn validate(&mut self) -> (usize, usize) {
        // Start with current nodes
        self.pairs_to_validate.push((
            self.input_cursor.descendant_index(),
            self.schema_cursor.descendant_index(),
        ));

        // Do validation until there's no more pairs to validate (skipping incomplete last nodes)
        while !self.is_incomplete() && self.validate_node_pair() {}

        // Return to parent nodes if not at EOF, we'll need to revalidate them on the next run
        if !self.state.got_eof() {
            self.input_cursor.goto_parent();
            self.schema_cursor.goto_parent();
        }

        // Return final descendant indices
        (
            self.input_cursor.descendant_index(),
            self.schema_cursor.descendant_index(),
        )
    }

    fn is_incomplete(&self) -> bool {
        !self.state.got_eof()
            && is_last_node(self.state.last_input_str(), &self.input_cursor.node())
    }

    /// Whether the current schema node is a single list node and the input node is multiple list items.
    fn is_schema_specified_list_node(&self) -> bool {
        self.is_list_node(&self.schema_cursor.node())
            && self.schema_cursor.node().child_count() == 1
            && self.input_cursor.node().child_count() > 1
    }

    fn validate_node_pair(&mut self) -> bool {
        let (input_node, schema_node) = match self.pairs_to_validate.pop() {
            Some(pair) => pair,
            None => return false,
        };

        let is_schema_specified_list_node = self.is_schema_specified_list_node();

        self.input_cursor.goto_descendant(input_node);
        self.schema_cursor.goto_descendant(schema_node);

        let schema_children_code_node_count =
            Self::children_code_node_count(&self.schema_cursor.node(), &mut self.schema_cursor);

        let schema_node_first_list_item_code_node_count = {
            self.schema_cursor
                .node()
                .child(0)
                .map(|first_child| {
                    Self::children_code_node_count(&first_child, &mut self.schema_cursor.clone())
                })
                .unwrap_or(0)
        };

        if schema_children_code_node_count > 1
            || (self.schema_cursor.node().kind() == "tight_list"
                && schema_node_first_list_item_code_node_count > 0)
        {
            self.state.add_new_error(Error::SchemaError(
                SchemaError::MultipleMatchersInNodeChildren(schema_children_code_node_count),
            ));

            return true;
        }

        let input_is_text_only = self.input_cursor.node().kind() == "text"
            || (self.input_cursor.node().child_count() == 1
                && self
                    .input_cursor
                    .node()
                    .child(0)
                    .map(|c| c.kind() == "text")
                    .unwrap_or(false));

        if schema_children_code_node_count == 1 && input_is_text_only {
            self.schema_cursor.goto_parent();

            // Save cursor positions
            let saved_input_idx = self.input_cursor.descendant_index();
            let saved_schema_idx = self.schema_cursor.descendant_index();

            self.validate_matcher_vs_text();

            // Restore cursor positions
            self.input_cursor.goto_descendant(saved_input_idx);
            self.schema_cursor.goto_descendant(saved_schema_idx);

            return true;
        } else if is_schema_specified_list_node {
            self.validate_matcher_vs_list();

            return true;
        } else if self.schema_cursor.node().kind() == "text" {
            self.validate_text_vs_text();

            return true;
        }

        if self.input_cursor.node().child_count() != self.schema_cursor.node().child_count() {
            if is_schema_specified_list_node {
            } else if self.state.got_eof() {
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch(
                        self.input_cursor.node().child_count(),
                        self.schema_cursor.node().child_count(),
                        self.input_cursor.node().descendant_count(),
                    ),
                ));
            }
        }

        if self.input_cursor.goto_first_child() && self.schema_cursor.goto_first_child() {
            self.pairs_to_validate.push((
                self.input_cursor.descendant_index(),
                self.schema_cursor.descendant_index(),
            ));

            loop {
                let input_had_sibling = self.input_cursor.goto_next_sibling();
                let schema_had_sibling = self.schema_cursor.goto_next_sibling();

                if input_had_sibling && schema_had_sibling {
                    self.pairs_to_validate.push((
                        self.input_cursor.descendant_index(),
                        self.schema_cursor.descendant_index(),
                    ));
                } else {
                    debug!("No more siblings to process in current nodes");
                    break;
                }
            }

            self.input_cursor.goto_parent();
            self.schema_cursor.goto_parent();
        }

        true
    }

    fn validate_text_vs_text(&mut self) {
        let input_node = self.input_cursor.node();

        let schema_text = &self.state.schema_str()[self.schema_cursor.node().byte_range()];
        let input_text = &self.state.last_input_str()[input_node.byte_range()];

        if schema_text != input_text {
            self.state.add_new_error(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    self.input_cursor.descendant_index(),
                    schema_text.into(),
                ),
            ));
        }
    }

    fn validate_matcher_vs_list(&mut self) {
        assert!(self.is_list_node(&self.input_cursor.node()));
        assert!(self.is_list_node(&self.schema_cursor.node()));

        self.schema_cursor.goto_first_child();
        self.schema_cursor.goto_first_child();
        self.schema_cursor.goto_next_sibling();
        assert_eq!(self.schema_cursor.node().kind(), "paragraph");

        // Now validate each input node's list items against the schema's single list item

        if !self.input_cursor.goto_first_child() {
            // No children to validate
            return;
        }

        self.input_cursor.goto_first_child();
        self.input_cursor.goto_next_sibling();
        assert_eq!(self.input_cursor.node().kind(), "paragraph");

        loop {
            self.validate_matcher_vs_text();

            if !self.input_cursor.goto_next_sibling() || !self.input_cursor.goto_next_sibling() {
                break;
            }
        }
    }

    fn validate_matcher_vs_text(&mut self) {
        if self.is_list_node(&self.input_cursor.node())
            && self.is_list_node(&self.schema_cursor.node())
        {
            // If the input node is a list, delegate to validate_matcher_node_list
            return self.validate_matcher_vs_list();
        }

        let schema_nodes = self
            .schema_cursor
            .node()
            .named_children(&mut self.schema_cursor.clone())
            .collect::<Vec<Node>>();

        let input_node_descendant_index = self.input_cursor.descendant_index();

        let (code_node, next_node) =
            match Self::find_matcher_node(&schema_nodes, input_node_descendant_index) {
                Ok((code, next)) => (code, next),
                Err(e) => {
                    self.state.add_new_error(e.clone());
                    return;
                }
            };

        let matcher_node = match code_node {
            None => {
                self.state.add_new_error(Error::SchemaError(
                    SchemaError::NoMatcherInListNodeChildren(input_node_descendant_index),
                ));
                return;
            }
            Some(node) => node,
        };

        let matcher_text = &self.state.schema_str()[matcher_node.byte_range()];

        let matcher = match Matcher::new(
            matcher_text,
            next_node
                .map(|n| &self.state.schema_str()[n.byte_range()])
                .as_deref(),
        ) {
            Ok(m) => m,
            Err(_) => {
                self.state.add_new_errors(vec![Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        matcher_text.into(),
                    ),
                )]);

                return;
            }
        };

        let schema_start = schema_nodes[0].byte_range().start;
        let matcher_start = matcher_node.byte_range().start - schema_start;
        let matcher_end = matcher_node.byte_range().end - schema_start;

        // Always validate prefix, even for incomplete nodes
        let prefix_schema = &self.state.schema_str()[schema_start..schema_start + matcher_start];

        // Check if we have enough input to validate the prefix (the end of the
        // prefix is the start of the matcher)
        let input_has_full_prefix = self.input_cursor.node().byte_range().len() >= matcher_start;

        if input_has_full_prefix {
            let prefix_input =
                &self.state.last_input_str()[self.input_cursor.node().byte_range().start
                    ..self.input_cursor.node().byte_range().start + matcher_start];

            // Do the actual prefix comparison
            if prefix_schema != prefix_input {
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        prefix_schema.into(),
                    ),
                ));

                return;
            }
        } else if matcher_start > 0 && !self.is_incomplete() {
            // Input is too short to contain the required prefix, and we've reached EOF
            // so this is a genuine error (not just incomplete input)
            self.state.add_new_error(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    prefix_schema.into(),
                ),
            ));
            return;
        }

        // Skip matcher and suffix validation if node is incomplete
        if self.is_incomplete() {
            return;
        }

        let input_start = self.input_cursor.node().byte_range().start + matcher_start;
        let input_to_match = self.state.last_input_str()[input_start..].to_string();

        // If the matcher is for a ruler, we should expect the entire input node to be a ruler
        if matcher.is_ruler() {
            if self.input_cursor.node().kind() != "thematic_break" {
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::NodeTypeMismatch(
                        input_node_descendant_index,
                        input_node_descendant_index, // should be the same as the schema's.
                                                     // TODO: is this really true though?
                    ),
                ));
                return;
            } else {
                // It's a ruler, no further validation needed
                return;
            }
        }

        match matcher.match_str(&input_to_match) {
            Some(matched_str) => {
                // Validate suffix
                let schema_end = schema_nodes.last().unwrap().byte_range().end;

                let suffix_schema = get_everything_after_special_chars(
                    &self.state.schema_str()[schema_start + matcher_end..schema_end],
                );

                let suffix_start = input_start + matched_str.len();
                let input_end = self.input_cursor.node().byte_range().end;

                // Ensure suffix_start doesn't exceed input_end
                if suffix_start > input_end {
                    // Out of bounds
                    self.state.add_new_error(Error::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch(
                            input_node_descendant_index,
                            suffix_schema.into(),
                        ),
                    ));
                } else {
                    let suffix_input = &self.state.last_input_str()[suffix_start..input_end];

                    if suffix_schema != suffix_input {
                        self.state.add_new_error(Error::SchemaViolation(
                            SchemaViolationError::NodeContentMismatch(
                                input_node_descendant_index,
                                suffix_schema.into(),
                            ),
                        ));
                    }
                }
                // Good match! Add the matched node to the matches (if it has an id)
                match matcher.id() {
                    Some(id) => {
                        self.state.add_new_match(id.to_string(), json!(matched_str));
                    }
                    None => {}
                }
            }
            None => {
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        matcher_text.into(),
                    ),
                ));
            }
        };

        // Otherwise, check if the nodes are both list nodes
        if self.is_list_node(&self.input_cursor.node())
            && self.is_list_node(&self.schema_cursor.node())
        {
            // If the input node is a list, delegate to validate_matcher_node_list
            self.validate_matcher_vs_list();
        }
    }

    /// Find the matcher code_span node in a list of schema nodes.
    /// Returns the matcher node and the next node after it, if any.
    /// Returns an error if multiple matchers are found.
    fn find_matcher_node<'b>(
        schema_nodes: &'b [Node<'b>],
        input_node_descendant_index: usize,
    ) -> Result<(Option<&'b Node<'b>>, Option<&'b Node<'b>>), Error> {
        let mut code_node = None;
        let mut next_node = None;

        for (i, node) in schema_nodes.iter().enumerate() {
            if node.kind() == "code_span" {
                if code_node.is_some() {
                    return Err(Error::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch(
                            input_node_descendant_index,
                            "Multiple matchers in single node".into(),
                        ),
                    ));
                }
                code_node = Some(node);
                next_node = schema_nodes.get(i + 1);
            }
        }

        Ok((code_node, next_node))
    }

    /// Count the number of nodes that are code nodes.
    fn children_code_node_count(
        node: &tree_sitter::Node,
        cursor: &mut tree_sitter::TreeCursor,
    ) -> usize {
        node.children(&mut cursor.clone())
            .filter(|child| child.kind() == "code_span")
            .count()
    }

    /// Check if a node is a list (tight_list or loose_list).
    fn is_list_node(&self, node: &Node) -> bool {
        match node.kind() {
            "tight_list" | "loose_list" => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::utils::new_markdown_parser;

    use super::*;

    fn validate_str(schema: &str, input: &str) -> (Value, Vec<Error>) {
        let mut state = ValidatorState::new(schema.to_string(), input.to_string(), true);

        let mut parser = new_markdown_parser();
        let schema_tree = parser.parse(schema, None).unwrap();
        let input_tree = parser.parse(input, None).unwrap();

        {
            let mut node_validator =
                NodeValidator::new(&mut state, input_tree.walk(), schema_tree.walk());

            node_validator.validate();
        }

        let errors = state.errors_so_far().cloned().collect::<Vec<Error>>();
        let matches = state.matches_so_far().clone();

        (matches, errors)
    }

    #[test]
    fn test_heading_and_list() {
        let schema = "# Title\n\n- `item:/\\w+/`\n";
        let input = "# Title\n\n- hello\n";

        let (matches, errors) = validate_str(schema, input);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(
            matches,
            json!({
                "item": "hello"
            }),
        );
    }

    #[test]
    fn test_simple_heading() {
        let schema = "# Hello `name:/\\w+/`\n";
        let input = "# Hello Wolf\n";

        let (matches, errors) = validate_str(schema, input);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(matches, json!({"name": "Wolf"}));
    }
}
