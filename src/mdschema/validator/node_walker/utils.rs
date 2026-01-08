use mdv_utils::PrettyPrint;
use tree_sitter::TreeCursor;

#[cfg(test)]
use serde_json::Value;

use crate::mdschema::validator::ts_utils::walk_to_root;
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

pub fn pretty_print_cursor_pair(schema_cursor: &TreeCursor, input_cursor: &TreeCursor) -> String {
    use tabled::{Table, Tabled, settings::Style};

    let schema_cursor_descendant_index = schema_cursor.descendant_index();
    let input_cursor_descendant_index = input_cursor.descendant_index();

    let mut schema_cursor = schema_cursor.clone();
    let mut input_cursor = input_cursor.clone();

    walk_to_root(&mut schema_cursor);
    walk_to_root(&mut input_cursor);

    #[derive(Tabled)]
    struct Content {
        #[tabled(rename = "Schema:")]
        schema: String,
        #[tabled(rename = "Input:")]
        input: String,
    }

    let schema_str = schema_cursor
        .node()
        .pretty_print_with_highlight(&[schema_cursor_descendant_index]);
    let input_str = input_cursor
        .node()
        .pretty_print_with_highlight(&[input_cursor_descendant_index]);

    let content = Content {
        schema: schema_str,
        input: input_str,
    };

    Table::new(vec![content]).with(Style::blank()).to_string()
}
