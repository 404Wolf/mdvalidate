use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{SchemaError, ValidationError},
    matcher::{
        matcher::{Matcher, MatcherError},
        matcher_extras::get_all_extras,
    },
    ts_types::*,
    ts_utils::{get_next_node, get_node_text},
};

/// Check whether a paragraph is a repeated paragraph matcher.
///
/// A paragraph is a repeated paragraph matcher if it has a single child, which
/// is a a repeated matcher.
///
/// For example,
///
/// ```text
/// `test:/test/`{,}
/// ```
///
/// Contains a document with one child, which is a repeated paragraph matcher,
/// whereas
///
/// ```text
/// `test:/test/` test
/// ```
///
/// Contains a document with one child, which is just a normal paragraph with a
/// matcher in it.

/// Count the number of matchers, starting at some cursor pointing to a textual
/// container, and iterating through all of its children.
///
/// Returns the number of matchers, or a `ValidationError` that is probably a
/// `MatcherError` due to failing to construct a matcher given a code node that
/// is not marked as literal.
pub fn count_non_literal_matchers_in_children(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<usize, ValidationError> {
    let mut count = 0;
    let mut cursor = schema_cursor.clone();

    cursor.goto_first_child();

    loop {
        if !is_inline_code_node(&cursor.node()) {
            if !cursor.goto_next_sibling() {
                break;
            } else {
                continue;
            }
        }

        // If the following node is a text node, then it may have extras, so grab them.
        let extras_str = match get_next_node(&cursor)
            .filter(|n| is_text_node(n))
            .map(|next_node| {
                let next_node_str = get_node_text(&next_node, schema_str);
                get_all_extras(next_node_str)
            }) {
            Some(Ok(extras)) => Some(extras),
            Some(Err(error)) => {
                return Err(ValidationError::SchemaError(SchemaError::MatcherError {
                    error: error.into(),
                    schema_index: schema_cursor.descendant_index(),
                }));
            }
            None => None,
        };

        let pattern_str = get_node_text(&cursor.node(), schema_str);

        match Matcher::try_from_pattern_and_suffix_str(pattern_str, extras_str) {
            Ok(_) => count += 1,
            Err(MatcherError::WasLiteralCode) => {
                // Don't count it, but this is an OK error
            }
            Err(err) => {
                return Err(ValidationError::SchemaError(SchemaError::MatcherError {
                    error: err,
                    schema_index: cursor.descendant_index(),
                }));
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::{
        errors::{SchemaError, ValidationError},
        matcher::matcher::MatcherError,
        node_walker::helpers::count_non_literal_matchers_in_children::count_non_literal_matchers_in_children,
        ts_utils::parse_markdown,
    };

    #[test]
    fn test_count_non_literal_matchers_in_children_invalid_matcher() {
        let schema_str = "test `_*test*_`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        match count_non_literal_matchers_in_children(&schema_cursor, schema_str).unwrap_err() {
            ValidationError::SchemaError(SchemaError::MatcherError {
                error,
                schema_index,
            }) => {
                assert_eq!(schema_index, 3); // the index of the code_span
                match error {
                    MatcherError::MatcherInteriorRegexInvalid(_) => {}
                    _ => panic!("Expected MatcherInteriorRegexInvalid error"),
                }
            }
            _ => panic!("Expected InvalidMatcher error"),
        }
    }

    #[test]
    fn test_count_non_literal_matchers_in_children_only_literal_matcher() {
        let schema_str = "test `_*test*_`! `test:/test/`";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();

        assert_eq!(
            count_non_literal_matchers_in_children(&schema_cursor, schema_str).unwrap(),
            1 // one is literal
        );
    }

    #[test]
    fn test_count_non_literal_matchers_in_children_no_matchers() {
        let schema_str = "test *foo* _bar_";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();

        assert_eq!(
            count_non_literal_matchers_in_children(&schema_cursor, schema_str).unwrap(),
            0
        );
    }
}
