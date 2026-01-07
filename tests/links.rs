use serde_json::json;

#[macro_use]
mod helpers;

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
