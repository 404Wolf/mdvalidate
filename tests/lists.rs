use serde_json::json;

#[macro_use]
mod helpers;

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
