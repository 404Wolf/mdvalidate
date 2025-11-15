#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Error {
    SchemaViolation(SchemaViolationError),
    SchemaError(SchemaError),
    ParserError(ParserError),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ParserError {
    ReadAfterGotEOF,
    InvalidUTF8,
    TreesitterError,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaError {
    MultipleMatchersInNodeChildren(usize),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SchemaViolationError {
    /// Mismatch between schema definition and actual node
    NodeTypeMismatch(usize, usize),
    /// Text content of node does not match expected value
    NodeContentMismatch(usize, String),
    /// Nodes have different numbers of children
    ChildrenLengthMismatch(usize, usize),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum NodeContentMismatchError {
    /// A node's text content doesn't match expected literal text
    Text(String),
    /// A matcher's pattern doesn't match
    Matcher(usize),
}
