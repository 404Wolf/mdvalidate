use serde_json::{Value, json};
use tree_sitter::TreeCursor;

use crate::mdschema::validator::errors::ValidationError;
use crate::mdschema::validator::node_pos_pair::NodePosPair;
use crate::mdschema::validator::utils::join_values;

/// Validation results containing a Value with all matches, vector of all
/// errors, and the descendant indexes after validation
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationResult {
    /// The resulting JSON value with all matches
    pub value: Value,
    /// Vector of all validation errors encountered
    pub errors: Vec<ValidationError>,
    /// The farthest reached position
    farthest_reached_pos: NodePosPair,
}

impl ValidationResult {
    pub fn new(
        value: Value,
        errors: Vec<ValidationError>,
        farthest_reached_pos: NodePosPair,
    ) -> Self {
        Self {
            value,
            errors,
            farthest_reached_pos,
        }
    }

    /// Creates a new `ValidationResult` with an empty JSON object as the value and no errors, starting from given cursor positions.
    pub fn from_cursors(schema_cursor: &TreeCursor, input_cursor: &TreeCursor) -> Self {
        Self::new(
            json!({}),
            Vec::new(),
            NodePosPair::from_cursors(schema_cursor, input_cursor),
        )
    }

    /// Creates a new `ValidationResult` with an empty JSON object as the value and no errors, starting from given descendant indexes.
    pub fn from_descendant_indexes(schema_index: usize, input_index: usize) -> Self {
        Self::new(
            json!({}),
            Vec::new(),
            NodePosPair::from_pos(schema_index, input_index),
        )
    }

    /// Updates the cursor positions to the positions of the given cursors.
    pub fn sync_cursor_pos(&mut self, schema_cursor: &TreeCursor, input_cursor: &TreeCursor) {
        self.farthest_reached_pos = NodePosPair::from_cursors(schema_cursor, input_cursor);
    }

    /// Updates a given pair of cursors to match our position.
    pub fn walk_cursors_to_pos(
        &self,
        schema_cursor: &mut TreeCursor,
        input_cursor: &mut TreeCursor,
    ) {
        self.farthest_reached_pos()
            .walk_cursors_to_pos(input_cursor, schema_cursor);
    }

    /// Add an error to the `ValidationResult`.
    pub fn add_error(&mut self, error: ValidationError) {
        self.errors.push(error);
    }

    /// Whether there are any errors in the `ValidationResult`.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Add a match under an `id`.
    #[allow(dead_code)]
    pub fn set_match(&mut self, id: &str, value: Value) {
        self.value[id] = value;
    }

    /// Join in a different validation result.
    pub fn join_other_result(&mut self, other: &ValidationResult) {
        // Join in their values
        let joined = &mut self.value.clone();
        join_values(joined, other.value.clone());
        self.value = joined.clone();

        // Join in their errors
        self.errors.extend(other.errors.clone());

        // Make the descendant index pair the maximum of the two (as far as we got)
        self.farthest_reached_pos
            .keep_farther_pos(&other.farthest_reached_pos());
    }

    pub fn keep_farther_pos(&mut self, other: &NodePosPair) {
        self.farthest_reached_pos.keep_farther_pos(other);
    }

    /// Get the farthest reached position as a descendant index pair.
    pub fn farthest_reached_pos(&self) -> NodePosPair {
        self.farthest_reached_pos
    }

    /// Get the farthest reached position as a descendant index pair as a mutable reference to our internal one
    pub fn farthest_reached_pos_mut(&mut self) -> &mut NodePosPair {
        &mut self.farthest_reached_pos
    }

    /// Destruct the result into (value, errors, farthest_reached_pos).
    pub fn destruct(self) -> (Value, Vec<ValidationError>, NodePosPair) {
        (self.value, self.errors, self.farthest_reached_pos)
    }

}

impl Default for ValidationResult {
    fn default() -> Self {
        Self {
            value: Default::default(),
            errors: Default::default(),
            farthest_reached_pos: NodePosPair::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_basic_usage() {
        let mut result = ValidationResult::default();

        result.set_match("id", json!("value"));
        result.join_other_result(&ValidationResult::from_descendant_indexes(1, 1));
        result.add_error(ValidationError::ValidatorCreationFailed);

        assert_eq!(result.farthest_reached_pos().to_pos(), (1, 1)); // the farther!
        assert_eq!(result.value, json!({"id": "value"}));

        assert_eq!(result.errors.len(), 1);
        match result.errors[0] {
            ValidationError::ValidatorCreationFailed => (),
            _ => panic!("Unexpected error"),
        }
    }

    #[test]
    fn test_join_other_result() {
        let mut result = ValidationResult::default();
        let other = ValidationResult::from_descendant_indexes(1, 1);

        result.set_match("id", json!("value"));
        result.join_other_result(&other);

        assert_eq!(result.farthest_reached_pos().to_pos(), (1, 1));
        assert_eq!(result.value, json!({"id": "value"}));
        assert_eq!(result.errors.len(), 0);
    }
}
