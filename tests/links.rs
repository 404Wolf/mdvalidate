use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaViolationError, ValidationError,
};

test_case!(
    link_literal,
    r#"[hi](https://example.com)"#,
    r#"[hi](https://example.com)"#,
    json!({}),
    vec![]
);

test_case!(
    link_destination_matcher_schema,
    r#"[hi]({foo:/\w+/})"#,
    r#"[hi](hello)"#,
    json!({"foo": "hello"}),
    vec![]
);

test_case!(
    image_literal,
    r#"![alt](image.png)"#,
    r#"![alt](image.png)"#,
    json!({}),
    vec![]
);

test_case!(
    link_destination_mismatch,
    r#"[hi](https://example.com)"#,
    r#"[hi](https://different.com)"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeContentMismatch {
            schema_index: 6,
            input_index: 6,
            expected: "https://example.com".into(),
            actual: "https://different.com".into(),
            kind: NodeContentMismatchKind::Literal,
        }
    )]
);

test_case!(
    link_inside_heading,
    r#"# [hi]({url:/.*/}) `other:/.*/`"#,
    r#"# [hi](https://example.com) hi"#,
    json!({"url": "https://example.com", "other": "hi"}),
    vec![]
);
