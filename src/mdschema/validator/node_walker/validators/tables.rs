//! Table validator for node-walker comparisons.
//!
//! Types:
//! - `TableVsTableValidator`: validates table structure (rows, headers, cells)
//!   and delegates cell content checks to textual container validation.
//! - `RepeatedRowVsRowValidator`: processes schema rows followed by matcher
//!   repeaters, keeping the schema stationary while validating multiple input
//!   rows against a repeating matcher row.
use crate::mdschema::validator::errors::{
    MalformedStructureKind, NodeContentMismatchKind, SchemaViolationError, ValidationError,
};
use crate::mdschema::validator::matcher::matcher::Matcher;
use crate::mdschema::validator::matcher::matcher_extras::MatcherExtras;
use crate::mdschema::validator::node_pos_pair::NodePosPair;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::validators::containers::ContainerVsContainerValidator;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
use crate::mdschema::validator::ts_types::*;
use crate::mdschema::validator::ts_utils::{get_node_text, waiting_at_end};
use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::invariant_violation;
use log::trace;
use tree_sitter::TreeCursor;

/// Validate two tables.
#[derive(Default)]
pub(super) struct TableVsTableValidator;

impl ValidatorImpl for TableVsTableValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        let mut result = ValidationResult::from_cursors(&schema_cursor, &input_cursor);
        let need_to_restart_result = result.clone();

        // Both should be at tables already
        #[cfg(feature = "invariant_violations")]
        if !both_are_tables(&schema_cursor.node(), &input_cursor.node()) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "we should already be at table nodes"
            )
        }

        if !schema_cursor.goto_first_child() || !input_cursor.goto_first_child() {
            #[cfg(feature = "invariant_violations")]
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "we should be able to dive down one layer into a table"
            )
        }

        #[cfg(feature = "invariant_violations")]
        if !both_are_table_headers(&schema_cursor.node(), &input_cursor.node()) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "the immediate child of all tables should be table header"
            )
        }

        result.sync_cursor_pos(&schema_cursor, &input_cursor);

        //  (document[0]0..41)
        //  └─ (table[1]1..40)
        //     ├─ (table_header_row[2]1..22) <-- we are iterating over these in the outer loop
        //     │  ├─ (table_cell[3]2..16) <-- we are iterating over these in the inner loop
        //     │  │  └─ (text[4]2..16)
        //     │  └─ (table_cell[5]17..21)
        //     │     └─ (text[6]17..21)
        //     ├─ (table_delimiter_row[7]23..28)
        //     │  ├─ (table_column_alignment[8]24..25)
        //     │  └─ (table_column_alignment[9]26..27)
        //     └─ (table_data_row[10]29..40)
        //        ├─ (table_cell[11]30..34)
        //        │  └─ (text[12]30..34)
        //        └─ (table_cell[13]35..39)
        //           └─ (text[14]35..39)

        // General idea: For each row, walk down to the first child, iterate over all its siblings,
        // hop back to the row container, go to the next row, until there are no rows left.

        'row_iter: loop {
            // First check if we are dealing with a special case -- repeated rows!
            if both_are_table_data_rows(&schema_cursor.node(), &input_cursor.node())
                && let Some(bounds) =
                    try_get_repeated_row_bounds(&schema_cursor, walker.schema_str())
            {
                // Process the repeated rows using the main cursors
                let repeated_row_result = RepeatedRowVsRowValidator::from_bounds(bounds)
                    .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
                result.join_other_result(&repeated_row_result);

                // If there were errors in the repeated row validation, return immediately
                if repeated_row_result.has_errors() {
                    return result;
                }

                // Update the cursors to where the repeated row validator left them
                // The schema cursor stays at the repeating row, input cursor advanced past all matched rows
                repeated_row_result.walk_cursors_to_pos(&mut schema_cursor, &mut input_cursor);

                // Now continue to advance both cursors to the next row
            } else {
                // Dive in to the first row, iterate over children, hop back (hop
                // back is automatic since we use different cursors in the context)
                {
                    let mut schema_cursor = schema_cursor.clone();
                    let mut input_cursor = input_cursor.clone();

                    match (
                        schema_cursor.goto_first_child(),
                        input_cursor.goto_first_child(),
                    ) {
                        (true, true) => {
                            #[cfg(feature = "invariant_violations")]
                            if !both_are_table_cells(&schema_cursor.node(), &input_cursor.node()) {
                                invariant_violation!(
                                    result,
                                    &schema_cursor,
                                    &input_cursor,
                                    "the immediate child of table headers should be a table cell"
                                )
                            }
                        }
                        (false, false) => break 'row_iter,
                        _ => invariant_violation!(
                            result,
                            &schema_cursor,
                            &input_cursor,
                            "table is malformed in a way that should be impossible"
                        ),
                    }

                    // we are at the first cell initially. we validate the first
                    // cell in the input vs the first cell in the schema, and then
                    // jump to the next sibling pair. If there is no next sibling
                    // pair we are done.
                    'col_iter: loop {
                        let cell_result = ContainerVsContainerValidator::default()
                            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
                        result.join_other_result(&cell_result);

                        match (
                            schema_cursor.goto_next_sibling(),
                            input_cursor.goto_next_sibling(),
                        ) {
                            (true, true) => {}
                            (false, false) => break 'col_iter,
                            (false, true) => {
                                if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                                    // okay, we'll just wait!
                                } else {
                                    result.add_error(ValidationError::SchemaViolation(
                                        SchemaViolationError::MalformedNodeStructure {
                                            schema_index: schema_cursor.descendant_index(),
                                            input_index: input_cursor.descendant_index(),
                                            kind: MalformedStructureKind::InputHasChildSchemaDoesnt,
                                        },
                                    ));
                                }
                            }
                            (true, false) => {
                                if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                                    // okay, we'll just wait!
                                    return need_to_restart_result;
                                } else {
                                    result.add_error(ValidationError::SchemaViolation(
                                        SchemaViolationError::MalformedNodeStructure {
                                            schema_index: schema_cursor.descendant_index(),
                                            input_index: input_cursor.descendant_index(),
                                            kind: MalformedStructureKind::SchemaHasChildInputDoesnt,
                                        },
                                    ));
                                }
                                return result;
                            }
                        }
                    }
                }
            }

            'wait_for_row: loop {
                match (
                    schema_cursor.goto_next_sibling(),
                    input_cursor.goto_next_sibling(),
                ) {
                    (true, true) => {
                        result.keep_farther_pos(&NodePosPair::from_cursors(
                            &schema_cursor,
                            &input_cursor,
                        ));

                        if !both_are_table_delimiter_rows(
                            &schema_cursor.node(),
                            &input_cursor.node(),
                        ) {
                            break 'wait_for_row;
                        }
                    }
                    (false, false) => break 'row_iter,
                    (false, true) => {
                        if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                            // okay, we'll just wait!
                            return need_to_restart_result;
                        } else {
                            result.add_error(ValidationError::SchemaViolation(
                                SchemaViolationError::MalformedNodeStructure {
                                    schema_index: schema_cursor.descendant_index(),
                                    input_index: input_cursor.descendant_index(),
                                    kind: MalformedStructureKind::InputHasChildSchemaDoesnt,
                                },
                            ));
                        }
                    }
                    (true, false) => {
                        if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                            // okay, we'll just wait!
                            return need_to_restart_result;
                        } else {
                            result.add_error(ValidationError::SchemaViolation(
                                SchemaViolationError::MalformedNodeStructure {
                                    schema_index: schema_cursor.descendant_index(),
                                    input_index: input_cursor.descendant_index(),
                                    kind: MalformedStructureKind::SchemaHasChildInputDoesnt,
                                },
                            ));
                        }
                        return result;
                    }
                }
            }
        }

        result
    }
}

pub(super) struct RepeatedRowVsRowValidator {
    bounds: (Option<usize>, Option<usize>),
}

impl RepeatedRowVsRowValidator {
    pub fn from_bounds(bounds: (Option<usize>, Option<usize>)) -> Self {
        Self { bounds }
    }
}

impl ValidatorImpl for RepeatedRowVsRowValidator {
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut schema_cursor = walker.schema_cursor().clone();
        let mut input_cursor = walker.input_cursor().clone();

        let mut result = ValidationResult::from_cursors(&schema_cursor, &input_cursor);
        let _need_to_restart_result = result.clone();

        #[cfg(feature = "invariant_violations")]
        if !both_are_table_data_rows(&schema_cursor.node(), &input_cursor.node()) {
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "is_repeated_row only works for data row nodes. Title row nodes cannot be repeated. Got {:?}",
                schema_cursor.node().kind()
            )
        }

        let max_bound = self.bounds.1.unwrap_or(usize::MAX);

        let corresponding_matchers = {
            let mut schema_cursor = schema_cursor.clone();
            let had_first_child = schema_cursor.goto_first_child();

            #[cfg(feature = "invariant_violations")]
            if !had_first_child {
                invariant_violation!("should have had first child")
            }

            get_cell_indexes_that_have_simple_matcher(&schema_cursor, walker.schema_str())
        };

        let corresponding_matchers_only_matchers: Vec<&Matcher> = corresponding_matchers
            .iter()
            .filter_map(|n| n.as_ref())
            .collect();
        let num_corresponding_matchers = corresponding_matchers_only_matchers.len();

        let mut all_matches: Vec<Vec<String>> = vec![Vec::new(); num_corresponding_matchers];

        'row_iter: for _ in 0..max_bound {
            // Validate the entire row
            let mut input_cursor_at_first_cell = get_cursor_at_first_cell(&input_cursor);
            let mut schema_cursor_at_first_cell = get_cursor_at_first_cell(&schema_cursor);

            let mut matcher_num = 0;
            'col_iter: for i in 0.. {
                let cell_str =
                    get_node_text(&input_cursor_at_first_cell.node(), walker.input_str()).trim();

                match corresponding_matchers.get(i).unwrap() {
                    Some(matcher) => match matcher.match_str(cell_str) {
                        Some(captured_str) => {
                            all_matches
                                .get_mut(matcher_num)
                                .unwrap() // we pre filled it properly ahead of time
                                .push(captured_str.to_string());

                            matcher_num += 1;
                        }
                        None => {
                            result.add_error(ValidationError::SchemaViolation(
                                SchemaViolationError::NodeContentMismatch {
                                    schema_index: schema_cursor_at_first_cell.descendant_index(),
                                    input_index: input_cursor_at_first_cell.descendant_index(),
                                    expected: matcher.pattern().to_string(),
                                    actual: cell_str.into(),
                                    kind: NodeContentMismatchKind::Matcher,
                                },
                            ));

                            return result;
                        }
                    },
                    None => {
                        // Validate the cell as a normal container.
                        let cell_result = ContainerVsContainerValidator::default().validate(
                            &walker.with_cursors(
                                &schema_cursor_at_first_cell,
                                &input_cursor_at_first_cell,
                            ),
                            got_eof,
                        );
                        result.join_data(cell_result.data());
                        if cell_result.has_errors() {
                            result.join_errors(cell_result.errors());
                            return result;
                        }
                    }
                }

                if input_cursor_at_first_cell.goto_next_sibling() {
                    if !schema_cursor_at_first_cell.goto_next_sibling() {
                        break 'col_iter;
                    }
                } else {
                    break 'col_iter;
                }
            }

            // Move the input to the next row (the schema stays put!)
            if !input_cursor.goto_next_sibling() {
                break 'row_iter;
            }
        }

        for (matches, matcher) in all_matches.iter().zip(corresponding_matchers_only_matchers) {
            if let Some(key) = matcher.id() {
                result.set_match(key, matches.clone().into());
            }
        }

        // Update the result to reflect where we ended up:
        // - schema_cursor stays at the repeating row definition
        // - input_cursor has advanced past all matched rows
        schema_cursor.goto_next_sibling();
        result.sync_cursor_pos(&schema_cursor, &input_cursor);

        result
    }
}

/// For each cell, check if it is a single simple matcher. If it is, load a
/// Some(Matcher) with that matcher into a Vec, and if it is not, load a None
/// instead.
///
/// # Arguments
///
/// * `schema_cursor` - A cursor pointing to the first cell in the repeating schema row.
/// * `schema_str` - The string representation of the schema.
///
/// # Returns
///
/// A vector of `Some<Matcher>`s of the cells that have a single matcher.
fn get_cell_indexes_that_have_simple_matcher(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Vec<Option<Matcher>> {
    #[cfg(feature = "invariant_violations")]
    if !is_table_cell_node(&schema_cursor.node()) {
        invariant_violation!("we should start at the first cell in the repeating row in the table",)
    }

    let mut schema_cursor = schema_cursor.clone();

    let mut indexes = Vec::new();

    loop {
        let mut code_child_idx = None;
        let mut is_simple = true;

        for idx in 0..schema_cursor.node().child_count() {
            let child = schema_cursor.node().child(idx).unwrap();

            if is_inline_code_node(&child) {
                if code_child_idx.is_some() {
                    is_simple = false;
                    break;
                }
                code_child_idx = Some(idx);
            } else if child.kind() == "text" {
                let text = get_node_text(&child, schema_str);
                if !text.chars().all(|c| c.is_whitespace()) {
                    is_simple = false;
                    break;
                }
            } else {
                is_simple = false;
                break;
            }
        }

        if is_simple {
            if let Some(code_idx) = code_child_idx {
                let mut matcher_cursor = schema_cursor.clone();
                if matcher_cursor.goto_first_child() {
                    for _ in 0..code_idx {
                        matcher_cursor.goto_next_sibling();
                    }
                    if let Ok(matcher) =
                        Matcher::try_from_schema_cursor(&matcher_cursor, schema_str)
                    {
                        indexes.push(Some(matcher));
                    } else {
                        indexes.push(None);
                    }
                } else {
                    indexes.push(None);
                }
            } else {
                indexes.push(None);
            }
        } else {
            indexes.push(None);
        }

        if schema_cursor.goto_next_sibling() {
            // continue!
        } else {
            break;
        }
    }

    indexes
}

/// Walk down to the first node, and debug assert that it is a table cell.
fn get_cursor_at_first_cell<'a>(cursor: &TreeCursor<'a>) -> TreeCursor<'a> {
    let mut cursor = cursor.clone();
    cursor.goto_first_child();

    #[cfg(feature = "invariant_violations")]
    if !is_table_cell_node(&cursor.node()) {
        invariant_violation!("the descendant of the cursor here should be a table cell",)
    }

    cursor
}

/// We say that a row is repeated if there is a repeater directly after the row.
///
/// Example:
/// ```markdown
/// |c1|c2|
/// |-|-|
/// |r1|r2|{1,2} (a row like this row can appear 1-2 times)
/// ```
fn try_get_repeated_row_bounds(
    schema_cursor: &TreeCursor,
    schema_str: &str,
) -> Option<(Option<usize>, Option<usize>)> {
    #[cfg(feature = "invariant_violations")]
    if !is_table_data_row_node(&schema_cursor.node()) {
        invariant_violation!(
            "is_repeated_row only works for data row nodes. Title row nodes cannot be repeated. Got {:?}",
            schema_cursor.node().kind()
        )
    }

    // If we have a table like:
    //
    // |c1|c2|
    // |-|-|
    // |r1{1,2}|{2,}|
    //
    // We don't want to lock onto the {2,}
    let full_row_str = get_node_text(&schema_cursor.node(), schema_str);
    // We are guaranteed there will be a cell at the very end that could be a
    // correct repeater if the cell does not end with "|" or ":"
    if full_row_str.ends_with(|c| c == '|' || c == ':') {
        return None;
    }

    let mut schema_cursor = schema_cursor.clone();

    if !schema_cursor.goto_first_child() {
        // If there are no children then we can't be a repeated row.

        return None;
    }

    #[cfg(feature = "invariant_violations")]
    if !is_table_cell_node(&schema_cursor.node()) {
        invariant_violation!("at this point we should be at a table cell")
    }

    // Go to the last sibling
    while schema_cursor.goto_next_sibling() {}

    if schema_cursor.goto_first_child() && is_text_node(&schema_cursor.node()) {
        let node_str = get_node_text(&schema_cursor.node(), schema_str);

        match MatcherExtras::try_from_extras_str(node_str) {
            Ok(extras) if extras.had_min_max() => Some((extras.min_items(), extras.max_items())),
            Ok(extras) => {
                trace!("Got non-repeating extras: {:?}", extras);

                None
            }
            Err(error) => {
                trace!("Error parsing matcher extras: {:?}", error);

                None
            }
        }
    } else {
        trace!("Unexpected node kind: {:?}", schema_cursor.node().kind());

        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_utils::ValidatorTester;
    use super::*;
    use crate::mdschema::validator::{
        errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
        node_pos_pair::NodePosPair,
        ts_utils::parse_markdown,
    };
    use serde_json::json;

    #[test]
    fn get_cell_indexes_that_have_simple_matcher_simple() {
        // just has one matcher
        let schema_str = r#"
|c1|c2|c3|c4|c5|
|-|-|-|-|-|
|r1|`foo:/test/`|`bar:/test2/`|not a matcher|`baz:/test3/`|
        "#;

        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> table
        schema_cursor.goto_first_child(); // table -> header row
        schema_cursor.goto_next_sibling(); // header row -> delimiter row
        schema_cursor.goto_next_sibling(); // delimiter row -> data row
        assert!(is_table_data_row_node(&schema_cursor.node()));
        schema_cursor.goto_first_child(); // data row -> table cell
        assert!(is_table_cell_node(&schema_cursor.node()));

        assert_eq!(
            get_cell_indexes_that_have_simple_matcher(&schema_cursor, schema_str),
            vec![
                None,
                Some(Matcher::try_from_pattern_and_suffix_str("`foo:/test/`", None).unwrap()),
                Some(Matcher::try_from_pattern_and_suffix_str("`bar:/test2/`", None).unwrap()),
                None,
                Some(Matcher::try_from_pattern_and_suffix_str("`baz:/test3/`", None).unwrap())
            ]
        )
    }

    #[test]
    fn test_is_repeated_row_is_repeated() {
        let schema_str = r#"
|c1|c2|
|-|-|
|r1|r2|{1,2}
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> table
        schema_cursor.goto_first_child(); // table -> header row
        schema_cursor.goto_next_sibling(); // header row -> delimiter row
        schema_cursor.goto_next_sibling(); // delimiter row -> data row
        assert!(is_table_data_row_node(&schema_cursor.node()));

        assert_eq!(
            try_get_repeated_row_bounds(&schema_cursor, schema_str).unwrap(),
            (Some(1), Some(2))
        )
    }

    #[test]
    fn test_is_repeated_row_is_repeated_broken() {
        let schema_str = r#"
|c1|c2|
|-|-|
|r1|r2|{1,2
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> table
        schema_cursor.goto_first_child(); // table -> header row
        schema_cursor.goto_next_sibling(); // header row -> delimiter row
        schema_cursor.goto_next_sibling(); // delimiter row -> data row
        assert!(is_table_data_row_node(&schema_cursor.node()));

        assert_eq!(
            try_get_repeated_row_bounds(&schema_cursor, schema_str),
            None
        )
    }

    #[test]
    fn test_is_repeated_row_is_not_repeated_bounds_in_wrong_place() {
        let schema_str = r#"
|c1|c2|
|-|-|
|r1{21,5}|{2,}|
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> table
        schema_cursor.goto_first_child(); // table -> header row
        schema_cursor.goto_next_sibling(); // header row -> delimiter row
        schema_cursor.goto_next_sibling(); // delimiter row -> data row
        assert!(is_table_data_row_node(&schema_cursor.node()));

        assert_eq!(
            try_get_repeated_row_bounds(&schema_cursor, schema_str),
            None
        )
    }

    #[test]
    fn test_is_repeated_row_is_not_repeated() {
        let schema_str = r#"
|c1|c2|
|-|-|
|r1|r2|
"#;
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();
        schema_cursor.goto_first_child(); // document -> table
        schema_cursor.goto_first_child(); // table -> header row
        schema_cursor.goto_next_sibling(); // header row -> delimiter row
        schema_cursor.goto_next_sibling(); // delimiter row -> data row
        assert!(is_table_data_row_node(&schema_cursor.node()));

        assert_eq!(
            try_get_repeated_row_bounds(&schema_cursor, schema_str),
            None
        )
    }

    #[test]
    fn test_validate_table_vs_table_simple_literal() {
        let schema_str = r#"
|c1|c2|
|-|-|
|r1|r2|
            "#;
        let input_str = r#"
|c1|c2|
|-|-|
|r1|r2|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(result.value(), &json!({}));
        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(14, 14)
        ); // end of very end
    }

    #[test]
    fn test_validate_table_vs_table_simple_literal_incomplete_missing_last_cell() {
        let schema_str = r#"
|c1|`foo:/test/`|
|-|-|
|r1|r2|
            "#;
        let input_str = r#"
|c1|test|
|-|-|
|r1
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_incomplete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(result.value(), &json!({}));
        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(1, 1) // we'll have to revalidate the entire table
        );

        // but if the table is already valid, even if we are incomplete we
        // should be able to walk our way through it
        let input_str = r#"
|c1|test|
|-|-|
|r1|r2
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_incomplete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(result.value(), &json!({"foo": "test"}));
        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(15, 14)
        );
    }

    #[test]
    fn test_validate_table_vs_table_simple_literal_incomplete_missing_row() {
        let schema_str = r#"
|c1|`foo:/test/`|
|-|-|
|r1|r2|
            "#;
        let input_str = r#"
|c1|test|
|-|-|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_incomplete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(result.value(), &json!({}));
        assert_eq!(
            *result.farthest_reached_pos(),
            NodePosPair::from_pos(1, 1) // we'll have to revalidate the entire table
        );
    }

    #[test]
    fn test_validate_table_vs_table_literal_mismatch() {
        let schema_str = r#"
|c1|c3|
|-|-|
|r1|r2|
            "#;
        let input_str = r#"
|c1|c2|
|-|-|
|r1|r2|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(
            result.errors(),
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: 6,
                    input_index: 6,
                    expected: "c3".to_string(),
                    actual: "c2".to_string(),
                    kind: NodeContentMismatchKind::Literal,
                }
            )]
        );
        assert_eq!(*result.value(), json!({}));
    }

    #[test]
    fn test_validate_table_vs_table_with_matcher() {
        let schema_str = r#"
| foo `c1:/buzz/` bar |c2|
|-|-|
| r1 | r2 |
            "#;
        let input_str = r#"
| foo buzz bar |c2|
|-|-|
| r1 | r2 |
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(*result.value(), json!({"c1": "buzz"}));
    }

    #[test]
    fn test_validate_repeated_row_vs_row_simple() {
        let schema_str = r#"
|c2|c2|
|-|-|
|`a:/.*/`|`b:/.*/`|{,}
"#;
        let input_str = r#"
|c2|c2|
|-|-|
|a1|b1|
|a2|b2|
"#;

        let result = ValidatorTester::from_strs_and_validator(
            schema_str,
            input_str,
            RepeatedRowVsRowValidator::from_bounds((None, None)),
        )
        .walk()
        .goto_first_child_then_unwrap() // document -> table
        .goto_first_child_then_unwrap() // table -> header row
        .goto_next_sibling_then_unwrap() // header row -> delimiter row
        .goto_next_sibling_then_unwrap() // delimiter row -> data row
        .peek_nodes(|(s, i)| assert!(both_are_table_data_rows(s, i)))
        .validate_complete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(
            *result.value(),
            json!({"a": ["a1", "a2"], "b": ["b1", "b2"]})
        );
    }

    #[test]
    fn test_validate_table_vs_table_with_repeated_cell() {
        let schema_str = r#"
|c2|c2|
|-|-|
|`a:/.*/`|`b:/.*/`|{,}
            "#;
        let input_str = r#"
|c2|c2|
|-|-|
|a1|b1|
|a2|b2|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(
            *result.value(),
            json!({"a": ["a1", "a2"], "b": ["b1", "b2"]})
        );
    }

    #[test]
    fn test_validate_table_vs_table_with_repeated_cell_and_mismatch() {
        let schema_str = r#"
|c2|c2|
|-|-|
|`a:/.*/`|`b:/xx/`|{,}
            "#;
        let input_str = r#"
|c2|c2|
|-|-|
|a1|xx|
|a2|b2|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(result.errors().len(), 1);
        assert_eq!(
            result.errors(),
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: 14,
                    input_index: 18,
                    expected: "^xx".to_string(),
                    actual: "b2".to_string(),
                    kind: NodeContentMismatchKind::Matcher,
                }
            )]
        );
    }

    #[test]
    fn test_validate_table_vs_table_with_repeated_cell_max_bound() {
        let schema_str = r#"
|c2|c2|
|-|-|
|`a:/.*/`|`b:/.*/`|{,2}
            "#;
        let input_str = r#"
|c2|c2|
|-|-|
|a1|b1|
|a2|b2|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(
            *result.value(),
            json!({"a": ["a1", "a2"], "b": ["b1", "b2"]})
        );
    }

    #[test]
    fn test_validate_table_vs_table_with_repeated_cell_min_bound() {
        let schema_str = r#"
|c2|c2|
|-|-|
|`a:/.*/`|`b:/.*/`|{2,}
            "#;
        let input_str = r#"
|c2|c2|
|-|-|
|a1|b1|
|a2|b2|
|a3|b3|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(
            *result.value(),
            json!({"a": ["a1", "a2", "a3"], "b": ["b1", "b2", "b3"]})
        );
    }

    #[test]
    fn test_validate_table_vs_table_repeated_then_literal() {
        let schema_str = r#"
|c1|c2|
|-|-|
|`a:/.*/`|`b:/.*/`|{,2}
|lit1|lit2|
|lit3|lit4|
            "#;
        let input_str = r#"
|c1|c2|
|-|-|
|a1|b1|
|a2|b2|
|lit1|lit2|
|lit3|lit4|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(
            *result.value(),
            json!({"a": ["a1", "a2"], "b": ["b1", "b2"]})
        );
    }

    #[test]
    fn test_validate_table_vs_table_literal_repeated_literal_sandwich_with_footer() {
        let schema_str = r#"
# Shopping List

| Item | Price |
|:-----|:------|
| Header | 10 |
| `item:/\w+/` | `price:/\d+/` |{,3}
| Footer | 99 |
"#;
        let input_str = r#"
# Shopping List

| Item | Price |
|:-----|:------|
| Header | 10 |
| Apple | 5 |
| Banana | 3 |
| Cherry | 7 |
| Footer | 99 |
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .goto_next_sibling_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_complete();

        assert_eq!(result.errors(), vec![]);
        assert_eq!(
            *result.value(),
            json!({"item": ["Apple", "Banana", "Cherry"], "price": ["5", "3", "7"]})
        );
    }
}
