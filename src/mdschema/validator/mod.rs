pub mod zipper_tree_validator;

use super::reports::ValidatorReport;

pub trait Validator {
    fn new(schema_str: &str, input_str: &str) -> Self;
    fn validate(&self) -> ValidatorReport;
    fn read_input(&self, input: &str);
}
