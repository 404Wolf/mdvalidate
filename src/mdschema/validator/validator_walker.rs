use tree_sitter::TreeCursor;

pub struct ValidatorWalker<'a> {
    input_cursor: TreeCursor<'a>,
    schema_cursor: TreeCursor<'a>,
    input_str: &'a str,
    schema_str: &'a str,
}

impl<'a> ValidatorWalker<'a> {
    pub fn new(
        input_cursor: TreeCursor<'a>,
        schema_cursor: TreeCursor<'a>,
        schema_str: &'a str,
        input_str: &'a str,
    ) -> Self {
        Self {
            input_cursor,
            schema_cursor,
            input_str,
            schema_str,
        }
    }

    pub fn from_cursors(
        input_cursor: &TreeCursor<'a>,
        schema_cursor: &TreeCursor<'a>,
        schema_str: &'a str,
        input_str: &'a str,
    ) -> Self {
        Self::new(
            input_cursor.clone(),
            schema_cursor.clone(),
            schema_str,
            input_str,
        )
    }

    pub fn with_cursors(
        &self,
        input_cursor: &TreeCursor<'a>,
        schema_cursor: &TreeCursor<'a>,
    ) -> Self {
        Self::new(
            input_cursor.clone(),
            schema_cursor.clone(),
            self.schema_str,
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
        (&mut self.input_cursor, &mut self.schema_cursor)
    }
}
