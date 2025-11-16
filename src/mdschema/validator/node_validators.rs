use log::{debug, trace};
use serde_json::{json, Value};
use tree_sitter::{Node, TreeCursor};

use crate::mdschema::validator::{
    errors::{Error, SchemaError, SchemaViolationError},
    matcher::{get_everything_after_special_chars, Matcher},
    utils::{is_last_node, new_markdown_parser},
};

pub type NodeValidationResult = (Vec<Error>, Value);

/// Check if a node is a list (tight_list or loose_list).
fn is_list_node(node: &Node) -> bool {
    node.kind() == "tight_list" || node.kind() == "loose_list"
}

/// Find the matcher code_span node in a list of schema nodes.
/// Returns the matcher node and the next node after it, if any.
/// Returns an error if multiple matchers are found.
fn find_matcher_node<'b>(
    schema_cursor: &mut TreeCursor<'b>,
    input_cursor: &mut TreeCursor<'b>,
) -> Result<(Option<Node<'b>>, Option<Node<'b>>), Error> {
    let schema_nodes: Vec<Node> = schema_cursor
        .node()
        .children(&mut schema_cursor.clone())
        .collect();
    let input_node_descendant_index = input_cursor.descendant_index();

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
            code_node = Some(*node);
            next_node = schema_nodes.get(i + 1).copied();
        }
    }

    Ok((code_node, next_node))
}

/// Validate a text node against the schema text node.
///
/// This is a node that is just a simple literal text node. We validate that
/// the text content is identical.
pub fn validate_text_node<'b>(
    input_cursor: &mut TreeCursor<'b>,
    schema_cursor: &mut TreeCursor<'b>,
    input_str: &'b str,
    schema_str: &'b str,
    eof: bool,
) -> NodeValidationResult {
    let input_node = input_cursor.node();
    let schema_node = schema_cursor.node();

    let input_node_descendant_index = input_cursor.descendant_index();

    let mut errors = Vec::new();

    let schema_text = &schema_str[schema_node.byte_range()];
    let input_text = &input_str[input_node.byte_range()];

    debug!(
        "Comparing text: schema='{}' vs input='{}'",
        schema_text, input_text
    );

    if schema_text != input_text {
        trace!(
            "Text content mismatch at node index {}: expected '{}', got '{}'",
            input_node_descendant_index,
            schema_text,
            input_text
        );

        errors.push(Error::SchemaViolation(
            SchemaViolationError::NodeContentMismatch(
                input_node_descendant_index,
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
pub fn validate_matcher_node_list<'b>(
    input_cursor: &mut TreeCursor<'b>,
    schema_cursor: &mut TreeCursor<'b>,
    input_str: &'b str,
    schema_str: &'b str,
    eof: bool,
) -> NodeValidationResult {
    let (code_node, next_node) =
        match find_matcher_node(&mut schema_cursor.clone(), &mut input_cursor.clone()) {
            Ok((code, next)) => (code, next),
            Err(e) => return (vec![e], json!({})),
        };

    let input_root_node = input_cursor.node();
    let input_node_descendant_index = input_cursor.descendant_index();

    let schema_root_node = schema_cursor.node();

    match code_node {
        None => {
            return (
                vec![Error::SchemaError(
                    SchemaError::NoMatcherInListNodeChildren(input_node_descendant_index),
                )],
                json!({}),
            );
        }
        Some(matcher_code_node) => {
            let mut errors = Vec::new();
            let mut matches_array = Vec::new();

            // We have to create a matcher to extract the ID, even though we
            // call validate_matcher_node, which is a bit redundant.
            // We also check to make sure we are even in repeating mode here!
            let matcher_sr = &schema_str[matcher_code_node.byte_range()];
            let matcher = match Matcher::new(
                matcher_sr,
                next_node.map(|n| &schema_str[n.byte_range()]).as_deref(),
            ) {
                Ok(m) => m,
                Err(_) => {
                    return (
                        vec![Error::SchemaError(
                            SchemaError::NoMatcherInListNodeChildren(input_node_descendant_index),
                        )],
                        json!({}),
                    );
                }
            };

            // If we aren't in repeating mode, return an error
            if !matcher.is_repeated() {
                return (
                    vec![Error::SchemaViolation(
                        SchemaViolationError::NonRepeatingMatcherInListContext(
                            input_node_descendant_index,
                        ),
                    )],
                    json!({}),
                );
            }

            let current_depth = 0;

            // Count the number of list items at this depth level
            let num_list_items = input_root_node.child_count();

            let mut child_lists_to_validate = vec![(
                input_cursor.descendant_index(),
                schema_cursor.descendant_index(),
                Box::new(|| json!([])) as Box<dyn Fn() -> Value>,
            )];

            while let Some((input_descendant_index, schema_descendant_index, _nested_matches)) =
                child_lists_to_validate.pop()
            {
                input_cursor.reset(input_root_node);
                schema_cursor.reset(schema_root_node);

                input_cursor.goto_descendant(input_descendant_index);
                schema_cursor.goto_descendant(schema_descendant_index);

                'list_level_validation_loop: for input_node_list_item in
                    input_root_node.children(&mut input_root_node.walk())
                {
                    // list_item -> paragraph (child 1)
                    let _paragraph = input_node_list_item.child(1).unwrap();
                    let (child_errors, child_matches) = validate_matcher_node(
                        input_cursor,
                        schema_cursor,
                        input_str,
                        schema_str,
                        eof,
                    );
                    errors.extend(child_errors);

                    // Add the matched value from this list item
                    if let Some(obj) = child_matches.as_object() {
                        if let Some((_, value)) = obj.iter().next() {
                            matches_array.push(value.clone());
                        }
                    }

                    // Check if this list item has a nested list (child at index 2)
                    if let Some(nested_list) = input_node_list_item.child(2) {
                        if !is_list_node(&nested_list) {
                            continue 'list_level_validation_loop;
                        }

                        // Before doing anything else, check if we're allowed to go deeper
                        if let Some(max_allowed) = matcher.max_depth() {
                            if current_depth >= max_allowed {
                                trace!(
                                    "Maximum list nesting depth of {} exceeded at node index {}",
                                    max_allowed,
                                    input_node_descendant_index
                                );

                                // depth limit reached; report error
                                errors.push(Error::SchemaViolation(
                                    SchemaViolationError::NodeListTooDeep(
                                        max_allowed,
                                        input_node_descendant_index,
                                    ),
                                ));
                            }
                        }

                        // Parse the schema to get the nested list structure
                        let mut schema_parser = new_markdown_parser();

                        // This is just the excerpt we need of the schema string
                        let Some(schema_tree) = schema_parser.parse(schema_str, None) else {
                            continue 'list_level_validation_loop;
                        };

                        // Navigate to the nested list in the schema's first item
                        let Some(schema_nested_list) = schema_tree
                            .root_node()
                            // document -> list
                            .child(0)
                            // list -> list_item
                            .and_then(|n| n.child(0))
                            // list_item -> nested list ???
                            .and_then(|n| n.child(2))
                        else {
                            trace!(
                                "No nested list found in schema at node index {}",
                                input_node_descendant_index
                            );

                            continue 'list_level_validation_loop;
                        };

                        // Actually check!
                        if !is_list_node(&schema_nested_list) {
                            trace!(
                                "No nested list found in schema at node index {}",
                                input_node_descendant_index
                            );

                            continue 'list_level_validation_loop;
                        }

                        // nested list -> list_item -> paragraph
                        let Some(_schema_nested_paragraph) =
                            schema_nested_list.child(0).and_then(|item| item.child(1))
                        else {
                            trace!(
                                "No paragraph found in nested schema list at node index {}",
                                input_node_descendant_index
                            );

                            continue 'list_level_validation_loop;
                        };

                        // recurse, get matches at the next layer deep, then put
                        // them in the array at this level (using a callback to do this)
                        child_lists_to_validate.push((
                            nested_list.walk().descendant_index(),
                            schema_nested_list.walk().descendant_index(),
                            Box::new(|| json!([])) as Box<dyn Fn() -> Value>,
                        ));
                    }
                }
            }

            let mut matches = json!({});
            match matcher.id() {
                Some(id) => matches[id] = serde_json::Value::Array(matches_array.clone()),
                None => {}
            }

            // Check item count constraints based on the number of list items at this depth
            // (not the size of matches_array, which may include nested structures)
            if let Some(min) = matcher.min_items() {
                if num_list_items < min {
                    trace!(
                        "Minimum list item count of {} not met at node index {}: found {} items",
                        min,
                        input_node_descendant_index,
                        num_list_items
                    );

                    errors.push(Error::SchemaViolation(
                        SchemaViolationError::WrongListCount(
                            Some(min),
                            matcher.max_items(),
                            num_list_items,
                            input_node_descendant_index,
                        ),
                    ));
                }
            }

            if let Some(max) = matcher.max_items() {
                if num_list_items > max {
                    trace!(
                        "Maximum list item count of {} exceeded at node index {}: found {} items",
                        max,
                        input_node_descendant_index,
                        num_list_items
                    );

                    errors.push(Error::SchemaViolation(
                        SchemaViolationError::WrongListCount(
                            matcher.min_items(),
                            Some(max),
                            num_list_items,
                            input_node_descendant_index,
                        ),
                    ));
                }
            }

            (errors, matches)
        }
    }
}

/// Validate a non-repeating matcher node against the input node. A matcher node
/// looks like `id:/pattern/` in the schema. Pass the parent of the matcher
/// node, and the corresponding input node.
///
/// This method will use the cursors to walk around the input and schema nodes,
/// but will not modify them (we will walk back to their original positions).
pub fn validate_matcher_node<'b>(
    input_cursor: &mut TreeCursor<'b>,
    schema_cursor: &mut TreeCursor<'b>,
    input_str: &'b str,
    schema_str: &'b str,
    eof: bool,
) -> NodeValidationResult {
    let input_node = input_cursor.node();
    let schema_node = schema_cursor.node();

    let schema_node_children: Vec<Node> = schema_node.children(schema_cursor).collect();
    let input_node_descendant_index = input_cursor.descendant_index();

    let is_incomplete = !eof && is_last_node(input_str, &input_node);

    let mut errors = Vec::new();
    let mut matches = json!({});

    let (code_node, next_node) = match find_matcher_node(input_cursor, schema_cursor) {
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

    let schema_start = schema_node_children[0].byte_range().start;
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
            let schema_end = schema_node_children.last().unwrap().byte_range().end;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::utils::new_markdown_parser;
    use tree_sitter::Node;

    /// Helper function to create parsers and nodes for text validation tests
    fn get_text_validator(schema: &str, input: &str, eof: bool) -> (Vec<Error>, Value) {
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_node = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input node");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let schema_node = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema node");

        validate_text_node(&input_node, 0, &schema_node, input, schema, eof)
    }

    fn get_matcher_validator(schema: &str, input: &str, eof: bool) -> (Vec<Error>, Value) {
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_node = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input node");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema root child")
            .children(&mut schema_cursor)
            .collect();

        validate_matcher_node(&input_node, 0, &schema_nodes, input, schema, eof)
    }

    fn get_list_matcher_validator(
        schema: &str,
        input: &str,
        eof: bool,
    ) -> (Vec<Error>, serde_json::Value) {
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");

        let input_node = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input node");
        assert_eq!(input_node.kind(), "tight_list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let mut schema_cursor = schema_tree.walk();

        let schema_root_child = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema root child");

        // If the schema is a list, we need to get the paragraph content from the first list item
        let schema_nodes: Vec<Node> = if is_list_node(&schema_root_child) {
            let first_list_item = schema_root_child
                .child(0)
                .expect("Failed to get first list item");
            let paragraph = first_list_item
                .child(1)
                .expect("Failed to get paragraph from list item");
            paragraph.children(&mut schema_cursor).collect()
        } else {
            schema_root_child.children(&mut schema_cursor).collect()
        };

        // The schema nodes should contain at least one code_span (the matcher)
        assert!(
            schema_nodes.iter().any(|n| n.kind() == "code_span"),
            "Schema must contain at least one code_span matcher"
        );

        let (errors, matches) =
            validate_matcher_node_list(&input_node, 0, &schema_nodes, input, schema, eof);
        return (
            errors,
            serde_json::Value::Array(matches.get("test").unwrap().as_array().unwrap().clone()),
        );
    }

    #[test]
    fn test_different_text_content_nodes_mismatch() {
        let schema = "Hello world";
        let input = "Hello there";

        let (errors, _) = get_text_validator(schema, input, true);

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, expected)) => {
                assert_eq!(expected, "Hello world");
            }
            _ => panic!("Unexpected error type"),
        }
    }

    #[test]
    fn test_same_text_content_nodes_match() {
        let schema = "Hello world";
        let input = "Hello world";

        let (errors, _) = get_text_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_with_prefix_and_suffix() {
        let schema = "Hello `id:/foo/` world";
        let input = "Hello foo world";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_with_regex() {
        let schema = "Value: `num:/[0-9]+/`";
        let input = "Value: 12345";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_only_prefix() {
        let schema = "Start `id:/test/`";
        let input = "Start test";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_only_suffix() {
        let schema = "`id:/test/` end";
        let input = "test end";

        let (errors, matches) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        println!("matches: {:?}", matches);
        assert_eq!(matches.as_object().unwrap().len(), 1);
        assert_eq!(
            matches
                .as_object()
                .unwrap()
                .get("id")
                .unwrap()
                .as_str()
                .unwrap(),
            "test"
        );
    }

    #[test]
    fn test_validate_matcher_no_prefix_or_suffix() {
        let schema = "`id:/test/`";
        let input = "test";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }

    #[test]
    fn test_validate_matcher_fails_on_prefix_mismatch() {
        let schema = "Hello `id:/foo/` world";
        let input = "Goodbye foo world";

        let (errors, _) = get_matcher_validator(schema, input, true);

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeContentMismatch(_, expected)) => {
                assert_eq!(expected, "Hello ");
            }
            _ => panic!("Expected NodeContentMismatch error"),
        }
    }

    #[test]
    fn test_validate_matcher_list_expects_errors_on_pattern_mismatch() {
        let schema = "`test:/[0-9]/`+";
        let input = "- 1\n- a\n- 3\n- b\n- 5";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(!errors.is_empty(), "Expected errors but got none");
        assert_eq!(errors.len(), 2, "Expected 2 errors for 'a' and 'b'");

        // Verify that valid matches were still captured
        println!("{}", matches);
        let matches_arr = matches.as_array().unwrap();
        assert!(matches_arr
            .iter()
            .find(|m| m.as_str().unwrap() == "1")
            .is_some());
    }

    #[test]
    fn test_duplicate_special_repeating_char_allowed() {
        let schema = "`test:/[0-9]/`++++++";
        let input = "- 1\n- 2\n- 3";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 3);
        for i in 0..3 {
            assert_eq!(matches_arr[i].as_str().unwrap(), (i + 1).to_string());
        }
    }

    #[test]
    fn test_validate_matcher_list_for_simple_digit_pattern() {
        let schema = "`test:/[0-9]+/`+";
        let input = "- 1\n- 2\n- 3\n- 4\n- 5";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 5);
        for i in 0..5 {
            assert_eq!(matches_arr[i].as_str().unwrap(), (i + 1).to_string());
        }
    }

    #[test]
    fn test_prefix_on_list_node_with_repeater() {
        let schema = "Item: `test:/[0-9]+/`+";
        let input = "- Item: 1\n- Item: 2\n- Item: 3";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 3);
        for i in 0..3 {
            assert_eq!(matches_arr[i].as_str().unwrap(), (i + 1).to_string());
        }
    }

    #[test]
    fn test_prefix_with_partial_match_in_list() {
        let schema = "- t `test:/\\d/`++";
        let input = "- 1\n- t 2\n- 3";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        // Should have 2 errors: items "- 1" and "- 3" don't have the "t " prefix
        assert_eq!(errors.len(), 2, "Expected 2 errors but got: {:?}", errors);

        // Only the second item should match
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(
            matches_arr.len(),
            1,
            "Expected 1 match but got: {:?}",
            matches_arr
        );
        assert_eq!(matches_arr[0].as_str().unwrap(), "2");
    }

    #[test]
    fn test_numbered_list_simple_pattern() {
        let schema = "1. `test:/[0-9]+/`+";
        let input = "1. 10\n2. 20\n3. 30";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 3);
        assert_eq!(matches_arr[0].as_str().unwrap(), "10");
        assert_eq!(matches_arr[1].as_str().unwrap(), "20");
        assert_eq!(matches_arr[2].as_str().unwrap(), "30");
    }

    #[test]
    fn test_numbered_list_with_prefix() {
        let schema = "1. Value: `test:/[a-z]+/`+";
        let input = "1. Value: foo\n2. Value: bar\n3. Value: baz";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 3);
        assert_eq!(matches_arr[0].as_str().unwrap(), "foo");
        assert_eq!(matches_arr[1].as_str().unwrap(), "bar");
        assert_eq!(matches_arr[2].as_str().unwrap(), "baz");
    }

    #[test]
    fn test_numbered_list_with_prefix_mismatch() {
        let schema = "1. Item: `test:/\\d+/`+";
        let input = "1. Item: 1\n2. Wrong: 2\n3. Item: 3";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        // Should have 1 error for item 2 which has "Wrong: " instead of "Item: "
        assert_eq!(errors.len(), 1, "Expected 1 error but got: {:?}", errors);

        // Items 1 and 3 should match
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(
            matches_arr.len(),
            2,
            "Expected 2 matches but got: {:?}",
            matches_arr
        );
        assert_eq!(matches_arr[0].as_str().unwrap(), "1");
        assert_eq!(matches_arr[1].as_str().unwrap(), "3");
    }

    #[test]
    fn test_nested_list_with_inner_repeater() {
        let schema = "- Outer\n  - `test:/[a-z]+/`+";
        let input = "- Outer\n  - foo\n  - bar\n  - baz";

        // For nested lists, we need to manually navigate to the inner list
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");

        // Get the outer list's first item's nested list (child at index 2)
        let outer_list = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get outer list");
        let first_item = outer_list.child(0).expect("Failed to get first list item");
        let inner_list = first_item.child(2).expect("Failed to get nested list");
        assert_eq!(inner_list.kind(), "tight_list");

        // Parse schema and get the nested list's first item's paragraph nodes
        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");

        let schema_outer_list = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema outer list");
        let schema_first_item = schema_outer_list
            .child(0)
            .expect("Failed to get schema first item");
        let schema_inner_list = schema_first_item
            .child(2)
            .expect("Failed to get schema nested list");
        let schema_inner_first_item = schema_inner_list
            .child(0)
            .expect("Failed to get schema inner first item");
        let schema_paragraph = schema_inner_first_item
            .child(1)
            .expect("Failed to get schema paragraph");

        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<tree_sitter::Node> =
            schema_paragraph.children(&mut schema_cursor).collect();

        let (errors, matches) =
            validate_matcher_node_list(&inner_list, 0, &schema_nodes, input, schema, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.get("test").unwrap().as_array().unwrap();
        assert_eq!(matches_arr.len(), 3);
        assert_eq!(matches_arr[0].as_str().unwrap(), "foo");
        assert_eq!(matches_arr[1].as_str().unwrap(), "bar");
        assert_eq!(matches_arr[2].as_str().unwrap(), "baz");
    }

    #[test]
    fn test_nested_lists_both_with_repeaters() {
        let schema = "- Outer `outer:/[0-9]+/`++\n  - Inner `inner:/[a-z]+/`++";
        let input = "- Outer 1\n  - Inner a\n  - Inner b\n- Outer 2\n  - Inner c\n  - Inner d";

        // For this test, we'll parse the outer list only since the helper
        // function only handles one level of lists with repeaters
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");

        let input_node = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input node");
        assert_eq!(input_node.kind(), "tight_list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let mut schema_cursor = schema_tree.walk();

        let schema_root_child = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema root child");

        let schema_nodes: Vec<tree_sitter::Node> = if is_list_node(&schema_root_child) {
            let first_list_item = schema_root_child
                .child(0)
                .expect("Failed to get first list item");
            let paragraph = first_list_item
                .child(1)
                .expect("Failed to get paragraph from list item");
            paragraph.children(&mut schema_cursor).collect()
        } else {
            schema_root_child.children(&mut schema_cursor).collect()
        };

        let (errors, matches) =
            validate_matcher_node_list(&input_node, 0, &schema_nodes, input, schema, true);

        // Now captures nested structure too!
        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        // Should have outer values and nested structures
        let expected = json!({
            "outer": [
                "1",
                {"inner": ["a", "b"]},
                "2",
                {"inner": ["c", "d"]}
            ]
        });
        assert_eq!(matches, expected);
    }

    #[test]
    fn test_fully_nested_lists_with_repeaters() {
        let schema = "- `num1:/\\d/`++\n  - `num2:/\\d/`++";
        let input = "- 1\n  - 2\n- 3\n  - 4";

        // Parse input
        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_list = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input list");
        assert_eq!(input_list.kind(), "tight_list");

        // Parse schema
        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let schema_list = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema list");
        let schema_first_item = schema_list.child(0).expect("Failed to get first item");
        let schema_paragraph = schema_first_item.child(1).expect("Failed to get paragraph");

        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<tree_sitter::Node> =
            schema_paragraph.children(&mut schema_cursor).collect();

        // Validate the outer list
        let (errors, matches) =
            validate_matcher_node_list(&input_list, 0, &schema_nodes, input, schema, true);

        println!("Errors: {:?}", errors);
        println!(
            "Matches: {}",
            serde_json::to_string_pretty(&matches).unwrap()
        );

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        // Now we should get the full nested structure!
        let num1_matches = matches.get("num1").unwrap().as_array().unwrap();
        assert_eq!(num1_matches.len(), 4); // 2 outer values + 2 nested objects
        assert_eq!(num1_matches[0].as_str().unwrap(), "1");
        assert_eq!(
            num1_matches[1].get("num2").unwrap().as_array().unwrap()[0]
                .as_str()
                .unwrap(),
            "2"
        );
        assert_eq!(num1_matches[2].as_str().unwrap(), "3");
        assert_eq!(
            num1_matches[3].get("num2").unwrap().as_array().unwrap()[0]
                .as_str()
                .unwrap(),
            "4"
        );

        // Verify the full structure
        let expected = json!({
            "num1": ["1", {"num2": ["2"]}, "3", {"num2": ["4"]}]
        });
        assert_eq!(matches, expected);
    }

    #[test]
    fn test_get_everything_after_special_chars() {
        let input = "+++HelloWorld";
        let result = get_everything_after_special_chars(input);
        assert_eq!(result, "HelloWorld");

        let input_no_special = "HelloWorld";
        let result_no_special = get_everything_after_special_chars(input_no_special);
        assert_eq!(result_no_special, "HelloWorld");

        let input_mixed = "+-*/HelloWorld";
        let result_mixed = get_everything_after_special_chars(input_mixed);
        assert_eq!(result_mixed, "-*/HelloWorld");
    }

    #[test]
    fn test_nested_list_with_multiple_inner_items() {
        let schema = "- `num1:/\\d/`++\n  - `num2:/\\d/`++";
        let input = "- 1\n  - 2\n  - 3\n  - 4\n- 5\n  - 6";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_list = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let schema_list = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema list");
        let schema_first_item = schema_list.child(0).expect("Failed to get first item");
        let schema_paragraph = schema_first_item.child(1).expect("Failed to get paragraph");

        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<tree_sitter::Node> =
            schema_paragraph.children(&mut schema_cursor).collect();

        let (errors, matches) =
            validate_matcher_node_list(&input_list, 0, &schema_nodes, input, schema, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        let expected = json!({
            "num1": ["1", {"num2": ["2", "3", "4"]}, "5", {"num2": ["6"]}]
        });
        assert_eq!(matches, expected);
    }

    #[test]
    fn test_nested_list_without_nested_items() {
        let schema = "- `num1:/\\d/`++\n  - `num2:/\\d/`++";
        let input = "- 1\n- 2";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_list = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let schema_list = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema list");
        let schema_first_item = schema_list.child(0).expect("Failed to get first item");
        let schema_paragraph = schema_first_item.child(1).expect("Failed to get paragraph");

        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<tree_sitter::Node> =
            schema_paragraph.children(&mut schema_cursor).collect();

        let (errors, matches) =
            validate_matcher_node_list(&input_list, 0, &schema_nodes, input, schema, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        // Just outer values, no nested objects
        let expected = json!({
            "num1": ["1", "2"]
        });
        assert_eq!(matches, expected);
    }

    #[test]
    fn test_nested_list_with_prefix() {
        let schema = "- Item `num1:/\\d/`++\n  - Sub `num2:/\\d/`++";
        let input = "- Item 1\n  - Sub 2\n- Item 3\n  - Sub 4";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_list = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let schema_list = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema list");
        let schema_first_item = schema_list.child(0).expect("Failed to get first item");
        let schema_paragraph = schema_first_item.child(1).expect("Failed to get paragraph");

        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<tree_sitter::Node> =
            schema_paragraph.children(&mut schema_cursor).collect();

        let (errors, matches) =
            validate_matcher_node_list(&input_list, 0, &schema_nodes, input, schema, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        let expected = json!({
            "num1": ["1", {"num2": ["2"]}, "3", {"num2": ["4"]}]
        });
        assert_eq!(matches, expected);
    }

    #[test]
    fn test_nested_numbered_lists() {
        let schema = "1. `num1:/\\d/`++\n   1. `num2:/\\d/`++";
        let input = "1. 1\n   1. 2\n2. 3\n   1. 4";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_list = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let schema_list = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema list");
        let schema_first_item = schema_list.child(0).expect("Failed to get first item");
        let schema_paragraph = schema_first_item.child(1).expect("Failed to get paragraph");

        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<tree_sitter::Node> =
            schema_paragraph.children(&mut schema_cursor).collect();

        let (errors, matches) =
            validate_matcher_node_list(&input_list, 0, &schema_nodes, input, schema, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        let expected = json!({
            "num1": ["1", {"num2": ["2"]}, "3", {"num2": ["4"]}]
        });
        assert_eq!(matches, expected);
    }

    #[test]
    fn test_nested_list_with_complex_patterns() {
        let schema = "- `name:/[a-z]+/`++\n  - `value:/[0-9]+/`++";
        let input = "- alice\n  - 100\n  - 200\n- bob\n  - 300";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");
        let input_list = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");
        let schema_list = schema_tree
            .root_node()
            .child(0)
            .expect("Failed to get schema list");
        let schema_first_item = schema_list.child(0).expect("Failed to get first item");
        let schema_paragraph = schema_first_item.child(1).expect("Failed to get paragraph");

        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<tree_sitter::Node> =
            schema_paragraph.children(&mut schema_cursor).collect();

        let (errors, matches) =
            validate_matcher_node_list(&input_list, 0, &schema_nodes, input, schema, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        let expected = json!({
            "name": ["alice", {"value": ["100", "200"]}, "bob", {"value": ["300"]}]
        });
        assert_eq!(matches, expected);
    }

    #[test]
    fn test_nested_list_with_mismatched_inner_pattern() {
        let schema_str = "- `num1:/\\d/`++\n  - `num2:/\\d/`++";
        let input_str = "- 1\n  - 2\n  - bad\n- 3\n  - 4";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input_str, None)
            .expect("Failed to parse input");
        let input_list = input_tree
            .root_node()
            .child(0)
            .expect("Failed to get input list");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema_str, None)
            .expect("Failed to parse schema");

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_list.walk();

        let (errors, matches) = validate_matcher_node_list(
            &mut input_cursor,
            &mut schema_cursor,
            input_str,
            schema_str,
            true,
        );

        // Should have 1 error for "bad" not matching the pattern
        assert_eq!(errors.len(), 1, "Expected 1 error but got: {:?}", errors);

        // Should still capture valid matches
        let num1_array = matches.get("num1").unwrap().as_array().unwrap();
        assert_eq!(num1_array[0].as_str().unwrap(), "1");
        assert_eq!(
            num1_array[1].get("num2").unwrap().as_array().unwrap()[0]
                .as_str()
                .unwrap(),
            "2"
        );
        assert_eq!(num1_array[2].as_str().unwrap(), "3");
        assert_eq!(
            num1_array[3].get("num2").unwrap().as_array().unwrap()[0]
                .as_str()
                .unwrap(),
            "4"
        );
    }

    #[test]
    fn test_deeply_nested_three_levels() {
        // This tests that we properly handle recursive nesting
        // Use +++ (3 pluses) to allow 3 levels of depth
        let schema = "- `l1:/\\d/`+++\n  - `l2:/\\d/`+++";
        let input = "- 1\n  - 2\n    - 3\n- 4\n  - 5";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input, None)
            .expect("Failed to parse input");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema, None)
            .expect("Failed to parse schema");

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        let (errors, matches) =
            validate_matcher_node_list(&mut input_cursor, &mut schema_cursor, input, schema, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );

        // The implementation recursively handles nesting, so item "2" has a nested list "3"
        // which gets captured as l2: [2, {l2: [3]}]
        let expected = json!({
            "l1": ["1", {"l2": ["2", {"l2": ["3"]}]}, "4", {"l2": ["5"]}]
        });
        assert_eq!(matches, expected);
    }

    #[test]
    fn test_depth_limit_enforced_with_error() {
        // Use + (1 plus) to allow max depth of 1 (no nesting allowed)
        // But provide 2 levels of nesting - should error on the second level
        let schema_str = "- `num:/\\d/`+";
        let input_str = "- 1\n  - 2\n- 3";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser
            .parse(input_str, None)
            .expect("Failed to parse input");

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser
            .parse(schema_str, None)
            .expect("Failed to parse schema");

        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        let (errors, matches) = validate_matcher_node_list(
            &mut input_cursor,
            &mut schema_cursor,
            input_str,
            schema_str,
            true,
        );

        // Should have an error about nesting too deep
        assert_eq!(errors.len(), 1, "Expected 1 error but got: {:?}", errors);

        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::NodeListTooDeep(
                max_depth,
                _node_index,
            )) => {
                assert_eq!(*max_depth, 1, "Expected max_depth to be 1");
            }
            _ => panic!("Expected NodeListTooDeep error but got: {:?}", errors[0]),
        }

        // Matches should be have the valid outer items
        let num_array = matches.get("num").unwrap().as_array().unwrap();
        assert_eq!(num_array.len(), 2);
        assert_eq!(num_array[0].as_str().unwrap(), "1");
        assert_eq!(num_array[1].as_str().unwrap(), "3");
    }

    #[test]
    fn test_list_item_count_too_few() {
        // Schema requires at least 3 items {3,}
        let schema = "`test:/\\d+/`{3,}+";
        let input = "- 1\n- 2";

        let (errors, _matches) = get_list_matcher_validator(schema, input, true);

        assert_eq!(errors.len(), 1, "Expected 1 error but got: {:?}", errors);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::WrongListCount(min, max, actual, _)) => {
                assert_eq!(*min, Some(3));
                assert_eq!(*max, None);
                assert_eq!(*actual, 2);
            }
            _ => panic!("Expected WrongListCount error but got: {:?}", errors[0]),
        }
    }

    #[test]
    fn test_list_item_count_too_many() {
        // Schema allows at most 2 items {,2}
        let schema = "`test:/\\d+/`{,2}+";
        let input = "- 1\n- 2\n- 3";

        let (errors, _matches) = get_list_matcher_validator(schema, input, true);

        assert_eq!(errors.len(), 1, "Expected 1 error but got: {:?}", errors);
        match &errors[0] {
            Error::SchemaViolation(SchemaViolationError::WrongListCount(min, max, actual, _)) => {
                assert_eq!(*min, None);
                assert_eq!(*max, Some(2));
                assert_eq!(*actual, 3);
            }
            _ => panic!("Expected WrongListCount error but got: {:?}", errors[0]),
        }
    }

    #[test]
    fn test_list_item_count_in_range() {
        // Schema requires 2-4 items {2,4}
        let schema = "`test:/\\d+/`{2,4}+";
        let input = "- 1\n- 2\n- 3";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 3);
    }

    #[test]
    fn test_list_item_count_exact_min() {
        // Schema requires at least 3 items {3,}
        let schema = "`test:/\\d+/`{3,}+";
        let input = "- 1\n- 2\n- 3";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 3);
    }

    #[test]
    fn test_list_item_count_exact_max() {
        // Schema allows at most 3 items {,3}
        let schema = "`test:/\\d+/`{,3}+";
        let input = "- 1\n- 2\n- 3";

        let (errors, matches) = get_list_matcher_validator(schema, input, true);

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
        let matches_arr = matches.as_array().unwrap();
        assert_eq!(matches_arr.len(), 3);
    }
}
