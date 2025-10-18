use crate::mdschema::reports::errors::{ErrorSeverity, ValidatorError};

#[derive(Debug, Clone)]
pub struct ValidatorReport {
    pub errors: Vec<ValidatorError>,
    pub source_content: String,
}

impl ValidatorReport {
    pub fn new(errors: Vec<ValidatorError>, source_content: String) -> Self {
        Self {
            errors,
            source_content,
        }
    }

    pub fn is_valid(&self) -> bool {
        !self
            .errors
            .iter()
            .any(|e| e.severity == ErrorSeverity::Error)
    }
}
