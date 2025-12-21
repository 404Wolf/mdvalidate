use crate::mdschema::validator::errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError};
#[cfg(test)]
use serde_json::Value;
use tree_sitter::{Node, TreeCursor};

#[cfg(test)]
pub fn validate_str(schema: &str, input: &str) -> (Value, Vec<ValidationError>) {
    use crate::mdschema::validator::{utils::new_markdown_parser};
    use crate::mdschema::validator::validator_state::ValidatorState;

    let mut state = ValidatorState::new(schema.to_string(), input.to_string(), true);

    let mut parser = new_markdown_parser();
    let schema_tree = parser.parse(schema, None).unwrap();
    let input_tree = parser.parse(input, None).unwrap();

    {
        use crate::mdschema::validator::node_walker::NodeWalker;

        let mut node_validator = NodeWalker::new(&mut state, input_tree.walk(), schema_tree.walk());

        node_validator.validate();
    }

    let errors = state
        .errors_so_far()
        .into_iter()
        .cloned()
        .collect::<Vec<ValidationError>>();
    let matches = state.matches_so_far().clone();

    (matches, errors)
}

/// Compare node kinds and return an error if they don't match
///
/// # Arguments
/// * `schema_node` - The schema node to compare against
/// * `input_node` - The input node to compare
/// * `schema_cursor` - The schema cursor, pointed at any node
/// * `input_cursor` - The input cursor, pointed at any node
///
/// # Returns
/// An optional validation error if the node kinds don't match
pub fn compare_node_kinds(
    schema_node: &Node,
    input_node: &Node,
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
) -> Option<ValidationError> {
    if schema_node.kind() != input_node.kind() {
        Some(ValidationError::SchemaViolation(
            SchemaViolationError::NodeTypeMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_node.kind().into(),
                actual: input_node.kind().into(),
            },
        ))
    } else {
        None
    }
}

/// Compare text contents and return an error if they don't match
///
/// # Arguments
/// * `schema_node` - The schema node to compare against
/// * `input_node` - The input node to compare
/// * `schema_str` - The full schema string
/// * `input_str` - The full input string
/// * `schema_cursor` - The schema cursor, pointed at any node that has text contents
/// * `input_cursor` - The input cursor, pointed at any node that has text contents
/// * `is_partial_match` - Whether the match is partial
///
/// # Returns
/// An optional validation error if the text contents don't match
pub fn compare_text_contents(
    schema_node: &Node,
    input_node: &Node,
    schema_str: &str,
    input_str: &str,
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    is_partial_match: bool,
) -> Option<ValidationError> {
    let (mut schema_text, input_text) = match (
        schema_node.utf8_text(schema_str.as_bytes()),
        input_node.utf8_text(input_str.as_bytes()),
    ) {
        (Ok(schema), Ok(input)) => (schema, input),
        (Err(_), _) | (_, Err(_)) => return None, // Can't compare invalid UTF-8
    };

    // If we're doing a partial match (not at EOF), adjust schema text length
    if is_partial_match {
        // If we got more input than expected, it's an error
        if input_text.len() > schema_text.len() {
            return Some(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_text.into(),
                    actual: input_text.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
        } else {
            // The schema might be longer than the input, so crop the schema to the input we've got
            schema_text = &schema_text[..input_text.len()];
        }
    }

    if schema_text != input_text {
        Some(ValidationError::SchemaViolation(
            SchemaViolationError::NodeContentMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_text.into(),
                actual: input_text.into(),
                kind: NodeContentMismatchKind::Literal,
            },
        ))
    } else {
        None
    }
}
