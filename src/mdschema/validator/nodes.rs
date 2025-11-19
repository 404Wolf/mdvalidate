use log::{debug, trace};
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
    /// Returns the final descendant indices (input_index, schema_index).
    pub fn validate(&mut self) -> (usize, usize) {
        // Start with current nodes
        self.pairs_to_validate.push((
            self.input_cursor.descendant_index(),
            self.schema_cursor.descendant_index(),
        ));

        // Do validation until there's no more pairs to validate
        while self.validate_next_pair() {}

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

    fn validate_next_pair(&mut self) -> bool {
        let (input_node, schema_node) = match self.pairs_to_validate.pop() {
            Some(pair) => pair,
            None => return false,
        };

        debug!(
            "Validating node pair: input_index={} [{}], schema_index={} [{}]",
            self.input_cursor.descendant_index(),
            self.input_cursor.node().kind(),
            self.schema_cursor.descendant_index(),
            self.schema_cursor.node().kind()
        );

        let input_node = self.input_cursor.node();
        let schema_node = self.schema_cursor.node();

        let schema_children_code_node_count =
            Self::children_code_node_count(&schema_node, &mut self.schema_cursor);

        let schema_node_first_list_item_code_node_count = {
            schema_node
                .child(0)
                .map(|first_child| {
                    Self::children_code_node_count(&first_child, &mut self.schema_cursor.clone())
                })
                .unwrap_or(0)
        };
        let is_schema_specified_list_node = schema_node.kind() == "tight_list"
            && schema_node.child_count() == 1
            && input_node.child_count() > 1;

        debug!(
            "Schema node is a schema-specified list node: {}",
            is_schema_specified_list_node
        );

        if schema_children_code_node_count > 1
            || (schema_node.kind() == "tight_list"
                && schema_node_first_list_item_code_node_count > 0)
        {
            trace!("Schema node has multiple matcher children, reporting error");

            self.state.add_new_error(Error::SchemaError(
                SchemaError::MultipleMatchersInNodeChildren(schema_children_code_node_count),
            ));

            return true;
        }

        let input_is_text_only = input_node.kind() == "text"
            || (input_node.child_count() == 1
                && input_node
                    .child(0)
                    .map(|c| c.kind() == "text")
                    .unwrap_or(false));
        trace!("Input node is text only: {}", input_is_text_only);

        if schema_children_code_node_count == 1 && input_is_text_only {
            debug!(
                "Validating matcher node at input_index={}, schema_index={}",
                self.input_cursor.descendant_index(),
                self.schema_cursor.descendant_index()
            );

            self.schema_cursor.goto_parent();

            // Save cursor positions
            let saved_input_idx = self.input_cursor.descendant_index();
            let saved_schema_idx = self.schema_cursor.descendant_index();

            let (errors, matches) = self.validate_matcher_node();

            // Restore cursor positions
            self.input_cursor.goto_descendant(saved_input_idx);
            self.schema_cursor.goto_descendant(saved_schema_idx);

            for error in errors {
                self.state.add_new_error(error);
            }
            self.state.add_new_matches(matches);

            return true;
        } else if is_schema_specified_list_node {
            // Save cursor positions
            let saved_input_idx = self.input_cursor.descendant_index();
            let saved_schema_idx = self.schema_cursor.descendant_index();

            let (errors, matches) = self.validate_matcher_node_list();

            // Restore cursor positions
            self.input_cursor.goto_descendant(saved_input_idx);
            self.schema_cursor.goto_descendant(saved_schema_idx);

            for error in errors {
                self.state.add_new_error(error);
            }

            for (key, new_value) in matches.as_object().unwrap() {
                self.state.add_new_match(key.clone(), new_value.clone());
            }

            return true;
        } else if schema_node.kind() == "text" {
            debug!(
                "Validating text node at input_index={}, schema_index={}",
                self.input_cursor.descendant_index(),
                self.schema_cursor.descendant_index()
            );

            // Save cursor positions
            let saved_input_idx = self.input_cursor.descendant_index();
            let saved_schema_idx = self.schema_cursor.descendant_index();

            let (errors, matches) = self.validate_text_node();

            // Restore cursor positions
            self.input_cursor.goto_descendant(saved_input_idx);
            self.schema_cursor.goto_descendant(saved_schema_idx);

            for error in errors {
                self.state.add_new_error(error);
            }
            for (key, value) in matches.as_object().unwrap() {
                self.state.add_new_match(key.clone(), value.clone());
            }

            return true;
        }

        if input_node.child_count() != schema_node.child_count() {
            if is_schema_specified_list_node {
                debug!("Skipping children length mismatch check for schema-specified list node");
            } else if self.state.got_eof() {
                debug!(
                                "Children length mismatch at input_index={}, schema_index={}: input_child_count={}, schema_child_count={}",
                                self.input_cursor.descendant_index(),
                                self.schema_cursor.descendant_index(),
                                input_node.child_count(),
                                schema_node.child_count()
                            );

                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch(
                        input_node.child_count(),
                        schema_node.child_count(),
                        input_node.descendant_count(),
                    ),
                ));
            }
        }

        debug!(
                        "Currently at input_index={}, schema_index={}: input_child_count={}, schema_child_count={}",
                        self.input_cursor.descendant_index(),
                        self.schema_cursor.descendant_index(),
                        input_node.child_count(),
                        schema_node.child_count()
                    );

        if self.input_cursor.goto_first_child() && self.schema_cursor.goto_first_child() {
            debug!(
                "Queued first child pair for validation: input_index={}, schema_index={}",
                self.input_cursor.descendant_index(),
                self.schema_cursor.descendant_index()
            );

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
                    debug!(
                        "Queued child pair for validation: input_index={}, schema_index={}",
                        self.input_cursor.descendant_index(),
                        self.schema_cursor.descendant_index()
                    );
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

    /// Validate a text node against the schema text node.
    ///
    /// This is a node that is just a simple literal text node. We validate that
    /// the text content is identical.
    fn validate_text_node(&mut self) -> NodeValidationResult {
        let schema_node = self.schema_cursor.node();
        let input_node = self.input_cursor.node();

        let mut errors = Vec::new();

        let input_str = self.state.last_input_str();
        let schema_str = self.state.schema_str();
        let eof = self.state.got_eof();

        let schema_text = &schema_str[schema_node.byte_range()];
        let input_text = &input_str[input_node.byte_range()];

        debug!(
            "Comparing text: schema='{}' vs input='{}'",
            schema_text, input_text
        );

        if schema_text != input_text {
            trace!(
                "Text content mismatch at node index {}: expected '{}', got '{}'",
                self.input_cursor.descendant_index(),
                schema_text,
                input_text
            );

            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    self.input_cursor.descendant_index(),
                    schema_text.into(),
                ),
            ));
        }

        if !eof && is_last_node(input_str, &input_node) {
            debug!("Skipping error reporting, incomplete last node");
            (vec![], json!({}))
        } else {
            (errors, json!({}))
        }
    }

    /// Validate a matcher node against the children of a list input node.
    ///
    /// This works by re-running the validation using validate_matcher_node on each input node in the
    /// list.
    fn validate_matcher_node_list(&mut self) -> NodeValidationResult {
        assert!(self.is_list_node(&self.input_cursor.node()));
        assert!(self.is_list_node(&self.schema_cursor.node()));

        let mut errors = Vec::new();
        let mut matches = json!({});

        self.schema_cursor.goto_first_child();
        self.schema_cursor.goto_first_child();
        self.schema_cursor.goto_next_sibling();
        assert_eq!(self.schema_cursor.node().kind(), "paragraph");

        // Now validate each input node's list items against the schema's single list item

        if !self.input_cursor.goto_first_child() {
            // No children to validate
            return (errors, matches);
        }

        self.input_cursor.goto_first_child();
        self.input_cursor.goto_next_sibling();
        assert_eq!(self.input_cursor.node().kind(), "paragraph");

        loop {
            let (node_errors, node_matches) = self.validate_matcher_node();

            errors.extend(node_errors);
            for (key, value) in node_matches.as_object().unwrap() {
                matches[key] = value.clone();
            }

            if !self.input_cursor.goto_next_sibling() || !self.input_cursor.goto_next_sibling() {
                break;
            }
        }

        (errors, matches)
    }

    /// Validate a matcher node against the input node.
    ///
    /// A matcher node looks like `id:/pattern/` in the schema.
    fn validate_matcher_node(&mut self) -> NodeValidationResult {
        if self.is_list_node(&self.input_cursor.node())
            && self.is_list_node(&self.schema_cursor.node())
        {
            // If the input node is a list, delegate to validate_matcher_node_list
            return self.validate_matcher_node_list();
        }

        let input_str = self.state.last_input_str();
        let schema_str = self.state.schema_str();
        let eof = self.state.got_eof();
        let input_node = self.input_cursor.node();
        let schema_nodes = self
            .schema_cursor
            .node()
            .named_children(&mut self.schema_cursor.clone())
            .collect::<Vec<Node>>();
        let input_node_descendant_index = self.input_cursor.descendant_index();

        let is_incomplete = !eof && is_last_node(input_str, &input_node);

        let mut errors = Vec::new();
        let mut matches = json!({});

        let (code_node, next_node) =
            match Self::find_matcher_node(&schema_nodes, input_node_descendant_index) {
                Ok((code, next)) => (code, next),
                Err(e) => return (vec![e], matches),
            };

        let matcher_node = match code_node {
            None => {
                errors.push(Error::SchemaError(
                    SchemaError::NoMatcherInListNodeChildren(input_node_descendant_index),
                ));
                return (errors, matches);
            }
            Some(node) => node,
        };

        let matcher_text = &schema_str[matcher_node.byte_range()];

        let matcher = match Matcher::new(
            matcher_text,
            next_node.map(|n| &schema_str[n.byte_range()]).as_deref(),
        ) {
            Ok(m) => m,
            Err(_) => {
                trace!(
                    "Invalid matcher format at node index {}: '{}'",
                    input_node_descendant_index,
                    matcher_text
                );

                return (
                    vec![Error::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch(
                            input_node_descendant_index,
                            matcher_text.into(),
                        ),
                    )],
                    matches,
                );
            }
        };

        let schema_start = schema_nodes[0].byte_range().start;
        let matcher_start = matcher_node.byte_range().start - schema_start;
        let matcher_end = matcher_node.byte_range().end - schema_start;

        // Always validate prefix, even for incomplete nodes
        let prefix_schema = &schema_str[schema_start..schema_start + matcher_start];

        // Check if we have enough input to validate the prefix (the end of the
        // prefix is the start of the matcher)
        let input_has_full_prefix = input_node.byte_range().len() >= matcher_start;

        if input_has_full_prefix {
            let prefix_input = &input_str
                [input_node.byte_range().start..input_node.byte_range().start + matcher_start];

            // Do the actual prefix comparison
            if prefix_schema != prefix_input {
                trace!(
                    "Prefix mismatch at node index {}: expected '{}', got '{}'",
                    input_node_descendant_index,
                    prefix_schema,
                    prefix_input
                );

                errors.push(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        prefix_schema.into(),
                    ),
                ));

                return (errors, matches);
            }
        } else if matcher_start > 0 && !is_incomplete {
            // Input is too short to contain the required prefix, and we've reached EOF
            // so this is a genuine error (not just incomplete input)
            trace!(
                    "Input too short for prefix at node index {}: expected prefix '{}' ({} bytes) but input is only {} bytes",
                    input_node_descendant_index,
                    prefix_schema,
                    matcher_start,
                    input_node.byte_range().len()
                );

            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    prefix_schema.into(),
                ),
            ));

            return (errors, matches);
        }

        // Skip matcher and suffix validation if node is incomplete
        if is_incomplete {
            debug!("Skipping matcher and suffix validation - incomplete node");
            return (errors, matches);
        }

        trace!(
            "Validating matcher at node index {}: '{}'",
            input_node_descendant_index,
            matcher_text
        );

        let input_start = input_node.byte_range().start + matcher_start;
        let input_to_match = &input_str[input_start..];

        // If the matcher is for a ruler, we should expect the entire input node to be a ruler
        if matcher.is_ruler() {
            if input_node.kind() != "thematic_break" {
                errors.push(Error::SchemaViolation(
                    SchemaViolationError::NodeTypeMismatch(
                        input_node_descendant_index,
                        input_node_descendant_index, // should be the same as the schema's.
                                                     // TODO: is this really true though?
                    ),
                ));
                return (errors, matches);
            } else {
                // It's a ruler, no further validation needed
                return (errors, json!({}));
            }
        }

        match matcher.match_str(input_to_match) {
            Some(matched_str) => {
                // Validate suffix
                let schema_end = schema_nodes.last().unwrap().byte_range().end;

                let suffix_schema = get_everything_after_special_chars(
                    &schema_str[schema_start + matcher_end..schema_end],
                );

                let suffix_start = input_start + matched_str.len();
                let input_end = input_node.byte_range().end;

                // Ensure suffix_start doesn't exceed input_end
                if suffix_start > input_end {
                    trace!(
                        "Suffix mismatch at node index {}: expected '{}', but input is too short",
                        input_node_descendant_index,
                        suffix_schema
                    );

                    // out of bounds
                    errors.push(Error::SchemaViolation(
                        SchemaViolationError::NodeContentMismatch(
                            input_node_descendant_index,
                            suffix_schema.into(),
                        ),
                    ));
                } else {
                    let suffix_input = &input_str[suffix_start..input_end];

                    if suffix_schema != suffix_input {
                        trace!(
                            "Suffix mismatch at node index {}: expected '{}', got '{}'",
                            input_node_descendant_index,
                            suffix_schema,
                            suffix_input
                        );

                        errors.push(Error::SchemaViolation(
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
                        matches[id] = json!(matched_str);
                    }
                    None => {}
                }
            }
            None => {
                trace!(
                    "Matcher pattern mismatch at node index {}: '{}'",
                    input_node_descendant_index,
                    matcher_text
                );

                errors.push(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        matcher_text.into(),
                    ),
                ));
            }
        };

        // If this is the last node, don't validate it if we haven't reached EOF,
        // since the matcher might be incomplete.
        if !eof && is_incomplete {
            (vec![], matches)
        } else {
            (errors, matches)
        }
        // Otherwise, check if the nodes are both list nodes
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
                    trace!(
                        "Multiple matchers found in single node at index {}",
                        input_node_descendant_index
                    );

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
        env_logger::Builder::from_default_env()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init()
            .ok();

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
        env_logger::Builder::from_default_env()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init()
            .ok();

        let schema = "# Hello `name:/\\w+/`\n";
        let input = "# Hello Wolf\n";

        let (matches, errors) = validate_str(schema, input);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(matches, json!({"name": "Wolf"}));
    }
}
