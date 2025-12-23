use tracing::instrument;
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::{SchemaError, ValidationError},
    matcher::matcher::{Matcher, MatcherError},
    node_walker::{text_vs_text::validate_text_vs_text, ValidationResult}, ts_utils::has_single_code_child,
};

/// Validate a list node against a schema list node.
///
/// For each element in the schema list, if it is a literal, match it against
/// the corresponding input list element and move on.
///
/// ```md
/// - test1^
/// - test2
/// ```
///
/// ```md
/// - test1^
/// - test2
/// ```
///
/// If the cursor is at a matcher in the schema list, check what its range of
/// allowed number of matching input nodes is. Then try to match as many input
/// nodes as possible.
///
/// Then move on to the next element in the schema list, and repeat.
///
/// ```md
/// - test1
/// - test2^ can't match this one anymore!
/// - footest2
/// ```
///
/// ```md
/// - `id:/test\d/`{,2}^
/// - `id:/footest\d/`{,2}
/// ```
#[instrument(skip(input_cursor, schema_cursor, schema_str, input_str, got_eof), level = "debug", fields(
    input = %input_cursor.node().kind(),
    schema = %schema_cursor.node().kind()
), ret)]
pub fn validate_list_vs_list(
    input_cursor: &TreeCursor,
    schema_cursor: &TreeCursor,
    schema_str: &str,
    input_str: &str,
    got_eof: bool,
) -> ValidationResult {
    let mut result = ValidationResult::from_empty(
        input_cursor.descendant_index(),
        input_cursor.descendant_index(),
    );

    let mut input_cursor = input_cursor.clone();
    let mut schema_cursor = schema_cursor.clone();

    ensure_at_first_list_item(&mut input_cursor);
    ensure_at_first_list_item(&mut schema_cursor);
    debug_assert_eq!(input_cursor.node().kind(), "list_item");
    debug_assert_eq!(schema_cursor.node().kind(), "list_item");

    loop {
        match extract_repeated_matcher_from_list_item(&mut schema_cursor, schema_str) {
            // We were able to find a valid repeated matcher in the schema list item.
            Some(Ok(_matcher)) => {
                todo!()
            }
            // We were able to find a matcher in the schema list item, but it was invalid (we failed to parse it).
            Some(Err(e)) => {
                result.add_error(ValidationError::SchemaError(SchemaError::MatcherError {
                    error: e,
                    schema_index: schema_cursor.descendant_index(),
                    input_index: input_cursor.descendant_index(),
                }));
            }
            // We didn't find a repeating matcher. In this case, just use text_vs_text and move on.
            None => {
                let list_item_match_result = validate_text_vs_text(
                    &input_cursor,
                    &schema_cursor,
                    schema_str,
                    input_str,
                    got_eof,
                );
                result.join_other_result(&list_item_match_result);
            }
        }

        if !input_cursor.goto_next_sibling() || !schema_cursor.goto_next_sibling() {
            break;
        }
    }

    result
}

/// Extract a repeated matcher from a list item node.
///
/// Returns a matcher if the list item contains a repeated matcher pattern like:
///
/// ```md
/// - `name:/pattern/`{1,}
/// ```
///
/// Returns `None` if:
/// - The list item doesn't contain a matcher
/// - The matcher is not repeated
///
/// Otherwise we attempt to construct the matcher, maybe returning an error.
fn extract_repeated_matcher_from_list_item(
    schema_cursor: &mut TreeCursor,
    schema_str: &str,
) -> Option<Result<Matcher, MatcherError>> {
    debug_assert_eq!(schema_cursor.node().kind(), "list_item");

    // If the first node in the list item is not a paragraph that starts with a
    // code node, we can't have a matcher.
    let list_item_node = schema_cursor.node();
    let mut list_item_cursor = list_item_node.walk();
    list_item_cursor.goto_last_child(); // Should be a paragraph
    if list_item_cursor.node().kind() != "paragraph" {
        return None;
    }
    if !has_single_code_child(&list_item_cursor) {
        return None;
    }

    match Matcher::try_from_cursor(schema_cursor, schema_str) {
        Ok(matcher) if matcher.is_repeated() => Some(Ok(matcher)),
        Ok(_) => None,
        Err(e @ MatcherError::MatcherInteriorRegexInvalid(_)) => Some(Err(e)),
        Err(_) => None,
    }
}

fn ensure_at_first_list_item(input_cursor: &mut TreeCursor) {
    if input_cursor.node().kind() == "tight_list" {
        input_cursor.goto_first_child();
    }
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::{
        errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
        matcher::matcher::MatcherType,
        node_walker::list_vs_list::{
            ensure_at_first_list_item, extract_repeated_matcher_from_list_item,
            validate_list_vs_list,
        },
        ts_utils::parse_markdown,
    };

    #[test]
    fn test_ensure_at_first_list_item() {
        // Test with - (hyphen)
        let input_str = "- test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with + (plus)
        let input_str = "+ test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with * (asterisk)
        let input_str = "* test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");

        // Test with ordered list (1.)
        let input_str = "1. test\ntest2";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        assert_eq!(input_cursor.node().kind(), "tight_list");

        ensure_at_first_list_item(&mut input_cursor);
        assert_eq!(input_cursor.node().kind(), "list_item");
    }

    #[test]
    fn test_validate_list_vs_list_literal_list_items() {
        // Test with matching list items
        let schema_str = "- test1\n- test2";
        let input_str = "- test1\n- test2";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );

        // Test with different list items (should have errors)
        let schema_str = "- test1\n- test2";
        let input_str = "- test1\n- different";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let input_tree = parse_markdown(input_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let mut input_cursor = input_tree.walk();

        schema_cursor.goto_first_child();
        input_cursor.goto_first_child();

        let result =
            validate_list_vs_list(&input_cursor, &schema_cursor, schema_str, input_str, false);

        assert!(!result.errors.is_empty());
        match &result.errors[0] {
            ValidationError::SchemaViolation(SchemaViolationError::NodeContentMismatch {
                kind: NodeContentMismatchKind::Literal,
                ..
            }) => {}
            _ => panic!("Unexpected error type"),
        }
    }

    #[test]
    fn test_extract_repeated_matcher_from_list_item() {
        let input_str = "- `name:/pattern/`{1,1}";
        let input_tree = parse_markdown(input_str).unwrap();
        let mut input_cursor = input_tree.walk();

        input_cursor.goto_first_child();
        ensure_at_first_list_item(&mut input_cursor);

        let matcher = extract_repeated_matcher_from_list_item(&mut input_cursor, input_str)
            .unwrap()
            .unwrap();
        assert_eq!(matcher.id(), Some("name".into()));
        assert!(matches!(matcher.pattern(), MatcherType::Regex(_)));
    }
}
