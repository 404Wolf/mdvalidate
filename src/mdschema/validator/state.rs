use std::collections::HashSet;

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
    errors_so_far: HashSet<Error>,
}

impl ValidatorState {
    pub fn new(schema_str: String, last_input_str: String, got_eof: bool) -> Self {
        Self {
            last_input_str: last_input_str,
            schema_str,
            got_eof: got_eof,
            matches_so_far: Value::Object(Map::new()),
            errors_so_far: HashSet::new(),
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

    pub fn add_new_match(&mut self, key: String, value: Value) {
        // Insert new match into matches_so_far
        if let Value::Object(ref mut existing_map) = self.matches_so_far {
            existing_map.insert(key, value);
        }
    }

    pub fn add_new_matches(&mut self, new_matches: Value) {
        // Merge new_matches into matches_so_far
        if let (Value::Object(ref mut existing_map), Value::Object(new_map)) =
            (&mut self.matches_so_far, new_matches)
        {
            for (key, value) in new_map {
                existing_map.insert(key, value);
            }
        }
    }

    pub fn errors_so_far(&self) -> impl Iterator<Item = &Error> + std::fmt::Debug {
        self.errors_so_far.iter()
    }

    pub fn add_new_error(&mut self, new_error: Error) {
        self.errors_so_far.insert(new_error);
    }

    pub fn add_new_errors(&mut self, new_errors: HashSet<Error>) {
        for error in new_errors {
            self.errors_so_far.insert(error);
        }
    }
}
