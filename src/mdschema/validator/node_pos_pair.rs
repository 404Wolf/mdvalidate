use tree_sitter::TreeCursor;

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
