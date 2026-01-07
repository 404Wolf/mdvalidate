use mdv_utils::PrettyPrint;
use tree_sitter::TreeCursor;

#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::mdschema::validator::{errors::ValidationError, validator::Validator};

#[cfg(test)]
pub fn validate_str(schema: &str, input: &str) -> (Value, Vec<ValidationError>, Validator) {
    use crate::mdschema::validator::validator::ValidatorState;

    let mut validator = Validator::new_complete(schema, input).unwrap();
    validator.validate();

    let errors = validator
        .errors_so_far()
        .cloned()
        .collect::<Vec<ValidationError>>();
    let matches = validator.matches_so_far().to_owned();

    (matches, errors, validator)
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
