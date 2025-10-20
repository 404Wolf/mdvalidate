#[derive(Debug, Clone)]
pub struct ValidatorError {
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub severity: ErrorSeverity,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
}

impl ValidatorError {
    pub fn new(
        message: String,
        line: usize,
        column: usize,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
        Self {
            message,
            line,
            column,
            byte_start,
            byte_end,
            severity: ErrorSeverity::Error,
        }
    }

    pub fn from_offset(
        message: String,
        byte_start: usize,
        byte_end: usize,
        input_str: &str,
    ) -> Self {
        let prefix = &input_str[..byte_start];
        let line = prefix.lines().count();
        let column = prefix.lines().last().map(|l| l.len()).unwrap_or(0) + 1;

        Self::new(message, line, column, byte_start, byte_end)
    }

    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = severity;
        self
    }
}
