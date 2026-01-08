use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{SchemaViolationError, ValidationError};

test_case!(
    ordered_list_literal,
    r#"
1. a
2. b
"#,
    r#"
1. a
2. b
"#,
    json!({}),
    vec![]
);

test_case!(
    list_matcher,
    r#"
- `item:/\w+/`
"#,
    r#"
- apple
"#,
    json!({"item": "apple"}),
    vec![]
);

test_case!(
    list_matcher_no_limit,
    r#"
- a
- b
    - `items:/.*/`{1,1}
        - `items2:/.*/`{1,}
"#,
    r#"
- a
- b
    - b
        - c
"#,
    json!({"items": ["b", {"items2": ["c"]}]}),
    vec![]
);

test_case!(
    list_kind_mismatch,
    r#"
- a
"#,
    r#"
1. a
"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeTypeMismatch {
            schema_index: 1,
            input_index: 1,
            expected: "tight_list(-)".into(),
            actual: "tight_list(1.)".into(),
        }
    )]
);
