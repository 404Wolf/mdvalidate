use mdvalidate::mdschema::validator::errors::ValidationError;
use mdvalidate::mdschema::validator::validator::{Validator, ValidatorState};
use serde_json::Value;

pub fn run_test_case(schema: &str, input: &str) -> (Vec<ValidationError>, Value) {
    let mut validator = Validator::new_complete(schema, input).expect("Failed to create validator");
    validator.validate();

    (
        validator.errors_so_far().cloned().collect::<Vec<_>>(),
        validator.matches_so_far().clone(),
    )
}

macro_rules! test_case {
    (
        $fn_name:ident,
        $schema:expr,
        $input:expr,
        $expected_value:expr,
        $expected_errors:expr
    ) => {
        #[test]
        fn $fn_name() {
            let (errors, value) = crate::helpers::run_test_case($schema, $input);
            assert_eq!(errors, $expected_errors, "{}", stringify!($fn_name));
            assert_eq!(value, $expected_value, "{}", stringify!($fn_name));
        }
    };
}
