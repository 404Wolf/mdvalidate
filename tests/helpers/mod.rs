use mdvalidate::mdschema::validator::errors::ValidationError;
use mdvalidate::mdschema::validator::validator::Validator;
use serde_json::Value;

pub fn run_test_case(
    name: &str,
    schema: &str,
    input: &str,
    expected_value: Value,
    expected_errors: Vec<ValidationError>,
) {
    let mut validator =
        Validator::new_complete(schema, input).expect("Failed to create validator");
    validator.validate();

    let errors = validator.errors_so_far().cloned().collect::<Vec<_>>();
    let value = validator.matches_so_far().clone();

    assert_eq!(errors, expected_errors, "{}", name);
    assert_eq!(value, expected_value, "{}", name);
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
            crate::helpers::run_test_case(
                stringify!($fn_name),
                $schema,
                $input,
                $expected_value,
                $expected_errors,
            );
        }
    };
}
