use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    link_literal,
    "link literal",
    r#"[hi](https://example.com)"#,
    r#"[hi](https://example.com)"#,
    json!({}),
    vec![]
);

test_case!(
    link_destination_matcher_schema,
    "link destination matcher schema",
    r#"[hi]({foo:/\w+/})"#,
    r#"[hi](hello)"#,
    json!({"foo": "hello"}),
    vec![]
);

test_case!(
    image_literal,
    "image literal",
    r#"![alt](image.png)"#,
    r#"![alt](image.png)"#,
    json!({}),
    vec![]
);
