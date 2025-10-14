pub mod pretty_print;

use crate::mdschema::errors::{ErrorSeverity, ValidatorError};
use pretty_print::PrettyPrintReporter;

#[derive(Debug, Clone)]
pub struct ValidatorReport {
    pub is_valid: bool,
    pub errors: Vec<ValidatorError>,
    pub source_content: String,
    pub filename: String,
}

impl ValidatorReport {
    pub fn new(errors: Vec<ValidatorError>, source_content: String, filename: String) -> Self {
        let is_valid = errors.iter().all(|e| e.severity != ErrorSeverity::Error);

        Self {
            is_valid,
            errors,
            source_content,
            filename,
        }
    }

    pub fn valid(source_content: String, filename: String) -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            source_content,
            filename,
        }
    }

    pub fn has_errors(&self) -> bool {
        self.errors
            .iter()
            .any(|e| e.severity == ErrorSeverity::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.errors
            .iter()
            .any(|e| e.severity == ErrorSeverity::Warning)
    }

    pub fn error_count(&self) -> usize {
        self.errors
            .iter()
            .filter(|e| e.severity == ErrorSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.errors
            .iter()
            .filter(|e| e.severity == ErrorSeverity::Warning)
            .count()
    }

    pub fn print_pretty(&self) {
        let reporter = PrettyPrintReporter;

        if !self.errors.is_empty() {
            reporter.print_errors(&self.errors, &self.source_content, &self.filename);
        }

        reporter.print_validation_summary(self.error_count(), self.warning_count());
    }
}
