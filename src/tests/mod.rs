mod matchers;

#[cfg(test)]
mod test {
    use crate::mdschema::reports::errors::Error;
    use crate::mdschema::Validator;

    pub fn get_report(input_str: &str, schema_str: &str) -> Vec<Error> {
        let mut validator =
            Validator::new(schema_str, input_str, false).expect("Failed to create validator");

        validator.validate().expect("Validation failed");

        validator.errors()
    }

    pub fn report_has_error_that_matches(errors: &Vec<Error>, pattern: &str) -> bool {
        let regex = regex::Regex::new(pattern).unwrap();

        errors
            .iter()
            .any(|error| {
                let error_str = format!("{:?}", error);
                regex.is_match(&error_str)
            })
    }
}
