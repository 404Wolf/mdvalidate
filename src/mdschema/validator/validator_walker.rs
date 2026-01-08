use tree_sitter::TreeCursor;

pub struct ValidatorWalker<'a> {
    schema_cursor: TreeCursor<'a>,
    schema_str: &'a str,
    input_cursor: TreeCursor<'a>,
    input_str: &'a str,
}

impl<'a> ValidatorWalker<'a> {
    pub fn new(
        schema_cursor: TreeCursor<'a>,
        schema_str: &'a str,
        input_cursor: TreeCursor<'a>,
        input_str: &'a str,
    ) -> Self {
        Self {
            schema_cursor,
            schema_str,
            input_cursor,
            input_str,
        }
    }

    pub fn from_cursors(
        schema_cursor: &TreeCursor<'a>,
        schema_str: &'a str,
        input_cursor: &TreeCursor<'a>,
        input_str: &'a str,
    ) -> Self {
        Self::new(
            schema_cursor.clone(),
            schema_str,
            input_cursor.clone(),
            input_str,
        )
    }

    pub fn with_cursors(
        &self,
        schema_cursor: &TreeCursor<'a>,
        input_cursor: &TreeCursor<'a>,
    ) -> Self {
        Self::new(
            schema_cursor.clone(),
            self.schema_str,
            input_cursor.clone(),
            self.input_str,
        )
    }

    pub fn input_cursor(&self) -> &TreeCursor<'a> {
        &self.input_cursor
    }

    pub fn schema_cursor(&self) -> &TreeCursor<'a> {
        &self.schema_cursor
    }

    pub fn input_str(&self) -> &str {
        self.input_str
    }

    pub fn schema_str(&self) -> &str {
        self.schema_str
    }

    pub fn cursors_mut(&mut self) -> (&mut TreeCursor<'a>, &mut TreeCursor<'a>) {
        (&mut self.schema_cursor, &mut self.input_cursor)
    }
}
