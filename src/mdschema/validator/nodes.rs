use log::{debug, trace};
use serde_json::{json, Value};
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::{Error, SchemaError, SchemaViolationError},
    matcher::{get_everything_after_special_chars, Matcher},
    state::ValidatorState,
    utils::{is_last_node, new_markdown_parser},
};

pub type NodeValidationResult = (Vec<Error>, Value);

/// A node validator that validates input nodes against schema nodes.
pub struct NodeValidator<'a> {
    state: &'a ValidatorState,
}

impl<'a> NodeValidator<'a> {
    pub fn new(state: &'a ValidatorState) -> Self {
        Self { state }
    }

    /// Check if a node is a list (tight_list or loose_list).
    fn is_list_node(&self, node: &Node) -> bool {
        match node.kind() {
            "tight_list" | "loose_list" => true,
            _ => false,
        }
    }

    /// Validate a text node against the schema text node.
    ///
    /// This is a node that is just a simple literal text node. We validate that
    /// the text content is identical.
    pub fn validate_text_node(
        &self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) -> NodeValidationResult {
        let schema_node = schema_cursor.node();
        let input_node = input_cursor.node();

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
                input_cursor.descendant_index(),
                schema_text,
                input_text
            );

            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_cursor.descendant_index(),
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
    pub fn validate_matcher_node_list(
        &self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) -> NodeValidationResult {
        assert!(self.is_list_node(&input_cursor.node()));
        assert!(self.is_list_node(&schema_cursor.node()));

        let mut errors = Vec::new();
        let mut matches = json!({});

        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        schema_cursor.goto_next_sibling();
        assert_eq!(schema_cursor.node().kind(), "paragraph");

        // Now validate each input node's list items against the schema's single list item

        if !input_cursor.goto_first_child() {
            // No children to validate
            return (errors, matches);
        }

        input_cursor.goto_first_child();
        input_cursor.goto_next_sibling();
        assert_eq!(input_cursor.node().kind(), "paragraph");

        loop {
            let (node_errors, node_matches) =
                self.validate_matcher_node(&mut input_cursor.clone(), &mut schema_cursor.clone());

            errors.extend(node_errors);
            for (key, value) in node_matches.as_object().unwrap() {
                matches[key] = value.clone();
            }

            if !input_cursor.goto_next_sibling() || !input_cursor.goto_next_sibling() {
                break;
            }
        }

        (errors, matches)
    }

    /// Validate a matcher node against the input node.
    ///
    /// A matcher node looks like `id:/pattern/` in the schema.
    ///
    /// Pass the parent of the matcher node, and the corresponding input node.
    pub fn validate_matcher_node(
        &self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) -> NodeValidationResult {
        if self.is_list_node(&input_cursor.node()) && self.is_list_node(&schema_cursor.node()) {
            // If the input node is a list, delegate to validate_matcher_node_list
            return self.validate_matcher_node_list(input_cursor, schema_cursor);
        }

        let input_str = self.state.last_input_str();
        let schema_str = self.state.schema_str();
        let eof = self.state.got_eof();
        let input_node = input_cursor.node();
        let schema_nodes = schema_cursor
            .node()
            .named_children(&mut schema_cursor.clone())
            .collect::<Vec<Node>>();
        let input_node_descendant_index = input_cursor.descendant_index();

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_list_validation() {
        let mut parser = new_markdown_parser();
        env_logger::Builder::from_default_env()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .init();

        // Test a simple list with matcher
        let schema = "- `id:/pattern/`";
        let input = "- pattern";

        let schema_tree = parser.parse(schema, None).unwrap();
        let input_tree = parser.parse(input, None).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the list nodes
        schema_cursor.goto_first_child(); // -> tight_list
        input_cursor.goto_first_child(); // -> tight_list

        let state = ValidatorState::new(schema.to_string(), input.to_string(), true);
        let validator = NodeValidator::new(&state);

        assert!(validator.is_list_node(&schema_cursor.node()));
        assert!(validator.is_list_node(&input_cursor.node()));

        let (errors, value) =
            validator.validate_matcher_node(&mut input_cursor, &mut schema_cursor);

        assert!(errors.is_empty(), "Errors: {:?}", errors);
        assert_eq!(value["id"], "pattern");
    }

    #[test]
    fn test_nested_list_validation() {
        let mut parser = new_markdown_parser();
        env_logger::Builder::from_default_env()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .init();

        // Test a nested list with matchers
        let schema = "- `id1:/item1/`\n  - `id2:/item2/`";
        let input = "- item1\n  - item2";

        let schema_tree = parser.parse(schema, None).unwrap();
        let input_tree = parser.parse(input, None).unwrap();

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        // Navigate to the outer list nodes
        schema_cursor.goto_first_child(); // -> tight_list
        input_cursor.goto_first_child(); // -> tight_list

        let state = ValidatorState::new(schema.to_string(), input.to_string(), true);
        let validator = NodeValidator::new(&state);

        assert!(validator.is_list_node(&schema_cursor.node()));
        assert!(validator.is_list_node(&input_cursor.node()));

        let (errors, value) =
            validator.validate_matcher_node(&mut input_cursor, &mut schema_cursor);

        assert!(errors.is_empty(), "Errors: {:?}", errors);
        assert_eq!(value["id1"], "item1");
        assert_eq!(value["id2"], "item2");
    }
}
