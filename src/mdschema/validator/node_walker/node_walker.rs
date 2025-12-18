use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    node_walker::{node_vs_node::validate_node_vs_node, validation_result::ValidationResult},
    validator_state::ValidatorState,
};

/// A node validator that validates input nodes against schema nodes.
pub struct NodeWalker<'a> {
    pub state: &'a mut ValidatorState,
    input_cursor: TreeCursor<'a>,
    schema_cursor: TreeCursor<'a>,
}

impl<'a> NodeWalker<'a> {
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

    pub fn validate(&mut self) -> ValidationResult {
        let validation_result = validate_node_vs_node(
            &mut self.input_cursor,
            &mut self.schema_cursor,
            self.state.schema_str(),
            self.state.last_input_str(),
            self.state.got_eof(),
        );

        self.state.push_validation_result(validation_result.clone());

        validation_result
    }
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::node_walker::utils::validate_str;
    use serde_json::json;

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
    fn test_simple_paragraph() {
        let schema = "Hello `name:/\\w+/`\n";
        let input = "Hello Wolf\n";

        let (matches, errors) = validate_str(schema, input);

        assert!(errors.is_empty(), "Errors found: {:?}", errors);
        assert_eq!(matches, json!({"name": "Wolf"}));
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
