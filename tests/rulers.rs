use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    ruler_dashes,
    "ruler dashes",
    r#"---"#,
    r#"---"#,
    json!({}),
    vec![]
);

test_case!(
    ruler_stars,
    "ruler stars",
    r#"***"#,
    r#"***"#,
    json!({}),
    vec![]
);
