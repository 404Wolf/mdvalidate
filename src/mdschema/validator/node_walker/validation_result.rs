use serde_json::{json, Value};

use crate::mdschema::validator::errors::Error;

/// Validation results containing a Value with all matches, vector of all
/// errors, and the descendant indexes after validation
#[derive(Clone, Debug)]
pub struct ValidationResult {
    /// The resulting JSON value with all matches
    pub value: Value,
    /// Vector of all validation errors encountered
    pub errors: Vec<Error>,
    /// The descendant index in the schema after validation
    pub schema_descendant_index: usize,
    /// The descendant index in the input after validation
    pub input_descendant_index: usize,
}

impl ValidationResult {
    pub fn new(
        value: Value,
        errors: Vec<Error>,
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
    pub fn add_error(&mut self, error: Error) {
        self.errors.push(error);
    }

    /// Add a match under an `id`.
    pub fn set_match(&mut self, id: &str, value: Value) {
        self.value[id] = value;
    }

    /// Get the descendant index pair (schema, input)
    pub fn descendant_index_pair(&self) -> (usize, usize) {
        (self.schema_descendant_index, self.input_descendant_index)
    }
}
