use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    heading_literal,
    r#"# Hi"#,
    r#"# Hi"#,
    json!({}),
    vec![]
);

test_case!(
    heading_matcher,
    r#"# `name:/\w+/`"#,
    r#"# Alice"#,
    json!({"name": "Alice"}),
    vec![]
);
