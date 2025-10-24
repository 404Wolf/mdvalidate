mod matchers;

#[cfg(test)]
mod test {
    use crate::mdschema::reports::validation_report::ValidatorReport;
    use crate::mdschema::Validator;

    pub fn get_report(input_str: &str, schema_str: &str) -> ValidatorReport {
        let mut validator =
            Validator::new(schema_str, input_str, false).expect("Failed to create validator");

        validator.validate().expect("Validation failed");

        validator.report()
    }

    pub fn report_has_error_with_str_includes(report: &ValidatorReport, search_str: &str) -> bool {
        report
            .errors
            .iter()
            .any(|error| error.message.contains(search_str))
    }
}
