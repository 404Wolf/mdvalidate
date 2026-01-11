use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validation::errors::{
    NodeContentMismatchKind, SchemaViolationError, ValidationError,
};

test_case!(
    paragraph_literal,
    r#"hello **world**"#,
    r#"hello **world**"#,
    json!({}),
    vec![]
);

test_case!(
    paragraph_content_mismatch,
    r#"hello **world**"#,
    r#"hello **there**"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeContentMismatch {
            schema_index: 3,
            input_index: 3,
            expected: "**world**".into(),
            actual: "**there**".into(),
            kind: NodeContentMismatchKind::Literal,
        }
    )]
);

test_case!(
    paragraph_matcher,
    r#"hello `name:/\w+/`"#,
    r#"hello Alice"#,
    json!({"name": "Alice"}),
    vec![]
);

test_case!(
    paragraph_mixed_literal,
    r#"# hello *there*"#,
    r#"# hello *there*"#,
    json!({}),
    vec![]
);

test_case!(
    heading_link_and_text_matchers,
    r#"# [hi]({url:/.*/}) `other:/\w+/`"#,
    r#"# [hi](https://example.com) hi"#,
    json!({"url": "https://example.com", "other": "hi"}),
    vec![]
);
