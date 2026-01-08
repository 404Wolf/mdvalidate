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

pub fn pretty_print_cursor_pair(schema_cursor: &TreeCursor, input_cursor: &TreeCursor) -> String {
    pretty_print_cursor_pair_with_highlight(schema_cursor, input_cursor, None)
}

pub fn pretty_print_cursor_pair_with_highlight(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    highlight_index: Option<usize>,
) -> String {
    use tabled::{Table, Tabled, settings::Style};

    #[derive(Tabled)]
    struct Content {
        #[tabled(rename = "Schema:")]
        schema: String,
        #[tabled(rename = "Input:")]
        input: String,
    }

    let mut schema_str = schema_cursor.node().pretty_print();
    let mut input_str = input_cursor.node().pretty_print();

    if let Some(idx) = highlight_index {
        // Mark the position with a dot
        let marker = format!(" <-- Index {}", idx);
        schema_str.push_str(&marker);
        input_str.push_str(&marker);
    }

    let content = Content {
        schema: schema_str,
        input: input_str,
    };

    Table::new(vec![content]).with(Style::blank()).to_string()
}
