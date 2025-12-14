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

    pub fn validate(&mut self) -> (usize, usize) {
        let new_matches = self.validate_node_pair(
            &mut self.input_cursor.clone(),
            &mut self.schema_cursor.clone(),
        );

        self.state.join_new_matches(new_matches);

        // TODO: this is wrong, we never get the newest indexes
        (
            self.input_cursor.descendant_index(),
            self.schema_cursor.descendant_index(),
        )
    }

    fn is_incomplete(&self, input_cursor: &mut TreeCursor) -> bool {
        !self.state.got_eof() && is_last_node(self.state.last_input_str(), &input_cursor.node())
    }

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

        // TODO: do we need this?
        let input_cursor = &mut input_cursor.clone();
        let schema_cursor = &mut schema_cursor.clone();

        let mut matches = json!({});

        // TODO: single list item support
        let is_schema_specified_list_node_result =
            extract_list_matcher(input_cursor, schema_cursor, self.state.schema_str());

        let input_is_text_node = input_cursor.node().kind() == "text";

        // It's a paragraph and it has a single text child
        // TODO: support all types, including bold etc
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
                SchemaError::MultipleMatchersInNodeChildren {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    received: schema_direct_children_code_node_count,
                    expected: 1,
                },
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
        } else if is_schema_specified_list_node_result.is_ok() {
            // TODO: Use this matcher!!
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
            if extract_list_matcher(input_cursor, schema_cursor, self.state.schema_str()).is_ok() { // TODO: Use this matcher!!
                 // In the repeating list node case that we already took care of this situation is fine
                 // TODO: have we made sure that the repeating list had a +?
            } else if self.state.got_eof() {
                // TODO: this feels wrong, we should check to make sure that when eof is false we detect nested incomplete nodes too
                self.state.add_new_error(Error::SchemaViolation(
                    SchemaViolationError::ChildrenLengthMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_cursor.descendant_index(),
                        expected: schema_cursor.node().child_count(),
                        actual: input_cursor.node().child_count(),
                    },
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
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_text.into(),
                },
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

        debug_assert!(
            is_list_node(&input_cursor.node()),
            "Input node is not a list, got {}",
            input_cursor.node().kind()
        );
        debug_assert!(
            is_list_node(&schema_cursor.node()),
            "Schema node is not a list, got {}",
            schema_cursor.node().kind()
        );

        let input_list_node = input_cursor.node();

        let input_list_children_count = input_list_node.children(&mut input_cursor.clone()).count();

        schema_cursor.goto_first_child(); // we're at a list_item
        assert_eq!(schema_cursor.node().kind(), "list_item");
        schema_cursor.goto_first_child(); // we're at a list_marker
        assert_eq!(schema_cursor.node().kind(), "list_marker");
        schema_cursor.goto_next_sibling(); // list_marker -> content (may be paragraph)

        // Get the matcher for this level
        let matcher_str = &self.state.schema_str()
            [schema_cursor.node().child(0).unwrap().byte_range()]
        .to_string();

        let child1_text = schema_cursor
            .node()
            .child(1)
            .map(|child1| &self.state.schema_str()[child1.byte_range()]);

        let main_matcher = Matcher::new(matcher_str.as_str(), child1_text).unwrap(); // TODO: don't unwrap

        // When there are multiple nodes in the input list we require a
        // repeating matcher
        if !main_matcher.is_repeated() && input_list_children_count > 1 {
            self.state.add_new_error(Error::SchemaViolation(
                SchemaViolationError::NonRepeatingMatcherInListContext {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                },
            ));
        }

        let main_matcher_id = main_matcher.id();
        let mut main_items = Vec::new();
        let mut notes_objects = Vec::new();

        // Process each list item at this level
        for child in input_list_node.children(
            &mut input_cursor.clone(), // TODO: don't clone cursor
        ) {
            let mut child_cursor = child.walk();

            assert_eq!(child_cursor.node().kind(), "list_item");

            if !child_cursor.goto_first_child() {
                continue;
            }

            assert_eq!(child_cursor.node().kind(), "list_marker");

            if !child_cursor.goto_next_sibling() {
                continue;
            }

            // Process paragraph if present
            if child_cursor.node().kind() == "paragraph" {
                let paragraph_text =
                    self.state.last_input_str()[child_cursor.node().byte_range()].trim();

                main_items.push(json!(paragraph_text));

                let has_nested_list = child_cursor.goto_next_sibling();
                if has_nested_list && is_list_node(&child_cursor.node()) {
                    // Save a copy of the schema cursor
                    let mut schema_list_cursor = schema_cursor.clone();

                    // Navigate to the nested list in the schema
                    let schema_has_nested_list = schema_list_cursor.goto_next_sibling();

                    if schema_has_nested_list && is_list_node(&schema_list_cursor.node()) {
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
            } else {
                todo!(
                    "nested lists not supported, got {}",
                    child_cursor.node().kind()
                )
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
        input_cursor: &mut TreeCursor, // we know it's text
        schema_cursor: &mut TreeCursor,
    ) -> Value {
        let input_cursor = &mut input_cursor.clone();
        let schema_cursor = &mut schema_cursor.clone();

        let mut matches = json!({});

        let schema_nodes = schema_cursor
            .node()
            .children(&mut schema_cursor.clone())
            .collect::<Vec<Node>>();

        let input_node_descendant_index = input_cursor.descendant_index();

        let matcher = extract_text_matcher(input_cursor, schema_cursor, self.state.schema_str()).unwrap();

        let matcher_node = schema_nodes[0].clone();
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
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_node_descendant_index,
                        expected: prefix_schema.into(),
                    },
                ));

                return matches;
            }
        } else if matcher_start > 0 && !self.is_incomplete(input_cursor) {
            // Input is too short to contain the required prefix, and we've reached EOF
            // so this is a genuine error (not just incomplete input)
            self.state.add_new_error(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_node_descendant_index,
                    expected: prefix_schema.into(),
                },
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
                    SchemaViolationError::NodeTypeMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_node_descendant_index,
                    },
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
                        SchemaViolationError::NodeContentMismatch {
                            schema_index: schema_cursor.descendant_index(),
                            input_index: input_node_descendant_index,
                            expected: suffix_schema.into(),
                        },
                    ));
                } else {
                    let suffix_input = &self.state.last_input_str()[suffix_start..input_end];

                    if suffix_schema != suffix_input {
                        self.state.add_new_error(Error::SchemaViolation(
                            SchemaViolationError::NodeContentMismatch {
                                schema_index: schema_cursor.descendant_index(),
                                input_index: input_node_descendant_index,
                                expected: suffix_schema.into(),
                            },
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
                    SchemaViolationError::NodeContentMismatch {
                        schema_index: schema_cursor.descendant_index(),
                        input_index: input_node_descendant_index,
                        expected: matcher.pattern().to_string(),
                    },
                ));
            }
        };

        // Otherwise, check if the nodes are both list nodes
        if is_list_node(&input_cursor.node()) && is_list_node(&schema_cursor.node()) {
            // If the input node is a list, delegate to validate_matcher_node_list
            self.validate_matcher_vs_list(input_cursor, schema_cursor);
        }

        matches
    }
}

/// Check if a node is a list (tight_list or loose_list).
fn is_list_node(node: &Node) -> bool {
    match node.kind() {
        "tight_list" | "loose_list" => true,
        _ => false,
    }
}

/// For an input list and a schema list, extract the matcher from the schema list item.
///
/// For example:
///
/// ```markdown
/// - foo:/test/`{1,2}
/// ```
///
/// The matcher would be for `/test/`{1,2}`.
///
/// But if the matcher list contained multiple items, we would return an error.
fn extract_list_matcher(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<Matcher, Error> {
    if !is_list_node(&input_cursor.node()) || !is_list_node(&schema_cursor.node()) {
        return Err(Error::SchemaError(SchemaError::BadListMatcher {
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
        }));
    }

    let schema_children_count = schema_cursor.node().child_count();
    let input_children_count = input_cursor.node().child_count();

    // Schema should have exactly one child (the template list item)
    // Input should have one or more children (the actual list items)
    if schema_children_count == 1 && input_children_count >= 1 {
        // A matcher looks like `foo:/test/`{1,2}. The {1,2} / anything after are the "extras".
        let matcher_extras_str = match schema_cursor.node().child(0) {
            Some(child) if child.kind() == "text" => child.utf8_text(schema_str.as_bytes()).ok(),
            _ => None,
        };

        Matcher::new(schema_str, matcher_extras_str).map_err(|e| {
            Error::SchemaError(SchemaError::MatcherError {
                error: e,
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
            })
        })
    } else {
        Err(Error::SchemaError(SchemaError::BadListMatcher {
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
        }))
    }
}

/// For an input text and a schema text, extract the matcher and extras.
fn extract_text_matcher(
    input_cursor: &mut TreeCursor,
    schema_cursor: &mut TreeCursor,
    schema_str: &str,
) -> Result<Matcher, Error> {
    let matcher_extras_str = match schema_cursor.node().child(0) {
        Some(child) if child.kind() == "text" => child.utf8_text(schema_str.as_bytes()).ok(),
        _ => None,
    };

    Matcher::new(schema_str, matcher_extras_str).map_err(|e| {
        Error::SchemaError(SchemaError::MatcherError {
            error: e,
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
        })
    })
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

    #[test]
    fn test_single_list_item() {
        let schema = "- `item:/\\w+/`";
        let input = "- hello";

        let (matches, errors) = validate_str(schema, input);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(matches, json!({"item": "hello"}));
    }
}
