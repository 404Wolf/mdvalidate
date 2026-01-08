use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    matcher::matcher::{Matcher, MatcherError},
    ts_types::is_inline_code_node,
};

pub fn check_repeating_matchers(schema_cursor: &TreeCursor, schema_str: &str) -> Option<usize> {
    let mut schema_cursor = schema_cursor.clone();

    schema_cursor.goto_first_child();

    loop {
        if !is_inline_code_node(&schema_cursor.node()) {
            if !schema_cursor.goto_next_sibling() {
                break;
            } else {
                continue;
            }
        }

        match Matcher::try_from_schema_cursor(&schema_cursor, schema_str) {
            Ok(matcher) if matcher.is_repeated() => {
                return Some(schema_cursor.descendant_index());
            }
            Ok(_) => {}
            Err(MatcherError::WasLiteralCode) => {}
            _ => {}
        }

        if !schema_cursor.goto_next_sibling() {
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::{
        node_walker::helpers::check_repeating_matchers::check_repeating_matchers,
        ts_utils::parse_markdown,
    };

    fn get_check_repeating_matchers(schema_str: &str) -> Option<usize> {
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child();

        check_repeating_matchers(&schema_cursor, schema_str)
    }

    #[test]
    fn test_check_repeating_matchers_no_repeating() {
        assert_eq!(get_check_repeating_matchers("`test:/test/`"), None);
    }

    #[test]
    fn test_check_repeating_matchers_some_repeating() {
        // document -> paragraph -> codespan
        assert_eq!(get_check_repeating_matchers("`test:/test/`{,}").unwrap(), 2);
    }

    #[test]
    fn test_check_repeating_matchers_some_literal() {
        // document -> paragraph -> codespan -> text -> codespan
        assert_eq!(
            get_check_repeating_matchers("`test!`! `test:/test/`{1,}").unwrap(),
            5
        );
    }

    #[test]
    fn test_check_repeating_matchers_some_literal_some_invalid() {
        assert_eq!(get_check_repeating_matchers("`jeioafjioae`"), None);
    }
}
