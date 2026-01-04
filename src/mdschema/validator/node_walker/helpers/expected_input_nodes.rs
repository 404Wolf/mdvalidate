use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{SchemaError, ValidationError},
    matcher::{
        matcher::{Matcher, MatcherError},
        matcher_extras::{get_after_extras, get_all_extras},
    },
    ts_utils::{get_next_node, is_code_node, is_text_node},
};

/// Determine the number of nodes we expect in some corresponding input string.
///
/// # Algorithm
///
/// ```ignore
//// we at matcher?
/// ├── no
/// │   └── next is matcher?
/// │       ├── no -> 0
/// │       └── yes
/// │           └── at text?
/// │               ├── no -> 0
/// │               └── yes -> next is coalescing
/// │                           ├── no -> 1
/// │                           └── yes -> 0
/// └── yes
///     ├── is coalescing?
///     │   └── yes
///     │       └── end is at end?
///     │           ├── yes -> 1
///     │           └── no -> non text follows?
///     │                       ├── yes -> 1
///     │                       └── no -> 1
///     └── no
///         └── has extra text?
///             ├── yes -> 1
///             └── no -> 0
///```
pub fn expected_input_nodes(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<usize, ValidationError> {
    let mut schema_cursor = schema_cursor.clone();

    let mut node_chunk_count = 0;
    let mut correction_count = 0;

    loop {
        node_chunk_count += 1;

        let at_text_node = is_text_node(&schema_cursor.node());
        let next_is_non_text = next_is_non_text(&schema_cursor);

        correction_count += match at_coalescing_matcher(&schema_cursor, schema_str)? {
            Some(at_coalescing) => {
                let has_extra_text = has_extra_text(&schema_cursor, schema_str);

                if at_coalescing {
                    if !has_extra_text {
                        1
                    } else if next_is_non_text {
                        1
                    } else {
                        0
                    }
                } else if has_extra_text {
                    1
                } else {
                    0
                }
            }
            None => match next_at_coalescing_matcher(&schema_cursor, schema_str)? {
                Some(next_is_coalescing) if at_text_node && !next_is_coalescing => 1,
                Some(_) => 0,
                None => 0,
            },
        };

        if !schema_cursor.goto_next_sibling() {
            break;
        }
    }

    Ok(node_chunk_count - correction_count)
}

/// Whether the next node is non text. If there is no next node, then this returns false.
fn next_is_non_text(schema_cursor: &TreeCursor) -> bool {
    match get_next_node(schema_cursor) {
        Some(next_node) => !is_text_node(&next_node),
        None => false,
    }
}

/// Whether a node has "extra text" after it. This takes into account matchers.
///
/// # Algorithm
///
/// ```ignore
/// | is literal?
/// | - no
/// |   | - text after matcher?
/// |       | - no -> T
/// |       | - yes -> F
/// | - yes
///     | - matcher follows?
///         | - no
///         |   | - text after matcher?
///         |       | - no -> T
///         |       | - yes -> F
///         | - yes
///             | - following is literal?
///                 | - no -> T
///                 | - yes -> F
/// ```
fn has_extra_text(schema_cursor: &TreeCursor, schema_str: &str) -> bool {
    debug_assert!(is_code_node(&schema_cursor.node()));

    let mut lookahead_cursor = schema_cursor.clone();
    match at_coalescing_matcher(schema_cursor, schema_str).unwrap_or(Some(false)) {
        Some(is_literal) => {
            let had_next_matcher = move_cursor_to_next_matcher(&mut lookahead_cursor, schema_str);

            let text_after_matcher = text_after_matcher(schema_cursor, schema_str) != "";

            if text_after_matcher {
                true
            } else if is_literal {
                match at_coalescing_matcher(&lookahead_cursor, schema_str).unwrap_or(Some(false)) {
                    Some(next_matcher_is_literal) if had_next_matcher => !next_matcher_is_literal,
                    Some(_) => text_after_matcher,
                    None => text_after_matcher,
                }
            } else {
                false
            }
        }
        None => false, // not even at matcher to begin with. TODO: should we error here?
    }
}

/// Get the text that comes after a matcher.
///
/// The cursor must be pointing at a code node, which is the matcher, and this
/// gets all the text that comes after the next node's matcher extras.
fn text_after_matcher<'a>(schema_cursor: &TreeCursor, schema_str: &'a str) -> &'a str {
    debug_assert!(is_code_node(&schema_cursor.node()));

    match get_next_node(&schema_cursor) {
        Some(next_node) => {
            if !is_text_node(&next_node) {
                return "";
            }

            let next_node_str = next_node.utf8_text(schema_str.as_bytes()).unwrap();

            get_after_extras(next_node_str).unwrap_or("")
        }
        None => "",
    }
}

/// Get the extras for the node after a matcher.
///
/// The cursor must be pointing at a code node, which is the matcher, and this
/// gets all the extras for it.
fn extras_after_matcher<'a>(schema_cursor: &TreeCursor, schema_str: &'a str) -> &'a str {
    debug_assert!(is_code_node(&schema_cursor.node()));

    match get_next_node(&schema_cursor) {
        Some(next_node) => {
            let next_node_str = next_node.utf8_text(schema_str.as_bytes()).unwrap();

            get_all_extras(next_node_str).unwrap_or("")
        }
        None => "",
    }
}

/// Whether we are currently at a matcher, and whether that matcher is coalescing.
///
/// # Returns
///
/// - `Some(false)` if we are at a matcher that is not coalescing.
/// - `Some(true)` if we are at a matcher that is coalescing.
/// - `None` if we are not at a matcher.
fn at_coalescing_matcher(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<Option<bool>, ValidationError> {
    if !is_code_node(&schema_cursor.node()) {
        return Ok(None);
    }

    match Matcher::try_from_schema_cursor(schema_cursor, schema_str) {
        Ok(matcher) if matcher.is_repeated() => Ok(Some(true)),
        Ok(_) => Ok(Some(false)),
        Err(MatcherError::WasLiteralCode) => Ok(Some(true)),
        Err(error) => Err(ValidationError::SchemaError(SchemaError::MatcherError {
            error,
            schema_index: schema_cursor.descendant_index(),
        })),
    }
}

/// Whether the next node exists, and if so, if it is a matcher, and if so, is a
/// coalescing matcher.
///
/// # Returns
///
/// - `Some(false)` if the next node is a matcher that is not coalescing.
/// - `Some(true)` if the next node is a matcher that is coalescing.
/// - `None` if there is no next node or if the next node is not a matcher.
fn next_at_coalescing_matcher(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Result<Option<bool>, ValidationError> {
    let mut lookahead_cursor = schema_cursor.clone();
    if lookahead_cursor.goto_next_sibling() {
        at_coalescing_matcher(&lookahead_cursor, schema_str)
    } else {
        Ok(None)
    }
}

/// Assuming the cursor is at a matcher, move it forward to the next text node,
/// then move it forward to the next code span.
fn move_cursor_to_next_matcher(schema_cursor: &mut TreeCursor, schema_str: &str) -> bool {
    let extras_after_matcher = extras_after_matcher(schema_cursor, schema_str) != "";

    // If there was extras after the matcher, that means we should skip to the
    // next next node
    if extras_after_matcher {
        schema_cursor.goto_next_sibling() && schema_cursor.goto_next_sibling()
    } else {
        // Otherwise just go to the next node
        schema_cursor.goto_next_sibling()
    }
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::{
        node_walker::helpers::expected_input_nodes::{
            at_coalescing_matcher, expected_input_nodes, extras_after_matcher, has_extra_text,
            text_after_matcher,
        },
        ts_utils::parse_markdown,
    };

    fn get_expected_input_nodes(schema_str: &str) -> usize {
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        expected_input_nodes(&schema_cursor, schema_str).unwrap()
    }

    fn get_has_extra_text(schema_str: &str) -> bool {
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        has_extra_text(&schema_cursor, schema_str)
    }

    fn get_text_after_matcher<'a>(schema_str: &'a str) -> &'a str {
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        text_after_matcher(&schema_cursor, schema_str)
    }

    fn get_extras_after_matcher<'a>(schema_str: &'a str) -> &'a str {
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        extras_after_matcher(&schema_cursor, schema_str)
    }

    fn get_at_literal_matcher(schema_str: &str) -> Option<bool> {
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();
        schema_cursor.goto_first_child();
        at_coalescing_matcher(&schema_cursor, schema_str).unwrap()
    }

    #[test]
    fn test_get_extras_after_matcher() {
        assert_eq!(get_extras_after_matcher("`test`!"), "!");
        assert_eq!(get_extras_after_matcher("`test`!*test*"), "!");
        assert_eq!(get_extras_after_matcher("`test`{,} test"), "{,}");
        assert_eq!(get_extras_after_matcher("`test`!test `test`! test"), "!");
    }

    #[test]
    fn test_get_text_after_matcher() {
        assert_eq!(get_text_after_matcher("`test`!"), "");
        assert_eq!(get_text_after_matcher("`test`*test*"), "");
        assert_eq!(get_text_after_matcher("`test`!*test*"), "");
        assert_eq!(get_text_after_matcher("`test`! test"), " test");
        assert_eq!(get_text_after_matcher("`test`! test`test:/test/`"), " test");
        assert_eq!(get_text_after_matcher("`test`!test `test`! test"), "test ");
    }

    #[test]
    fn test_has_extra_text_for_literal() {
        assert!(!get_has_extra_text("`test`!"));
        assert!(!get_has_extra_text("`test`!*test*"));
        assert!(get_has_extra_text("`test`! test"));
        assert!(get_has_extra_text("`test`!test `test`!"));
    }

    #[test]
    fn test_has_extra_text_for_mixed() {
        assert!(get_has_extra_text("`test`!`test`"));
        assert!(!get_has_extra_text("`test``test`!"));
    }

    #[test]
    fn test_has_extra_text_for_regular() {
        assert!(!get_has_extra_text("`test`"));
        assert!(!get_has_extra_text("`test`*test*"));
        assert!(get_has_extra_text("`test` test"));
        assert!(get_has_extra_text("`test` i*test*"));
    }

    #[test]
    fn test_expected_input_nodes_only_text() {
        let schema_str = "test";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    fn test_expected_input_nodes_only_matcher() {
        let schema_str = "`foo:/bar/`";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    fn test_expected_input_nodes_matcher_then_matcher() {
        let schema_str = "`foo:/bar/``foo:/bar/`";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    fn test_expected_input_nodes_only_literal_matcher() {
        let schema_str = "`test`!";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    fn test_expected_input_nodes_two_literal_matchers() {
        let schema_str = "`test`!`test`!`test`!";
        assert_eq!(get_expected_input_nodes(schema_str), 3);
    }

    #[test]
    fn test_expected_input_nodes_literal_then_regular() {
        let schema_str = "`test`!`test:/bar/`";
        assert_eq!(get_expected_input_nodes(schema_str), 2);
    }

    #[test]
    fn test_expected_input_nodes_regular_then_literal() {
        let schema_str = "`test:/bar/` `test`!";
        assert_eq!(get_expected_input_nodes(schema_str), 2);
    }

    #[test]
    fn test_at_literal_matcher() {
        assert!(get_at_literal_matcher("`test:/test/`!").unwrap());
        assert!(get_at_literal_matcher("`test:/test/`! test").unwrap());
        assert!(!get_at_literal_matcher("`test:/test/`").unwrap());
        assert!(!get_at_literal_matcher("`test:/test/` test").unwrap());
        assert!(!get_at_literal_matcher("`test:/test/``test:/test/`").unwrap());
        assert!(get_at_literal_matcher("`test`!`test:/test/`").unwrap());
    }

    #[test]
    fn test_expected_input_nodes_two_literal_matchers_and_regular() {
        let schema_str = "`test`!`test`!`test`!`test:/bar/`";
        assert_eq!(get_expected_input_nodes(schema_str), 4);
    }

    #[test]
    fn test_expected_input_nodes_only_literal_matcher_with_suffix() {
        let schema_str = "`test`! test";
        assert_eq!(get_expected_input_nodes(schema_str), 2);
    }

    #[test]
    fn test_expected_input_nodes_no_matcher() {
        let schema_str = "test *test*";
        assert_eq!(get_expected_input_nodes(schema_str), 2);
    }

    #[test]
    fn test_expected_input_nodes_non_text_after_literal() {
        let schema_str = "`_*test*_`!*bar*";
        assert_eq!(get_expected_input_nodes(schema_str), 2);
    }

    #[test]
    fn test_expected_input_nodes_non_text_after_literal_text_before() {
        let schema_str = "test `_*test*_`!*bar*";
        assert_eq!(get_expected_input_nodes(schema_str), 3);
    }

    #[test]
    fn test_expected_input_nodes_literal_at_end() {
        let schema_str = "test `_*test*_`!";
        assert_eq!(get_expected_input_nodes(schema_str), 2);
    }

    #[test]
    fn test_expected_input_nodes_literal_matcher_at_end() {
        let schema_str = "test `_*test*_`!";
        assert_eq!(get_expected_input_nodes(schema_str), 2);
    }

    #[test]
    fn test_expected_input_nodes_normal_matcher_at_end() {
        let schema_str = "test `foo:/bar/`";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    fn test_expected_input_nodes_normal_matcher_at_start() {
        let schema_str = "`foo:/bar/` test";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    fn test_expected_input_nodes_normal_matcher_at_start_and_end() {
        let schema_str = "`foo:/bar/` test `foo:/bar/` ";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    fn test_expected_input_nodes_normal_and_literal_matcher() {
        let schema_str = "`foo:/bar/` test `foo:/bar/`!";
        assert_eq!(get_expected_input_nodes(schema_str), 2);
    }

    #[test]
    fn test_expected_input_nodes_repeated_matcher() {
        let schema_str = r"`test2:/\w+/`{1,1}";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    fn test_expected_input_nodes_repeated_matcher_many_digit() {
        let schema_str = r"`test2:/\w+/`{111,111}";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }

    #[test]
    #[ignore]
    // TODO: this doesn't pass but does it need to?
    fn test_expected_input_nodes_two_repeated_matcher() {
        let schema_str = "`foo:/bar/`{,}`foo:/bar/`{,}";
        assert_eq!(get_expected_input_nodes(schema_str), 1);
    }
}
