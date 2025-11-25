use serde_json::{json, Value};
use tracing::{debug, instrument};
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
        }
    }

    /// Validates nodes starting from the current cursor positions and walks them to completion.
    ///
    /// Returns the final descendant indices (input_index, schema_index).
    pub fn validate(&mut self) -> (usize, usize) {
        let new_matches = self.validate_node_pair(
            &mut self.input_cursor.clone(),
            &mut self.schema_cursor.clone(),
        );

        if !self.state.got_eof() {
            self.input_cursor.goto_parent();
            self.schema_cursor.goto_parent();
        }

        self.state.join_new_matches(new_matches);

        (
            self.input_cursor.descendant_index(),
            self.schema_cursor.descendant_index(),
        )
    }

    fn is_incomplete(&self, input_cursor: &mut TreeCursor) -> bool {
        !self.state.got_eof() && is_last_node(self.state.last_input_str(), &input_cursor.node())
    }

    /// Whether both the schema and input node are lists nodes, but the schema node has only one child while the input node has multiple children.
    fn is_schema_specified_list_node(
        &self,
        input_cursor: &TreeCursor,
        schema_cursor: &TreeCursor,
    ) -> bool {
        self.is_list_node(&schema_cursor.node())
            && self.is_list_node(&input_cursor.node())
            && schema_cursor.node().child_count() == 1
            && input_cursor.node().child_count() >= 1
    }

    /// Validate the next pair of nodes in our stack.
    ///
    /// Returns whether there were more pairs to validate.
    #[instrument(skip(self, input_cursor, schema_cursor), level = "trace", fields(
        input = %input_cursor.node().kind(),
        schema = %schema_cursor.node().kind()
    ), ret)]
    fn validate_node_pair(
        &mut self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) -> Value {
        debug!("Input sexpr: {}", input_cursor.node().to_sexp());
        debug!("Schema sexpr: {}", schema_cursor.node().to_sexp());

        let input_cursor = &mut input_cursor.clone();
        let schema_cursor = &mut schema_cursor.clone();

        let mut matches = json!({});

        let is_schema_specified_list_node =
            self.is_schema_specified_list_node(input_cursor, schema_cursor);

        let input_is_text_node = input_cursor.node().kind() == "text";
        let input_has_single_text_child = input_cursor.node().child_count() == 1
            && input_cursor
                .node()
                .child(0)
                .map(|c| c.kind() == "text")
                .unwrap_or(false);

        let input_is_text_only = input_is_text_node || input_has_single_text_child;
        let schema_direct_children_code_node_count = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .filter(|c| c.kind() == "code_span")
            .count();

        if schema_direct_children_code_node_count > 1 {
            self.state.add_new_error(Error::SchemaError(
                SchemaError::MultipleMatchersInNodeChildren(
                    schema_cursor.descendant_index(),
                    schema_direct_children_code_node_count,
                ),
            ));
            return json!({});
        }

        if schema_direct_children_code_node_count == 1 && input_is_text_only {
            let new_matches = self.validate_matcher_vs_text(input_cursor, schema_cursor);

            // Add the validation matches to our top-level matches
            if let Some(obj) = new_matches.as_object() {
                for (key, value) in obj {
                    matches
                        .as_object_mut()
                        .unwrap()
                        .insert(key.clone(), value.clone());
                }
            }

            return new_matches;
        } else if is_schema_specified_list_node {
            let new_matches = self.validate_matcher_vs_list(input_cursor, schema_cursor);

            // Add the validation matches to our top-level matches
            if let Some(obj) = new_matches.as_object() {
                for (key, value) in obj {
                    matches
                        .as_object_mut()
                        .unwrap()
                        .insert(key.clone(), value.clone());
                }
            }

            return matches;
        } else if schema_cursor.node().kind() == "text" {
            self.validate_text_vs_text(input_cursor, schema_cursor); // doesn't return matches since it's just literal comparison
        }

        if input_cursor.node().child_count() != schema_cursor.node().child_count() {
            if self.is_schema_specified_list_node(input_cursor, schema_cursor) {
                // In the repeating list node case that we already took care of this situation is fine
                // TODO: have we made sure that the repeating list had a +?
            } else if self.state.got_eof() {
                // TODO: this feels wrong, we should check to make sure that when eof is false we detect nested incomplete nodes too
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch(
                        input_cursor.node().child_count(),
                        schema_cursor.node().child_count(),
                        schema_cursor.node().descendant_count(),
                    ),
                ));
            }
        }

        // TODO: what if one node has children and the other doesn't?
        if input_cursor.goto_first_child() && schema_cursor.goto_first_child() {
            let new_matches = self.validate_node_pair(input_cursor, schema_cursor);

            // Add the new matches to our top-level matches
            if let Some(obj) = new_matches.as_object() {
                for (key, value) in obj {
                    matches
                        .as_object_mut()
                        .unwrap()
                        .insert(key.clone(), value.clone());
                }
            }

            loop {
                // TODO: handle case where one has more children than the other
                let input_had_sibling = input_cursor.goto_next_sibling();
                let schema_had_sibling = schema_cursor.goto_next_sibling();

                if input_had_sibling && schema_had_sibling {
                    let new_matches = self.validate_node_pair(input_cursor, schema_cursor);

                    // Add the new matches to our top-level matches
                    if let Some(obj) = new_matches.as_object() {
                        for (key, value) in obj {
                            matches
                                .as_object_mut()
                                .unwrap()
                                .insert(key.clone(), value.clone());
                        }
                    }
                } else {
                    break;
                }
            }
        }

        matches
    }

    #[instrument(skip(self, input_cursor, schema_cursor), level = "debug", fields(
        input = %input_cursor.node().kind(),
        schema = %schema_cursor.node().kind()
    ), ret)]
    fn validate_text_vs_text(
        &mut self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) {
        let input_node = input_cursor.node();

        let schema_text = &self.state.schema_str()[schema_cursor.node().byte_range()];
        let input_text = &self.state.last_input_str()[input_node.byte_range()];

        if schema_text != input_text && self.state.got_eof() {
            self.state.add_new_error(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_cursor.descendant_index(),
                    schema_text.into(),
                ),
            ));
        }
    }

    #[instrument(skip(self, input_cursor, schema_cursor), level = "debug", fields(
         input = %input_cursor.node().kind(),
         schema = %schema_cursor.node().kind()
     ), ret)]
    fn validate_matcher_vs_list(
        &mut self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) -> Value {
        let mut matches = json!({});

        // Called when we have our cursors pointed at a schema list node and an
        // input list node where the schema has only one child (the list item to
        // match against all input list items) and the input has (>=1) children.

        assert!(
            self.is_list_node(&input_cursor.node()),
            "Input node is not a list, got {}",
            input_cursor.node().kind()
        );
        assert!(
            self.is_list_node(&schema_cursor.node()),
            "Schema node is not a list, got {}",
            schema_cursor.node().kind()
        );

        let input_list_node = input_cursor.node();

        schema_cursor.goto_first_child(); // we're at a list_item
        assert_eq!(schema_cursor.node().kind(), "list_item");
        schema_cursor.goto_first_child(); // we're at a list_marker
        assert_eq!(schema_cursor.node().kind(), "list_marker");
        schema_cursor.goto_next_sibling(); // list_marker -> content (may be paragraph)

        // Get the matcher for this level
        let main_matcher = Matcher::new(
            &self.state.schema_str()[schema_cursor.node().child(0).unwrap().byte_range()],
            None,
        )
        .unwrap(); // TODO: don't unwrap

        if !main_matcher.is_repeated() {
            self.state.add_new_error(Error::SchemaViolation(
                SchemaViolationError::NonRepeatingMatcherInListContext(
                    schema_cursor.descendant_index(),
                ),
            ));
        }

        let main_matcher_id = main_matcher.id();
        let mut main_items = Vec::new();
        let mut notes_objects = Vec::new();

        // Process each list item at this level
        for child in input_list_node.children(&mut input_cursor.clone()) {
            if child.kind() != "list_item" {
                continue;
            }

            let mut child_cursor = input_list_node.walk();
            child_cursor.reset(child);

            // Process this list item
            child_cursor.goto_first_child(); // list_marker
            if child_cursor.node().kind() != "list_marker" {
                continue;
            }

            let has_content = child_cursor.goto_next_sibling(); // Move to content after the list marker

            // Process paragraph if present
            if has_content && child_cursor.node().kind() == "paragraph" {
                // Get the text content of the paragraph
                let paragraph_text =
                    self.state.last_input_str()[child_cursor.node().byte_range()].trim();

                // Add the text as a separate item in the main array
                main_items.push(json!(paragraph_text));

                // Check for nested list - move to next sibling
                // A sibling of the paragraph is the next node
                let has_nested_list = child_cursor.goto_next_sibling();

                // Process nested list if present
                if has_nested_list && self.is_list_node(&child_cursor.node()) {
                    // Save a copy of the schema cursor
                    let mut schema_list_cursor = schema_cursor.clone();

                    // Navigate to the nested list in the schema
                    let schema_has_nested_list = schema_list_cursor.goto_next_sibling();

                    if schema_has_nested_list && self.is_list_node(&schema_list_cursor.node()) {
                        // Process the nested list
                        let nested_matches = self
                            .validate_matcher_vs_list(&mut child_cursor, &mut schema_list_cursor);

                        // Add each nested match as a separate object in the notes_objects array
                        for (key, value) in nested_matches.as_object().unwrap() {
                            let mut note_obj = json!({});
                            note_obj[key] = value.clone();
                            notes_objects.push(note_obj);
                        }
                    }
                }
            }
        }

        // Add all notes objects to the main items array
        for note_obj in notes_objects {
            main_items.push(note_obj);
        }

        // Add the main items to the result
        if let Some(id) = main_matcher_id {
            matches[id] = json!(main_items);
        }

        return matches;
    }

    #[instrument(skip(self, input_cursor, schema_cursor), level = "debug", fields(
        input = %input_cursor.node().kind(),
        schema = %schema_cursor.node().kind()
    ), ret)]
    fn validate_matcher_vs_text(
        &mut self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) -> Value {
        let input_cursor = &mut input_cursor.clone();
        let schema_cursor = &mut schema_cursor.clone();

        let mut matches = json!({});

        if self.is_list_node(&input_cursor.node()) && self.is_list_node(&schema_cursor.node()) {
            // If the input node is a list, delegate to validate_matcher_node_list
            return self.validate_matcher_vs_list(input_cursor, schema_cursor);
        }

        let schema_nodes = schema_cursor
            .node()
            .named_children(&mut schema_cursor.clone())
            .collect::<Vec<Node>>();

        let input_node_descendant_index = input_cursor.descendant_index();

        let (code_node, next_node) =
            match Self::find_matcher_node(&schema_nodes, input_node_descendant_index) {
                Ok((code, next)) => (code, next),
                Err(e) => {
                    self.state.add_new_error(e.clone());
                    return matches;
                }
            };

        let matcher_node = match code_node {
            None => {
                self.state.add_new_error(Error::SchemaError(
                    SchemaError::NoMatcherInListNodeChildren(input_node_descendant_index),
                ));
                return matches;
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
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        matcher_text.into(),
                    ),
                ));

                return matches;
            }
        };

        let schema_start = schema_nodes[0].byte_range().start;
        let matcher_start = matcher_node.byte_range().start - schema_start;
        let matcher_end = matcher_node.byte_range().end - schema_start;

        // Always validate prefix, even for incomplete nodes
        let prefix_schema = &self.state.schema_str()[schema_start..schema_start + matcher_start];

        // Check if we have enough input to validate the prefix (the end of the
        // prefix is the start of the matcher)
        let input_has_full_prefix = input_cursor.node().byte_range().len() >= matcher_start;

        if input_has_full_prefix {
            let prefix_input = &self.state.last_input_str()[input_cursor.node().byte_range().start
                ..input_cursor.node().byte_range().start + matcher_start];

            // Do the actual prefix comparison
            if prefix_schema != prefix_input {
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        prefix_schema.into(),
                    ),
                ));

                return matches;
            }
        } else if matcher_start > 0 && !self.is_incomplete(input_cursor) {
            // Input is too short to contain the required prefix, and we've reached EOF
            // so this is a genuine error (not just incomplete input)
            self.state.add_new_error(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    prefix_schema.into(),
                ),
            ));
            return matches;
        }

        // Skip matcher and suffix validation if node is incomplete
        if self.is_incomplete(input_cursor) {
            return matches;
        }

        let input_start = input_cursor.node().byte_range().start + matcher_start;
        let input_to_match = self.state.last_input_str()[input_start..].to_string();

        // If the matcher is for a ruler, we should expect the entire input node to be a ruler
        if matcher.is_ruler() {
            if input_cursor.node().kind() != "thematic_break" {
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::NodeTypeMismatch(
                        input_node_descendant_index,
                        input_node_descendant_index, // should be the same as the schema's.
                                                     // TODO: is this really true though?
                    ),
                ));
                return matches;
            } else {
                // It's a ruler, no further validation needed
                return matches;
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
                let input_end = input_cursor.node().byte_range().end;

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
                if let Some(id) = matcher.id() {
                    matches[id] = json!(matched_str);
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
        if self.is_list_node(&input_cursor.node()) && self.is_list_node(&schema_cursor.node()) {
            // If the input node is a list, delegate to validate_matcher_node_list
            self.validate_matcher_vs_list(input_cursor, schema_cursor);
        }

        matches
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

        let errors = state
            .errors_so_far()
            .into_iter()
            .cloned()
            .collect::<Vec<Error>>();
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
                "item": ["hello"]
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

    #[test]
    fn test_nested_repeater_list() {
        let schema = r#"
- `item1:/\w+/`{1,1}
    - `item2:/\w+/`{1,1}
"#;
        let input = r#"
- apple
    - banana
    - cherry
"#;

        let (matches, errors) = validate_str(schema, input);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(
            matches,
            json!({
                "item1": ["apple", {"item2": ["banana", "cherry"]}]
            }),
        );
    }
}
