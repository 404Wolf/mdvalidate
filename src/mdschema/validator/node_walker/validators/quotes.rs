//! Block quote validator for node-walker comparisons.
//!
//! Types:
//! - `QuoteVsQuoteValidator`: verifies quote node kinds and delegates content
//!   validation to textual containers.
use crate::mdschema::validator::node_walker::ValidationResult;
use crate::mdschema::validator::node_walker::validators::containers::ContainerVsContainerValidator;
use crate::mdschema::validator::node_walker::validators::{Validator, ValidatorImpl};
use crate::mdschema::validator::validator_walker::ValidatorWalker;
use crate::{compare_node_kinds_check, invariant_violation};

/// Validator for block quote nodes.
///
/// This validator handles the validation of block_quote nodes by:
/// 1. Checking that both nodes are block_quote nodes
/// 2. Moving into the first child of both schema and input
/// 3. Delegating to TextualContainerVsTextualContainerValidator for content validation
#[derive(Default)]
pub(super) struct QuoteVsQuoteValidator;

impl ValidatorImpl for QuoteVsQuoteValidator {
    #[track_caller]
    fn validate_impl(&self, walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        let mut result =
            ValidationResult::from_cursors(walker.schema_cursor(), walker.input_cursor());

        let schema_cursor = walker.schema_cursor();
        let input_cursor = walker.input_cursor();

        // Verify both nodes are block_quote nodes
        compare_node_kinds_check!(
            schema_cursor,
            input_cursor,
            walker.schema_str(),
            walker.input_str(),
            result
        );

        // Move into the children
        let mut schema_cursor = schema_cursor.clone();
        let mut input_cursor = input_cursor.clone();

        if !schema_cursor.goto_first_child() {
            #[cfg(feature = "invariant_violations")]
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "schema block_quote has no children"
            );
        }

        if !input_cursor.goto_first_child() {
            #[cfg(feature = "invariant_violations")]
            invariant_violation!(
                result,
                &schema_cursor,
                &input_cursor,
                "input block_quote has no children"
            );
        }

        // Delegate to TextualContainerVsTextualContainerValidator for the children
        return ContainerVsContainerValidator::default()
            .validate(&walker.with_cursors(&schema_cursor, &input_cursor), got_eof);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_utils::ValidatorTester;
    use super::QuoteVsQuoteValidator;
    use crate::mdschema::validator::{
        errors::{NodeContentMismatchKind, SchemaViolationError, ValidationError},
        node_pos_pair::NodePosPair,
    };

    #[test]
    fn test_validate_quote_vs_quote_simple() {
        let schema_str = "> test";
        let input_str = "> test";

        let result = ValidatorTester::<QuoteVsQuoteValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(3, 3));
        assert_eq!(result.errors(), vec![]);
    }

    #[test]
    fn test_validate_quote_vs_quote_mismatch() {
        let schema_str = "> test";
        let input_str = "> testbar";

        let result = ValidatorTester::<QuoteVsQuoteValidator>::from_strs(schema_str, input_str)
            .walk()
            .goto_first_child_then_unwrap()
            .validate_complete();

        assert_eq!(*result.farthest_reached_pos(), NodePosPair::from_pos(3, 3));
        assert_eq!(
            result.errors(),
            vec![ValidationError::SchemaViolation(
                SchemaViolationError::NodeContentMismatch {
                    schema_index: 3,
                    input_index: 3,
                    expected: "test".to_string(),
                    actual: "testbar".to_string(),
                    kind: NodeContentMismatchKind::Literal,
                }
            )]
        );
    }
}
