#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::mdschema::validator::{
    errors::Error, node_walker::NodeWalker, utils::new_markdown_parser,
};

#[cfg(test)]
pub fn validate_str(schema: &str, input: &str) -> (Value, Vec<Error>) {
    use crate::mdschema::validator::validator_state::ValidatorState;

    let mut state = ValidatorState::new(schema.to_string(), input.to_string(), true);

    let mut parser = new_markdown_parser();
    let schema_tree = parser.parse(schema, None).unwrap();
    let input_tree = parser.parse(input, None).unwrap();

    {
        let mut node_validator = NodeWalker::new(&mut state, input_tree.walk(), schema_tree.walk());

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
