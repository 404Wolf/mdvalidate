use mdv_utils::PrettyPrint;
use tree_sitter::TreeCursor;

#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::mdschema::validator::{errors::ValidationError, validator_state::ValidatorState};

#[cfg(test)]
pub fn validate_str(schema: &str, input: &str) -> (Value, Vec<ValidationError>, ValidatorState) {
    use crate::mdschema::validator::ts_utils::new_markdown_parser;
    use crate::mdschema::validator::validator_state::ValidatorState;

    let mut state = ValidatorState::from_beginning(schema.into(), input.into(), true);

    let mut parser = new_markdown_parser();
    let schema_tree = parser.parse(schema, None).unwrap();
    let input_tree = parser.parse(input, None).unwrap();

    let new_state = {
        use crate::mdschema::validator::node_walker::NodeWalker;

        let mut node_validator = NodeWalker::new(&mut state, input_tree.walk(), schema_tree.walk());

        node_validator.validate();
        node_validator.state().clone()
    };

    let errors = new_state
        .errors_so_far()
        .into_iter()
        .cloned()
        .collect::<Vec<ValidationError>>();
    let matches = new_state.matches_so_far().to_owned();

    (matches, errors, new_state.clone())
}

pub fn pretty_print_cursor_pair(input_cursor: &TreeCursor, schema_cursor: &TreeCursor) -> String {
    use tabled::{Table, Tabled, settings::Style};

    #[derive(Tabled)]
    struct Content {
        #[tabled(rename = "Schema:")]
        schema: String,
        #[tabled(rename = "Input:")]
        input: String,
    }

    let content = Content {
        schema: schema_cursor.node().pretty_print(),
        input: input_cursor.node().pretty_print(),
    };

    Table::new(vec![content]).with(Style::blank()).to_string()
}
