use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    test_literal_tables,
    r#"| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |

"#,
    r#"| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |
"#,
    json!({}),
    vec![]
);
