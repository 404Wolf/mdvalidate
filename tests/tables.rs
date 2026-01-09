use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    test_literal_tables,
    r#"
# Hi `name:/[A-Z][a-z]*/`

| Header 1 | Header `num:/\d/` |
|:---------|:---------|
| Cell 1   | Cell 2   |

"#,
    r#"
# Hi Wolf

| Header 1 | Header 2 |
|:---------|----------|
| Cell 1   | Cell 2   |
"#,
    json!({"num": "2", "name": "Wolf"}),
    vec![]
);
