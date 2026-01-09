use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaViolationError, ValidationError,
};

test_case!(
    simple_blockquote,
    r#"> This is a quote
"#,
    r#"> This is a quote
"#,
    json!({}),
    vec![]
);

test_case!(
    blockquote_mismatch,
    r#"> This is a quote
"#,
    r#"> Different text
"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeContentMismatch {
            schema_index: 3,
            input_index: 3,
            expected: "This is a quote".to_string(),
            actual: "Different text".to_string(),
            kind: NodeContentMismatchKind::Literal,
        }
    )]
);

test_case!(
    nested_blockquote,
    r#"> Level 1
> > Level 2
"#,
    r#"> Level 1
> > Level 2
"#,
    json!({}),
    vec![]
);
