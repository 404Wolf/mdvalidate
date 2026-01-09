use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{
    ChildrenCount, SchemaViolationError, ValidationError,
};

test_case!(
    node_heading_and_paragraph,
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
    node_children_mismatch,
    r#""#,
    r#"# Hi"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::ChildrenLengthMismatch {
            schema_index: 0,
            input_index: 0,
            expected: ChildrenCount::SpecificCount(0),
            actual: 1,
        }
    )]
);

test_case!(
    complicated_multiple_doc_children_example,
    r#"
# Hi `name:/[A-Z][a-z]*/`

| Header 1 | Header `num:/\d/` |
|----------|----------|
| Cell 1   | Cell 2   |

- `items:/.*/`{,}

```{lang:/\w+/}
{code}
```

"#,
    r#"
# Hi Wolf

| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |

- apples
- bananas

```python
print("hi")
```
"#,
    json!({"lang": "python", "code": "print(\"hi\")", "name": "Wolf", "num": "2", "items": ["apples", "bananas"]}),
    vec![]
);

test_case!(
    complicated_multiple_doc_children_example_fails,
    r#"
# Hi `name:/[A-Z][a-z]*/`

- item
"#,
    r#"
- item

# Hi Wolf
"#,
    json!({}),
    vec![
        ValidationError::SchemaViolation(SchemaViolationError::NodeTypeMismatch {
            schema_index: 1,
            input_index: 1,
            expected: "atx_heading".to_string(),
            actual: "tight_list".to_string(),
        }),
        ValidationError::SchemaViolation(SchemaViolationError::NodeTypeMismatch {
            schema_index: 7,
            input_index: 6,
            expected: "tight_list".to_string(),
            actual: "atx_heading".to_string(),
        })
    ]
);
