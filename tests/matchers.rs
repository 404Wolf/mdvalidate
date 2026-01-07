use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    text_matcher_only,
    "text matcher only",
    r#"`name:/\w+/`"#,
    r#"Alice"#,
    json!({"name": "Alice"}),
    vec![]
);

test_case!(
    matcher_with_prefix,
    "matcher with prefix",
    r#"hi `name:/\w+/`"#,
    r#"hi Bob"#,
    json!({"name": "Bob"}),
    vec![]
);

test_case!(
    literal_matcher,
    "literal matcher",
    r#"`test`!"#,
    r#"`test`"#,
    json!({}),
    vec![]
);
