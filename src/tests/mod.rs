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

    pub fn report_has_error_that_matches(report: &ValidatorReport, pattern: &str) -> bool {
        let regex = regex::Regex::new(pattern).unwrap();

        report
            .errors
            .iter()
            .any(|error| regex.is_match(&error.message))
    }
}
