use crate::mdschema::reports::{errors::ErrorSeverity, validation_report::ValidatorReport};
use ariadne::{Color, Label, Report, ReportKind, Source};

/// Pretty prints a ValidatorReport using
/// [ariadne](https://github.com/zesterer/ariadne) for nice formatting.
///
/// Returns a String containing the formatted report, or an error message if
/// formatting fails with a message.
pub fn pretty_print_report(report: &ValidatorReport, filename: &str) -> Result<String, String> {
    let mut output = String::new();

    for error in &report.errors {
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

        let mut buffer = Vec::new();
        Report::build(report_kind, (filename, error.byte_start..error.byte_end))
            .with_message(error.message.clone())
            .with_label(
                Label::new((filename, error.byte_start..error.byte_end))
                    .with_message(error.message.clone())
                    .with_color(color),
            )
            .finish()
            .write(
                (filename, Source::from(&report.source_content)),
                &mut buffer,
            )
            .map_err(|e| e.to_string())?;

        output.push_str(&String::from_utf8_lossy(&buffer));
    }

    Ok(output)
}
