use serde_json::{json, Value};

use crate::mdschema::validator::errors::ValidationError;
use crate::mdschema::validator::utils::join_values;

/// Validation results containing a Value with all matches, vector of all
/// errors, and the descendant indexes after validation
#[derive(Clone, Debug)]
pub struct ValidationResult {
    /// The resulting JSON value with all matches
    pub value: Value,
    /// Vector of all validation errors encountered
    pub errors: Vec<ValidationError>,
    /// The descendant index in the schema after validation
    pub schema_descendant_index: usize,
    /// The descendant index in the input after validation
    pub input_descendant_index: usize,
}

impl ValidationResult {
    pub fn new(
        value: Value,
        errors: Vec<ValidationError>,
        schema_descendant_index: usize,
        input_descendant_index: usize,
    ) -> Self {
        Self {
            value,
            errors,
            schema_descendant_index,
            input_descendant_index,
        }
    }

    /// Creates a new `ValidationResult` with an empty JSON object as the value and no errors.
    pub fn from_empty(schema_descendant_index: usize, input_descendant_index: usize) -> Self {
        Self::new(
            json!({}),
            Vec::new(),
            schema_descendant_index,
            input_descendant_index,
        )
    }

    /// Add an error to the `ValidationResult`.
    pub fn add_error(&mut self, error: ValidationError) {
        self.errors.push(error);
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
        self.schema_descendant_index = self
            .schema_descendant_index
            .max(other.schema_descendant_index);
        self.input_descendant_index = self
            .input_descendant_index
            .max(other.input_descendant_index);
    }

    /// Get the descendant index pair (schema, input)
    pub fn descendant_index_pair(&self) -> (usize, usize) {
        (self.schema_descendant_index, self.input_descendant_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_basic_usage() {
        let mut result = ValidationResult::from_empty(0, 0);

        result.set_match("id", json!("value"));
        result.join_other_result(&ValidationResult::from_empty(1, 1));
        result.add_error(ValidationError::ValidatorCreationFailed);

        assert_eq!(result.descendant_index_pair(), (1, 1)); // the farther!
        assert_eq!(result.value, json!({"id": "value"}));

        assert_eq!(result.errors.len(), 1);
        match result.errors[0] {
            ValidationError::ValidatorCreationFailed => (),
            _ => panic!("Unexpected error"),
        }
    }

    #[test]
    fn test_join_other_result() {
        let mut result = ValidationResult::from_empty(0, 0);
        let other = ValidationResult::from_empty(1, 1);

        result.set_match("id", json!("value"));
        result.join_other_result(&other);

        assert_eq!(result.descendant_index_pair(), (1, 1));
        assert_eq!(result.value, json!({"id": "value", "key": "value"}));
        assert_eq!(result.errors.len(), 0);
    }
}
