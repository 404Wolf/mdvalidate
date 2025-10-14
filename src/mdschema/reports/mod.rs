pub mod pretty_print;

use crate::mdschema::errors::{ErrorSeverity, ValidatorError};

#[derive(Debug, Clone)]
pub struct ValidatorReport {
    pub is_valid: bool,
    pub errors: Vec<ValidatorError>,
    pub source_content: String,
}

impl ValidatorReport {
    pub fn new(errors: Vec<ValidatorError>, source_content: String) -> Self {
        let is_valid = errors.iter().all(|e| e.severity != ErrorSeverity::Error);

        Self {
            is_valid,
            errors,
            source_content,
        }
    }
}
