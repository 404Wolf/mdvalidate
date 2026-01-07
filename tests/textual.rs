use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    textual_literal,
    "textual literal",
    r#"hello world"#,
    r#"hello world"#,
    json!({}),
    vec![]
);

test_case!(
    textual_matcher,
    "textual matcher",
    r#"hi `name:/\w+/`"#,
    r#"hi Bob"#,
    json!({"name": "Bob"}),
    vec![]
);
