use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validation::errors::{SchemaViolationError, ValidationError};

test_case!(heading_literal, r#"# Hi"#, r#"# Hi"#, json!({}), vec![]);

test_case!(
    heading_matcher,
    r#"# `name:/\w+/`"#,
    r#"# Alice"#,
    json!({"name": "Alice"}),
    vec![]
);

test_case!(
    heading_mismatch,
    r#"# Hi"#,
    r#"## Hi"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeTypeMismatch {
            schema_index: 1,
            input_index: 1,
            expected: "atx_heading(atx_h1_marker)".into(),
            actual: "atx_heading(atx_h2_marker)".into(),
        }
    )]
);
