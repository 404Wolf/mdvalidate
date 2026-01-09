use core::panic;
use std::os::raw::c_short;
use std::rc::Rc;
use thiserror::Error;

use crate::mdschema::validator::errors::{
    MalformedStructureKind, SchemaViolationError, ValidationError,
};
use crate::mdschema::validator::node_pos_pair::NodePosPair;
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::validators::textual_container::TextualContainerVsTextualContainerValidator;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
#[cfg(feature = "invariant_violations")]
use crate::mdschema::validator::ts_types::{both_are_table_cells, both_are_table_headers};
use crate::mdschema::validator::ts_types::{both_are_table_delimiter_rows, both_are_tables};
use crate::mdschema::validator::ts_utils::waiting_at_end;
use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::trace_cursors;
use crate::{invariant_violation, mdschema::validator::ts_utils::get_node_text};

/// Validate two tables.
pub(super) struct TableVsTableValidator;

impl ValidatorImpl for TableVsTableValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        validate_impl(walker, got_eof)
    }
}

fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
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

                // we are at the first cell initially. we validate the first
                // cell in the input vs the first cell in the schema, and then
                // jump to the next sibling pair. If there is no next sibling
                // pair we are done.
                'col_iter: loop {
                    trace_cursors!(schema_cursor, input_cursor);
                    let cell_result = TextualContainerVsTextualContainerValidator::validate(
                        &walker.with_cursors(&schema_cursor, &input_cursor),
                        got_eof,
                    );
                    result.join_other_result(&cell_result);

                    match (
                        schema_cursor.goto_next_sibling(),
                        input_cursor.goto_next_sibling(),
                    ) {
                        (true, true) => {}
                        (false, false) => break 'col_iter,
                        (true, false) => {
                            // If the schema has another cell but the input
                            // doesn't, it may just be because we are in an
                            // incomplete state.
                            if waiting_at_end(got_eof, walker.input_str(), &input_cursor) {
                                // don't continue FOR NOW. We will want to revalidate the entire table.
                                return need_to_restart_result;
                            } else {
                                result.add_error(ValidationError::SchemaViolation(
                                    SchemaViolationError::MalformedNodeStructure {
                                        schema_index: schema_cursor.descendant_index(),
                                        input_index: input_cursor.descendant_index(),
                                        kind: MalformedStructureKind::MismatchingTableCells,
                                    },
                                ));
                                return result;
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

                    if !both_are_table_delimiter_rows(&schema_cursor.node(), &input_cursor.node()) {
                        break 'wait_for_row;
                    }
                }
                (false, false) => break 'row_iter,
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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MDTableError {
    #[error("Column count mismatch: expected {expected}, got {got}")]
    ColumnCountMismatch { expected: usize, got: usize },
}

struct MDTable {
    columns: usize,
    rows: Vec<Vec<Rc<str>>>,
}

impl MDTable {
    pub fn new(column_count: usize) -> Self {
        MDTable {
            columns: column_count,
            rows: Vec::new(),
        }
    }

    pub fn add_row(&mut self, row: Vec<Rc<str>>) -> Result<(), MDTableError> {
        if row.len() != self.columns {
            return Err(MDTableError::ColumnCountMismatch {
                expected: self.columns,
                got: row.len(),
            });
        }
        self.rows.push(row);
        Ok(())
    }

    pub fn iter_rows(&self) -> impl Iterator<Item = &Vec<Rc<str>>> {
        self.rows.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::{
        errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
        node_walker::validators::test_utils::ValidatorTester,
        ts_types::both_are_tables,
    };
    use serde_json::json;

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
    fn test_validate_table_vs_table_simple_literal_incomplete() {
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
