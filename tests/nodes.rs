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
    node_list_and_code_block,
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
