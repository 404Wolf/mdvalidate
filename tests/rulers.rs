use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    ruler_dashes,
    r#"---"#,
    r#"---"#,
    json!({}),
    vec![]
);

test_case!(
    ruler_stars,
    r#"***"#,
    r#"***"#,
    json!({}),
    vec![]
);
