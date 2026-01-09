use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{SchemaViolationError, ValidationError};

test_case!(ruler_dashes, r#"---"#, r#"---"#, json!({}), vec![]);

test_case!(
    ruler_missing_input,
    r#"---"#,
    r#""#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::ChildrenLengthMismatch {
            schema_index: 0,
            input_index: 0,
            expected: 1.into(),
            actual: 0,
        }
    )]
);

test_case!(ruler_stars, r#"***"#, r#"***"#, json!({}), vec![]);
