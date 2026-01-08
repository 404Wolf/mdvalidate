use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::validators::ValidatorImpl;
use crate::mdschema::validator::validator_walker::ValidatorWalker;

/// Validate two tables.
pub(super) struct TableVsTableValidator;

impl ValidatorImpl for TableVsTableValidator {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        panic!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::{
        node_walker::validators::test_utils::ValidatorTester, ts_utils::both_are_tables,
    };
    use serde_json::json;

    #[test]
    #[should_panic]
    fn test_validate_heading_vs_heading_simple_headings_so_far_wrong_type() {
        let schema_str = r#"
|c1|
|-|
|r1|
            "#;
        let input_str = r#"
|c1|
|-|
|r1|
"#;

        let result = ValidatorTester::<TableVsTableValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .peek_nodes(|(s, i)| assert!(both_are_tables(s, i)))
            .validate_incomplete();

        assert!(result.errors().is_empty()); // no errors yet
        assert_eq!(result.value(), &json!({}));
    }
}
