use tree_sitter::TreeCursor;

#[derive(Clone)]
pub struct NodeCursorPair<'a> {
    pub input_cursor: TreeCursor<'a>,
    pub schema_cursor: TreeCursor<'a>,
    pub input_str: &'a str,
    pub schema_str: &'a str,
}

impl<'a> NodeCursorPair<'a> {
    pub fn new(
        input_cursor: TreeCursor<'a>,
        schema_cursor: TreeCursor<'a>,
        input_str: &'a str,
        schema_str: &'a str,
    ) -> Self {
        Self {
            input_cursor,
            schema_cursor,
            input_str,
            schema_str,
        }
    }

    pub fn with_cursors(
        &self,
        input_cursor: TreeCursor<'a>,
        schema_cursor: TreeCursor<'a>,
    ) -> Self {
        Self {
            input_cursor,
            schema_cursor,
            input_str: self.input_str,
            schema_str: self.schema_str,
        }
    }
}
