use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaViolationError, ValidationError,
};

test_case!(
    text_matcher_only,
    r#"`name:/\w+/`"#,
    r#"Alice"#,
    json!({"name": "Alice"}),
    vec![]
);

test_case!(
    matcher_with_prefix,
    r#"hi `name:/\w+/`"#,
    r#"hi Bob"#,
    json!({"name": "Bob"}),
    vec![]
);

test_case!(
    literal_matcher,
    r#"`test`!"#,
    r#"`test`"#,
    json!({}),
    vec![]
);

test_case!(
    matcher_mismatch,
    r#"`name:/[a-z]+/`"#,
    r#"123"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeContentMismatch {
            schema_index: 2,
            input_index: 2,
            expected: "^[a-z]+".into(),
            actual: "123".into(),
            kind: NodeContentMismatchKind::Matcher,
        }
    )]
);
