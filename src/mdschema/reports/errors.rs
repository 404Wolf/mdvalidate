#[derive(Debug, Clone)]
pub enum Error {
    SchemaViolation(SchemaViolationError),
    SchemaError(SchemaError),
    ParserError(ParserError),
}

#[derive(Debug, Clone)]
pub enum ParserError {
    ReadAfterGotEOF,
    InvalidUTF8,
    TreesitterError,
}

#[derive(Debug, Clone)]
pub enum SchemaError {
    MultipleMatchersInNodeChildren(usize),
}

#[derive(Debug, Clone)]
pub enum SchemaViolationError {
    /// Mismatch between schema definition and actual node
    NodeTypeMismatch(usize, usize),
    /// Text content of node does not match expected value
    NodeContentMismatch(usize, String),
    /// Nodes have different numbers of children
    ChildrenLengthMismatch(usize, usize),
    /// When a given parent has more than a single code child in the schema
    MultipleMatchers(usize), // Just store the count, not the actual nodes
}

pub enum NodeContentMismatchError {
    /// A node's text content doesn't match expected literal text
    Text(String),
    /// A matcher's pattern doesn't match
    Matcher(usize),
}
