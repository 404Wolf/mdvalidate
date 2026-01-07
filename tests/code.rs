use serde_json::json;

#[macro_use]
mod helpers;

use mdvalidate::mdschema::validator::errors::{
    NodeContentMismatchKind, SchemaViolationError, ValidationError,
};

test_case!(
    code_literal,
    r#"
```rust
fn main() {}
```
"#,
    r#"
```rust
fn main() {}
```
"#,
    json!({}),
    vec![]
);

test_case!(
    code_language_matcher,
    r#"
```{lang:/\w+/}
fn main() {}
```
"#,
    r#"
```rust
fn main() {}
```
"#,
    json!({"lang": "rust"}),
    vec![]
);

test_case!(
    code_content_capture,
    r#"
```rust
{code}
```
"#,
    r#"
```rust
fn main() {}
```
"#,
    json!({"code": "fn main() {}"}),
    vec![]
);

test_case!(
    code_content_mismatch,
    r#"
```rust
fn main() {}
```
"#,
    r#"
```rust
fn main() { }
```
"#,
    json!({}),
    vec![ValidationError::SchemaViolation(
        SchemaViolationError::NodeContentMismatch {
            schema_index: 5,
            input_index: 5,
            expected: "fn main() {}".into(),
            actual: "fn main() { }".into(),
            kind: NodeContentMismatchKind::Literal,
        }
    )]
);
