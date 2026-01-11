use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validation::errors::{
    NodeContentMismatchKind, SchemaViolationError, ValidationError,
};

test_case!(
    textual_literal,
    r#"hello world"#,
    r#"hello world"#,
    json!({}),
    vec![]
);

test_case!(
    textual_matcher,
    r#"hi `name:/\w+/`"#,
    r#"hi Bob"#,
    json!({"name": "Bob"}),
    vec![]
);

test_case!(
    textual_mismatch,
    r#"hello"#,
    r#"hi"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeContentMismatch {
            schema_index: 2,
            input_index: 2,
            expected: "hello".into(),
            actual: "hi".into(),
            kind: NodeContentMismatchKind::Literal,
        }
    )]
);
