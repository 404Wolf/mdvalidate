#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::mdschema::validator::{errors::ValidationError, validator_state::ValidatorState};

#[cfg(test)]
pub fn validate_str(schema: &str, input: &str) -> (Value, Vec<ValidationError>, ValidatorState) {
    use crate::mdschema::validator::ts_utils::new_markdown_parser;
    use crate::mdschema::validator::validator_state::ValidatorState;

    let mut state = ValidatorState::from_beginning(schema.to_string(), input.to_string(), true);

    let mut parser = new_markdown_parser();
    let schema_tree = parser.parse(schema, None).unwrap();
    let input_tree = parser.parse(input, None).unwrap();

    let new_state = {
        use crate::mdschema::validator::node_walker::NodeWalker;

        let mut node_validator = NodeWalker::new(&mut state, input_tree.walk(), schema_tree.walk());

        node_validator.validate();
        node_validator.state
    };

    let errors = new_state
        .errors_so_far()
        .into_iter()
        .cloned()
        .collect::<Vec<ValidationError>>();
    let matches = new_state.matches_so_far().to_owned();

    (matches, errors, new_state.clone())
}
