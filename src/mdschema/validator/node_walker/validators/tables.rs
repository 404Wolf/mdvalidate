//! Table validator for node-walker comparisons.
//!
//! Types:
//! - `TableVsTableValidator`: validates table structure (rows, headers, cells)
//!   and delegates cell content checks to textual container validation.
use crate::invariant_violation;
use crate::mdschema::validator::errors::{
    MalformedStructureKind, SchemaViolationError, ValidationError,
};
use crate::mdschema::validator::matcher::matcher_extras::MatcherExtras;
use crate::mdschema::validator::node_pos_pair::NodePosPair;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::validators::containers::ContainerVsContainerValidator;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
use crate::mdschema::validator::ts_types::*;
use crate::mdschema::validator::ts_utils::{get_node_text, waiting_at_end};
use crate::mdschema::validator::validator_walker::ValidatorWalker;
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
            {
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

                    // First check if we are dealing with a special case -- repeated rows!
                    if both_are_table_data_rows(&schema_cursor.node(), &input_cursor.node())
                        && let Some(_bounds) =
                            try_get_repeated_row_bounds(&schema_cursor, walker.schema_str())
                    {
                        todo!()
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
                            (true, false) => {
                                if goto_next_sibling_pair_or_exit(
                                    &schema_cursor,
                                    &input_cursor,
                                    walker,
                                    got_eof,
                                    &mut result,
                                ) {
                                    return result;
                                } else {
                                    return need_to_restart_result;
                                }
                            }
                            (false, true) => {}
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
                    (true, false) => {
                        if goto_next_sibling_pair_or_exit(
                            &schema_cursor,
                            &input_cursor,
                            walker,
                            got_eof,
                            &mut result,
                        ) {
                            return result;
                        } else {
                            return need_to_restart_result;
                        }
                    }
                    _ => {
                        invariant_violation!(
                            result,
                            &schema_cursor,
                            &input_cursor,
                            "table is malformed in a way that should be impossible"
                        )
                    }
                }
            }
        }

        result
    }
}

/// Returns true if we should early return with an error (result was modified).
fn goto_next_sibling_pair_or_exit<'a>(
    schema_cursor: &TreeCursor<'a>,
    input_cursor: &TreeCursor<'a>,
    walker: &ValidatorWalker,
    got_eof: bool,
    result: &mut ValidationResult,
) -> bool {
    if !waiting_at_end(got_eof, walker.input_str(), input_cursor) {
        result.add_error(ValidationError::SchemaViolation(
            SchemaViolationError::MalformedNodeStructure {
                schema_index: schema_cursor.descendant_index(),
                input_index: input_cursor.descendant_index(),
                kind: MalformedStructureKind::MismatchingTableCells,
            },
        ));
        true
    } else {
        false
    }
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
        assert_eq!(result.value(), &json!({}));
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
        assert_eq!(result.value(), &json!({"c1": "buzz"}));
    }
}
