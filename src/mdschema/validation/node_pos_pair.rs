use tree_sitter::TreeCursor;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodePosPair {
    schema_index: usize,
    input_index: usize,
}

impl NodePosPair {
    pub fn new(schema_index: usize, input_index: usize) -> Self {
        Self {
            schema_index,
            input_index,
        }
    }

    /// Create a new `NodePosPair` from tree sitter TreeCursors.
    pub fn from_cursors(schema_cursor: &TreeCursor, input_cursor: &TreeCursor) -> Self {
        Self::new(
            schema_cursor.descendant_index(),
            input_cursor.descendant_index(),
        )
    }

    /// Create a new `NodePosPair` from descendant indexes.
    pub fn from_pos(schema_index: usize, input_index: usize) -> Self {
        Self::new(schema_index, input_index)
    }

    /// Convert the `NodePosPair` to a tuple of schema and input indexes.
    pub fn as_pos(&self) -> (usize, usize) {
        (self.schema_index, self.input_index)
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
        schema_cursor: &mut TreeCursor,
        input_cursor: &mut TreeCursor,
    ) {
        let (schema_pos, input_pos) = self.as_pos();

        schema_cursor.goto_descendant(schema_pos);
        input_cursor.goto_descendant(input_pos);
    }
}

impl Default for NodePosPair {
    fn default() -> Self {
        Self::new(0, 0)
    }
}
