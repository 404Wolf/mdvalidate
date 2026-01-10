use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaViolationError, ValidationError,
};

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

test_case!(
    test_literal_repeated_literal_sandwich,
    r#"
# Shopping List

| Item | Price |
|:-----|:------|
| Header | 10 |
| `item:/\w+/` | `price:/\d+/` |{,3}
| Footer | 99 |
"#,
    r#"
# Shopping List

| Item | Price |
|:-----|:------|
| Header | 10 |
| Apple | 5 |
| Banana | 3 |
| Cherry | 7 |
| Footer | 99 |
"#,
    json!({"item": ["Apple", "Banana", "Cherry"], "price": ["5", "3", "7"]}),
    vec![]
);

test_case!(
    test_literal_repeated_literal_sandwich_with_footer,
    r#"
# Shopping List

| Item | Price |
|:-----|:------|
| Header | 10 |
| `item:/\w+/` | `price:/\d+/` |{,3}
| Footer | 99 |
"#,
    r#"
# Shopping List

| Item | Price |
|:-----|:------|
| Header | 10 |
| Apple | 5 |
| Banana | 3 |
| Cherry | 7 |
| Footer | 99 |
"#,
    json!({"item": ["Apple", "Banana", "Cherry"], "price": ["5", "3", "7"]}),
    vec![]
);

test_case!(
    test_literal_repeated_literal_sandwich_with_mismatch,
    r#"
# Shopping List

| Item | Price |
|:-----|:------|
| Header | 10 |
| `item:/\w+/` | `price:/\d+/` |{,2}
| Footer | 99 |
"#,
    r#"
# Shopping List

| Item | Price |
|:-----|:------|
| Header | 10 |
| Apple | 5 |
| Banana | not_a_number |
| Cherry | 7 |
| Footer | 99 |
"#,
    json!({}),
    // Should error on the second repeated row where price doesn't match the \d+ pattern
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeContentMismatch {
            schema_index: 11,
            input_index: 18,
            expected: "^\\d+".to_string(),
            actual: "not_a_number".to_string(),
            kind: NodeContentMismatchKind::Matcher,
        }
    )]
);
