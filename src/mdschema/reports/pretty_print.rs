use crate::mdschema::errors::{ErrorSeverity, ValidatorError};
use ariadne::{Color, Fmt, Label, Report, ReportKind, Source};

pub struct PrettyPrintReporter;

impl PrettyPrintReporter {
    pub fn print_errors(&self, errors: &[ValidatorError], source_content: &str, filename: &str) {
        for error in errors {
            let report_kind = match error.severity {
                ErrorSeverity::Error => ReportKind::Error,
                ErrorSeverity::Warning => ReportKind::Warning,
                ErrorSeverity::Info => ReportKind::Advice,
            };

            let color = match error.severity {
                ErrorSeverity::Error => Color::Red,
                ErrorSeverity::Warning => Color::Yellow,
                ErrorSeverity::Info => Color::Blue,
            };

            Report::build(report_kind, filename, error.byte_start)
                .with_message(error.message.clone())
                .with_label(
                    Label::new((filename, error.byte_start..error.byte_end))
                        .with_message(error.message.clone())
                        .with_color(color),
                )
                .finish()
                .print((filename, Source::from(source_content)))
                .unwrap();
        }
    }

    pub fn print_validation_summary(&self, total_errors: usize, total_warnings: usize) {
        if total_errors == 0 && total_warnings == 0 {
            println!(
                "{}",
                "✓ Validation passed with no errors or warnings".fg(Color::Green)
            );
        } else {
            let error_text = if total_errors > 0 {
                format!("{} error{}", total_errors, if total_errors == 1 { "" } else { "s" })
                    .fg(Color::Red)
                    .to_string()
            } else {
                String::new()
            };

            let warning_text = if total_warnings > 0 {
                format!(
                    "{} warning{}",
                    total_warnings,
                    if total_warnings == 1 { "" } else { "s" }
                )
                .fg(Color::Yellow)
                .to_string()
            } else {
                String::new()
            };

            let separator = if total_errors > 0 && total_warnings > 0 {
                " and "
            } else {
                ""
            };

            println!(
                "✗ Validation completed with {}{}{}",
                error_text, separator, warning_text
            );
        }
    }
}