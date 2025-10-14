pub mod zipper_tree_validator;

use super::reports::ValidatorReport;

pub trait Validator {
    fn new(schema_str: &str, input_str: &str) -> Result<Self, Box<dyn std::error::Error>>
    where
        Self: Sized;
    fn validate(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn read_input(&self, input: &str) -> Result<(), Box<dyn std::error::Error>>;
    fn report(&self) -> ValidatorReport;
}
