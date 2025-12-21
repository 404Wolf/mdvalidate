use serde_json::{Map, Value};

use crate::mdschema::validator::{errors::ValidationError, node_walker::ValidationResult, utils::join_values};

pub struct ValidatorState {
    /// The full input string as last read. Not used internally but useful for
    /// debugging or reporting.
    last_input_str: String,
    /// The full schema string. Does not change.
    schema_str: String,
    /// Whether we have received the end of the input. This means that last
    /// input tree descendant index is at the end of the input.
    got_eof: bool,
    /// Map of matches found so far.
    matches_so_far: Value,
    /// Any errors encountered during validation.
    errors_so_far: Vec<ValidationError>,
}

impl ValidatorState {
    pub fn new(schema_str: String, last_input_str: String, got_eof: bool) -> Self {
        Self {
            last_input_str,
            schema_str,
            got_eof,
            matches_so_far: Value::Object(Map::new()),
            errors_so_far: Vec::new(),
        }
    }

    pub fn got_eof(&self) -> bool {
        self.got_eof
    }

    pub fn set_got_eof(&mut self, got_eof: bool) {
        self.got_eof = got_eof;
    }

    pub fn schema_str(&self) -> &str {
        &self.schema_str
    }

    pub fn last_input_str(&self) -> &str {
        &self.last_input_str
    }

    pub fn set_last_input_str(&mut self, new_input: String) {
        self.last_input_str = new_input;
    }

    pub fn matches_so_far(&self) -> &Value {
        &self.matches_so_far
    }

    pub fn join_new_matches(&mut self, new_matches: Value) {
        let joined = &mut self.matches_so_far.clone();
        join_values(joined, new_matches);
        self.matches_so_far = joined.clone();
    }

    pub fn errors_so_far(&self) -> Vec<&ValidationError> {
        self.errors_so_far.iter().collect()
    }

    /// Unpacks a ValidationResult and adds its matches and errors to the state.
    pub fn push_validation_result(&mut self, result: ValidationResult) {
        self.join_new_matches(result.value);
        self.errors_so_far.extend(result.errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_new_matches_objects() {
        let mut state = ValidatorState::new("{}".to_string(), "".to_string(), false);

        let initial_matches = serde_json::json!({ "key1": "value1" });
        state.join_new_matches(initial_matches);

        let new_matches = Value::Object({
            let mut map = Map::new();
            map.insert("key2".to_string(), Value::String("value2".to_string()));
            map
        });

        state.join_new_matches(new_matches);

        if let Value::Object(ref map) = state.matches_so_far() {
            assert_eq!(map.get("key1"), Some(&Value::String("value1".to_string())));
            assert_eq!(map.get("key2"), Some(&Value::String("value2".to_string())));
        } else {
            panic!("matches_so_far is not an object");
        }
    }

    #[test]
    fn test_join_new_matches_arrays() {
        let mut state = ValidatorState::new("{}".to_string(), "".to_string(), false);
        state.matches_so_far = Value::Array(vec![Value::String("value1".to_string())]);

        let new_matches = Value::Array(vec![
            Value::String("value2".to_string()),
            Value::String("value3".to_string()),
        ]);

        state.join_new_matches(new_matches);

        if let Value::Array(ref array) = state.matches_so_far() {
            assert_eq!(array.len(), 3);
            assert_eq!(array[0], Value::String("value1".to_string()));
            assert_eq!(array[1], Value::String("value2".to_string()));
            assert_eq!(array[2], Value::String("value3".to_string()));
        } else {
            panic!("matches_so_far is not an array");
        }
    }
}
