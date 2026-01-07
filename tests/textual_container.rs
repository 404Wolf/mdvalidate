use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    paragraph_literal,
    "paragraph literal",
    r#"hello **world**"#,
    r#"hello **world**"#,
    json!({}),
    vec![]
);

test_case!(
    paragraph_matcher,
    "paragraph matcher",
    r#"hello `name:/\w+/`"#,
    r#"hello Alice"#,
    json!({"name": "Alice"}),
    vec![]
);

test_case!(
    paragraph_mixed_literal,
    "paragraph mixed literal",
    r#"hello *there*"#,
    r#"hello *there*"#,
    json!({}),
    vec![]
);
