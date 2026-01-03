#[cfg(test)]
use crate::mdschema::validator::ts_utils::new_markdown_parser;
use serde_json::Value;
#[cfg(test)]
use tree_sitter::Tree;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
    ts_utils::{
        extract_list_marker, get_heading_kind, is_ordered_list_marker, is_unordered_list_marker,
    },
};

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

/// Compare node kinds and return an error if they don't match
///
/// # Arguments
/// * `schema_cursor` - The schema cursor, pointed at any node
/// * `input_cursor` - The input cursor, pointed at any node
/// * `input_str` - The input string
/// * `schema_str` - The schema string
///
/// # Returns
/// An optional validation error if the node kinds don't match
pub fn compare_node_kinds(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    input_str: &str,
    schema_str: &str,
) -> Option<ValidationError> {
    let schema_node = schema_cursor.node();
    let input_node = input_cursor.node();

    let schema_kind = schema_node.kind();
    let input_kind = input_node.kind();

    // If they are both tight lists, check the first children of each of them,
    // which are list markers. This will indicate whether they are the same type
    // of list.
    if schema_cursor.node().kind() == "tight_list" && input_cursor.node().kind() == "tight_list" {
        let schema_list_marker = extract_list_marker(schema_cursor, schema_str);
        let input_list_marker = extract_list_marker(input_cursor, input_str);

        // They must both be unordered, both be ordered, or both have the same marker
        if schema_list_marker == input_list_marker {
            // They can be the same list symbol!
        } else if is_ordered_list_marker(schema_list_marker)
            && is_ordered_list_marker(input_list_marker)
        {
            // Or both ordered
        } else if is_unordered_list_marker(schema_list_marker)
            && is_unordered_list_marker(input_list_marker)
        {
            // Or both unordered
        } else {
            // But anything else is a mismatch

            return Some(ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    // TODO: find a better way to represent the *kind* of list in this error
                    expected: format!("{}({})", input_cursor.node().kind(), schema_list_marker),
                    actual: format!("{}({})", input_cursor.node().kind(), input_list_marker),
                },
            ));
        }
    }

    if schema_cursor.node().kind() == "atx_heading" && input_cursor.node().kind() == "atx_heading" {
        let schema_heading_kind = get_heading_kind(&schema_cursor);
        let input_heading_kind = get_heading_kind(&input_cursor);

        if schema_heading_kind != input_heading_kind {
            return Some(ValidationError::SchemaViolation(
                SchemaViolationError::NodeTypeMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: format!("{}({})", input_cursor.node().kind(), schema_heading_kind),
                    actual: format!("{}({})", input_cursor.node().kind(), input_heading_kind),
                },
            ));
        }
    }

    if schema_kind != input_kind {
        Some(ValidationError::SchemaViolation(
            SchemaViolationError::NodeTypeMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_node.kind().into(),
                actual: input_node.kind().into(),
            },
        ))
    } else {
        None
    }
}

/// Compare node children lengths and return an error if they don't match
///
/// # Arguments
/// * `schema_cursor` - The schema cursor, pointed at any node
/// * `input_cursor` - The input cursor, pointed at any node
/// * `got_eof` - Whether we have reached the end of file
///
/// # Returns
/// An optional validation error if the children lengths don't match
pub fn compare_node_children_lengths(
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    got_eof: bool,
) -> Option<ValidationError> {
    use crate::mdschema::validator::errors::{ChildrenCount, SchemaViolationError};

    // First, count the children to check for length mismatches
    let input_child_count = input_cursor.node().child_count();
    let schema_child_count = schema_cursor.node().child_count();

    // Handle node mismatches
    // If we have reached the EOF:
    //   No difference in the number of children
    // else:
    //   We can have less input children
    //
    let children_len_mismatch_err =
        ValidationError::SchemaViolation(SchemaViolationError::ChildrenLengthMismatch {
            schema_index: schema_cursor.descendant_index(),
            input_index: input_cursor.descendant_index(),
            expected: ChildrenCount::from_specific(schema_child_count),
            actual: input_child_count,
        });

    if got_eof {
        // At EOF, children count must match exactly
        if input_child_count != schema_child_count {
            return Some(children_len_mismatch_err);
        }
    } else {
        // Not at EOF: input can have fewer children, but not more
        if input_child_count > schema_child_count {
            return Some(children_len_mismatch_err);
        }
    }

    None
}

/// Compare text contents and return an error if they don't match
///
/// # Arguments
/// * `schema_cursor` - The schema cursor, pointed at any node that has text contents
/// * `input_cursor` - The input cursor, pointed at any node that has text contents
/// * `schema_str` - The full schema string
/// * `input_str` - The full input string
/// * `is_partial_match` - Whether the match is partial
/// * `strip_extras` - Whether to strip matcher extras from the start of the
///   input string. For example, if the input string is "{1,2}! test", when
///   comparing, strip away until after the first space, only comparing "test".
///
/// # Returns
/// An optional validation error if the text contents don't match
pub fn compare_text_contents(
    schema_str: &str,
    input_str: &str,
    schema_cursor: &TreeCursor,
    input_cursor: &TreeCursor,
    is_partial_match: bool,
    strip_extras: bool,
) -> Option<ValidationError> {
    let (schema_text, input_text) = match (
        schema_cursor.node().utf8_text(schema_str.as_bytes()),
        input_cursor.node().utf8_text(input_str.as_bytes()),
    ) {
        (Ok(schema), Ok(input)) => (schema, input),
        (Err(_), _) | (_, Err(_)) => return None, // Can't compare invalid UTF-8
    };
    let schema_text = if strip_extras {
        // TODO: this assumes that ! is the only extra when it is an extra
        let stripped = schema_text
            .split_once(" ")
            .map(|(_extras, rest)| format!(" {}", rest))
            .unwrap_or(schema_text.to_string());

        if stripped.len() == 1 {
            " ".into()
        } else {
            stripped
        }
    } else {
        schema_text.to_string()
    };
    let mut schema_text = schema_text.as_str();

    // If we're doing a partial match (not at EOF), adjust schema text length
    if is_partial_match {
        // If we got more input than expected, it's an error
        if input_text.len() > schema_text.len() {
            return Some(ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                    expected: schema_text.into(),
                    actual: input_text.into(),
                    kind: NodeContentMismatchKind::Literal,
                },
            ));
        } else {
            // The schema might be longer than the input, so crop the schema to the input we've got
            schema_text = &schema_text[..input_text.len()];
        }
    }

    if schema_text != input_text {
        Some(ValidationError::SchemaViolation(
            SchemaViolationError::NodeContentMismatch {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                expected: schema_text.into(),
                actual: input_text.into(),
                kind: NodeContentMismatchKind::Literal,
            },
        ))
    } else {
        None
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
    use crate::mdschema::validator::ts_utils::{extract_codeblock_contents, parse_markdown};

    use super::*;

    #[test]
    fn test_extract_codeblock_contents() {
        // Without language, 3 backticks
        let input = "```\ncode\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input),
            Some((None, ("code".into(), 3)))
        );

        // With language, 3 backticks
        let input = "```rust\ncode\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input),
            Some((Some(("rust".into(), 3)), ("code".into(), 5)))
        );

        // Without language, 4 backticks
        let input = "````\ncode\n````\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input),
            Some((None, ("code".into(), 3)))
        );

        // With language, 4 backticks
        let input = "````rust\ncode\n````\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input),
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
            extract_codeblock_contents(&cursor, input),
            Some((None, ("line1\nline2\nline3".into(), 3)))
        );

        // Multiline code with language
        let input = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\n";
        let tree = parse_markdown_and_get_tree(input);
        let mut cursor = tree.walk();
        cursor.goto_first_child();
        assert_eq!(
            extract_codeblock_contents(&cursor, input),
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
            extract_codeblock_contents(&cursor, input),
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
    fn test_compare_node_kinds_list() {
        let input_1 = " - test1";
        let input_1_tree = parse_markdown(input_1).unwrap();
        let mut input_1_cursor = input_1_tree.walk();

        let input_2 = " * test1";
        let input_2_tree = parse_markdown(input_2).unwrap();
        let mut input_2_cursor = input_2_tree.walk();

        input_1_cursor.goto_first_child();
        input_2_cursor.goto_first_child();
        assert_eq!(input_2_cursor.node().kind(), "tight_list");
        assert_eq!(input_1_cursor.node().kind(), "tight_list");

        let result = compare_node_kinds(&input_2_cursor, &input_1_cursor, input_1, input_2);
        assert!(result.is_none());
    }

    #[test]
    fn test_compare_node_kinds_headings() {
        let input_1 = "# test1";
        let input_1_tree = parse_markdown(input_1).unwrap();
        let mut input_1_cursor = input_1_tree.walk();

        let input_2 = "# test2";
        let input_2_tree = parse_markdown(input_2).unwrap();
        let mut input_2_cursor = input_2_tree.walk();

        input_1_cursor.goto_first_child();
        input_2_cursor.goto_first_child();
        assert_eq!(input_2_cursor.node().kind(), "atx_heading");
        assert_eq!(input_1_cursor.node().kind(), "atx_heading");

        let result = compare_node_kinds(&input_2_cursor, &input_1_cursor, input_1, input_2);
        assert!(result.is_none());

        let input_2 = "## test2";
        let input_2_tree = parse_markdown(input_2).unwrap();
        let mut input_2_cursor = input_2_tree.walk();

        input_2_cursor.goto_first_child();
        assert_eq!(input_2_cursor.node().kind(), "atx_heading");
        assert_eq!(input_1_cursor.node().kind(), "atx_heading");

        let result = compare_node_kinds(&input_2_cursor, &input_1_cursor, input_1, input_2);
        assert!(result.is_some(), "Should detect heading level mismatch");

        if let Some(ValidationError::SchemaViolation(SchemaViolationError::NodeTypeMismatch {
            expected,
            actual,
            schema_index,
            input_index,
        })) = result
        {
            assert_eq!(expected, "atx_heading(atx_h2_marker)");
            assert_eq!(actual, "atx_heading(atx_h1_marker)");
            assert_eq!(input_index, 1);
            assert_eq!(schema_index, 1);
        } else {
            panic!("Expected NodeTypeMismatch error for different heading levels");
        }
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
