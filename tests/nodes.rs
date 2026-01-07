use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    node_heading_and_paragraph,
    "node heading and paragraph",
    r#"
# Title

Hello
"#,
    r#"
# Title

Hello
"#,
    json!({}),
    vec![]
);

test_case!(
    node_list_and_code_block,
    "node list and code block",
    r#"
- item

```txt
hi
```
"#,
    r#"
- item

```txt
hi
```
"#,
    json!({}),
    vec![]
);
