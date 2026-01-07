use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{SchemaViolationError, ValidationError};

test_case!(
    unordered_list_literal,
    r#"
- a
- b
"#,
    r#"
- a
- b
"#,
    json!({}),
    vec![]
);

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
