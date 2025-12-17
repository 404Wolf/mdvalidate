use serde_json::{Map, Value};

use crate::mdschema::validator::errors::Error;

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
    errors_so_far: Vec<Error>,
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

    #[allow(dead_code)]
    pub fn add_new_match(&mut self, key: String, value: Value) {
        // Insert new match into matches_so_far
        if let Value::Object(ref mut existing_map) = self.matches_so_far {
            existing_map.insert(key, value);
        }
    }

    pub fn join_new_matches(&mut self, new_matches: Value) {
        match (&mut self.matches_so_far, new_matches) {
            (Value::Object(ref mut existing_map), Value::Object(new_map)) => {
                for (key, value) in new_map {
                    existing_map.insert(key, value);
                }
            }
            (Value::Array(ref mut existing_array), Value::Array(new_array)) => {
                existing_array.extend(new_array);
            }
            _ => {}
        }
    }

    pub fn errors_so_far(&self) -> Vec<&Error> {
        self.errors_so_far.iter().collect()
    }

    pub fn add_new_error(&mut self, new_error: Error) {
        self.errors_so_far.push(new_error);
    }

    #[allow(dead_code)]
    pub fn add_new_errors(&mut self, new_errors: impl IntoIterator<Item = Error>) {
        self.errors_so_far.extend(new_errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_new_matches_objects() {
        let mut state = ValidatorState::new("{}".to_string(), "".to_string(), false);
        state.add_new_match("key1".to_string(), Value::String("value1".to_string()));

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
