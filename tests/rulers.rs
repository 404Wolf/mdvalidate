use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validation::errors::{
    MalformedStructureKind, SchemaViolationError, ValidationError,
};

test_case!(ruler_dashes, r#"---"#, r#"---"#, json!({}), vec![]);

test_case!(
    ruler_missing_input,
    r#"---"#,
    r#""#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::MalformedNodeStructure {
            schema_index: 1,
            input_index: 0,
            kind: MalformedStructureKind::SchemaHasChildInputDoesnt,
        }
    )]
);

test_case!(ruler_stars, r#"***"#, r#"***"#, json!({}), vec![]);
