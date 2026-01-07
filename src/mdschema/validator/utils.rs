#[cfg(test)]
use crate::mdschema::validator::ts_utils::new_markdown_parser;
use serde_json::Value;
#[cfg(test)]
use tree_sitter::Tree;

/// Join two values together in-place.
pub fn join_values(a: &mut Value, b: Value) {
    match (a, b) {
        (Value::Object(existing_map), Value::Object(new_map)) => {
            for (key, value) in new_map {
                existing_map.insert(key, value);
            }
        }
        (Value::Array(existing_array), Value::Array(new_array)) => {
            existing_array.extend(new_array);
        }
        _ => {}
    }
}

#[allow(dead_code)]
pub fn test_logging() {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .without_time()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::ENTER
                | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
        )
        .init();
}

#[cfg(test)]
pub fn parse_markdown_and_get_tree(input: &str) -> Tree {
    let mut parser = new_markdown_parser();
    parser.parse(input, None).unwrap()
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::ts_utils::extract_codeblock_contents;

    use super::*;

    #[test]
    fn test_extract_codeblock_contents() {
        // Without language, 3 backticks
        let input = "```\ncode\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input).unwrap(),
            Some((None, ("code".into(), 3)))
        );

        // With language, 3 backticks
        let input = "```rust\ncode\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input).unwrap(),
            Some((Some(("rust".into(), 3)), ("code".into(), 5)))
        );

        // Without language, 4 backticks
        let input = "````\ncode\n````\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input).unwrap(),
            Some((None, ("code".into(), 3)))
        );

        // With language, 4 backticks
        let input = "````rust\ncode\n````\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input).unwrap(),
            Some((Some(("rust".into(), 3)), ("code".into(), 5)))
        );
    }

    #[test]
    fn test_extract_codeblock_contents_multiline() {
        // Multiline code without language
        let input = "```\nline1\nline2\nline3\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input).unwrap(),
            Some((None, ("line1\nline2\nline3".into(), 3)))
        );

        // Multiline code with language
        let input = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input).unwrap(),
            Some((
                Some(("rust".into(), 3)),
                ("fn main() {\n    println!(\"Hello\");\n}".into(), 5)
            ))
        );

        // Multiline code with indentation
        let input = "```python\ndef hello():\n    print(\"world\")\n    return True\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input).unwrap(),
            Some((
                Some(("python".into(), 3)),
                (
                    "def hello():\n    print(\"world\")\n    return True".into(),
                    5
                )
            ))
        );
    }

    #[test]
    fn test_join_values_objects() {
        let mut a = serde_json::json!({ "key1": "value1" });
        let b = serde_json::json!({ "key2": "value2" });

        join_values(&mut a, b);

        if let Value::Object(ref map) = a {
            assert_eq!(map.get("key1"), Some(&Value::String("value1".to_string())));
            assert_eq!(map.get("key2"), Some(&Value::String("value2".to_string())));
        } else {
            panic!("a is not an object");
        }
    }

    #[test]
    fn test_join_values_arrays() {
        let mut a = Value::Array(vec![Value::String("value1".to_string())]);
        let b = Value::Array(vec![
            Value::String("value2".to_string()),
            Value::String("value3".to_string()),
        ]);

        join_values(&mut a, b);

        if let Value::Array(ref array) = a {
            assert_eq!(array.len(), 3);
            assert_eq!(array[0], Value::String("value1".to_string()));
            assert_eq!(array[1], Value::String("value2".to_string()));
            assert_eq!(array[2], Value::String("value3".to_string()));
        } else {
            panic!("a is not an array");
        }
    }
}
