use crate::mdschema::errors::ErrorSeverity;
use crate::mdschema::reports::ValidatorReport;
use ariadne::{Color, Label, Report, ReportKind, Source};

pub fn pretty_print_report(report: &ValidatorReport) -> Result<String, String> {
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
        Report::build(report_kind, ("Markdown", error.byte_start..error.byte_end))
            .with_message(error.message.clone())
            .with_label(
                Label::new(("Markdown", error.byte_start..error.byte_end))
                    .with_message(error.message.clone())
                    .with_color(color),
            )
            .finish()
            .write(
                ("Markdown", Source::from(&report.source_content)),
                &mut buffer,
            )
            .map_err(|e| e.to_string())?;

        output.push_str(&String::from_utf8_lossy(&buffer));
    }

    Ok(output)
}
