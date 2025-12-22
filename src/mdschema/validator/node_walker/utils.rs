#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::mdschema::validator::errors::ValidationError;

#[cfg(test)]
pub fn validate_str(schema: &str, input: &str) -> (Value, Vec<ValidationError>) {
    use crate::mdschema::validator::ts_utils::new_markdown_parser;
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

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::errors::{
        NodeContentMismatchKind, SchemaViolationError, ValidationError,
    };
    use crate::mdschema::validator::node_walker::list_vs_list::validate_list_vs_list;
    use crate::mdschema::validator::ts_utils::parse_markdown;

    #[test]
    fn test_list_vs_list_one_item_same_contents() {
        let schema_str = "# List\n- Item 1\n- Item 2\n";
        let input_str = "# List\n- Item 1\n- Item 2\n";

        let schema_tree = parse_markdown(schema_str);
        let input_tree = parse_markdown(input_str);

        let schema_tree = schema_tree.as_ref().unwrap();
        let input_tree = input_tree.as_ref().unwrap();

        let schema_cursor = schema_tree.root_node().walk();
        let input_cursor = input_tree.root_node().walk();

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_list_vs_list_one_item_different_contents() {
        let schema_str = "# List\n- Item 1\n- Item 2\n";
        let input_str = "# List\n- Item 1\n- Item 3\n";

        let schema_tree = parse_markdown(schema_str);
        let input_tree = parse_markdown(input_str);

        let schema_tree = schema_tree.as_ref().unwrap();
        let input_tree = input_tree.as_ref().unwrap();

        let schema_cursor = schema_tree.root_node().walk();
        let input_cursor = input_tree.root_node().walk();

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, true);

        assert_eq!(result.errors.len(), 1);
        assert_eq!(
            result.errors[0],
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                schema_index: 2,
                input_index: 2,
                expected: "Item 2".into(),
                actual: "Item 3".into(),
                kind: NodeContentMismatchKind::Literal,
            })
        );
    }
}