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

    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = severity;
        self
    }
}
