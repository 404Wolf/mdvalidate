use serde_json::{Map, Value};
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    errors::ValidationError, node_walker::ValidationResult, utils::join_values,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodePosPair {
    schema_index: usize,
    input_index: usize,
}

impl NodePosPair {
    pub fn new(input_index: usize, schema_index: usize) -> Self {
        Self {
            input_index,
            schema_index,
        }
    }

    /// Create a new `NodePosPair` from tree sitter TreeCursors.
    pub fn from_cursors(schema_cursor: &TreeCursor, input_cursor: &TreeCursor) -> Self {
        Self::new(
            input_cursor.descendant_index(),
            schema_cursor.descendant_index(),
        )
    }

    /// Create a new `NodePosPair` from descendant indexes.
    pub fn from_pos(schema_index: usize, input_index: usize) -> Self {
        Self::new(input_index, schema_index)
    }

    /// Convert the `NodePosPair` to a tuple of input and schema indexes.
    pub fn to_pos(&self) -> (usize, usize) {
        (self.input_index, self.schema_index)
    }

    /// Join another `NodePosPair`, keeping the farther positions for both
    /// schema and input indexes.
    pub fn keep_farther_pos(&mut self, other: &Self) {
        self.input_index = self.input_index.max(other.input_index);
        self.schema_index = self.schema_index.max(other.schema_index);
    }

    /// Walk a pair of cursors to the current position of the `NodePosPair`.
    pub fn walk_cursors_to_pos(
        &self,
        input_cursor: &mut TreeCursor,
        schema_cursor: &mut TreeCursor,
    ) {
        let (input_pos, schema_pos) = self.to_pos();

        input_cursor.goto_descendant(input_pos);
        schema_cursor.goto_descendant(schema_pos);
    }
}

impl Default for NodePosPair {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

#[derive(Clone, Debug)]
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
    /// Our farthest reached position.
    farthest_reached_pos: NodePosPair,
}

impl ValidatorState {
    pub fn new(
        schema_str: String,
        last_input_str: String,
        got_eof: bool,
        farthest_reached_pos: NodePosPair,
    ) -> Self {
        Self {
            last_input_str,
            schema_str,
            got_eof,
            matches_so_far: Value::Object(Map::new()),
            errors_so_far: Vec::new(),
            farthest_reached_pos,
        }
    }

    pub fn from_beginning(schema_str: String, last_input_str: String, got_eof: bool) -> Self {
        Self::new(
            schema_str,
            last_input_str,
            got_eof,
            NodePosPair::default(),
        )
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

    /// All matches we have accumulated.
    pub fn matches_so_far(&self) -> &Value {
        &self.matches_so_far
    }

    /// Join a set of matches into ours.
    pub fn join_new_matches(&mut self, new_matches: Value) {
        let joined = &mut self.matches_so_far.clone();
        join_values(joined, new_matches);
        self.matches_so_far = joined.clone();
    }

    /// All errors we have accumulated.
    pub fn errors_so_far(&self) -> Vec<&ValidationError> {
        self.errors_so_far.iter().collect()
    }

    /// Unpacks a ValidationResult and adds its matches and errors to the state.
    pub fn push_validation_result(&mut self, result: ValidationResult) {
        let result_descendant_index_pair = result.farthest_reached_pos();
        self.join_new_matches(result.value);
        self.errors_so_far.extend(result.errors);
        self.farthest_reached_pos = result_descendant_index_pair;
    }

    /// Our farthest reached position.
    pub fn farthest_reached_pos(&self) -> NodePosPair {
        self.farthest_reached_pos
    }

    pub fn set_farthest_reached_pos(&mut self, farthest_reached_pos: NodePosPair) {
        self.farthest_reached_pos = farthest_reached_pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_new_matches_objects() {
        let mut state = ValidatorState::from_beginning("{}".to_string(), "".to_string(), false);

        let initial_matches = serde_json::json!({ "key1": "value1" });
        state.join_new_matches(initial_matches);

        let new_matches = Value::Object({
            let mut map = Map::new();
            map.insert("key2".to_string(), Value::String("value2".to_string()));
            map
        });

        state.join_new_matches(new_matches);

        if let Value::Object(map) = state.matches_so_far() {
            assert_eq!(map.get("key1"), Some(&Value::String("value1".to_string())));
            assert_eq!(map.get("key2"), Some(&Value::String("value2".to_string())));
        } else {
            panic!("matches_so_far is not an object");
        }
    }

    #[test]
    fn test_join_new_matches_arrays() {
        let mut state = ValidatorState::from_beginning("{}".to_string(), "".to_string(), false);
        state.matches_so_far = Value::Array(vec![Value::String("value1".to_string())]);

        let new_matches = Value::Array(vec![
            Value::String("value2".to_string()),
            Value::String("value3".to_string()),
        ]);

        state.join_new_matches(new_matches);

        if let Value::Array(array) = state.matches_so_far() {
            assert_eq!(array.len(), 3);
            assert_eq!(array[0], Value::String("value1".to_string()));
            assert_eq!(array[1], Value::String("value2".to_string()));
            assert_eq!(array[2], Value::String("value3".to_string()));
        } else {
            panic!("matches_so_far is not an array");
        }
    }
}
