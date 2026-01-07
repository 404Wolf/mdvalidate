use serde_json::json;

#[macro_use]
mod helpers;

test_case!(
    code_literal,
    "code literal",
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
    "code language matcher",
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
    "code content capture",
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
