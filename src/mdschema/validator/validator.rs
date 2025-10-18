use crate::mdschema::reports::validation_report::ValidatorReport;

pub trait Validator {
    fn new(schema_str: &str, input_str: &str, eof: bool) -> Result<Self, Box<dyn std::error::Error>>
    where
        Self: Sized;
    fn validate(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn read_input(&mut self, input: &str, eof: bool) -> Result<(), Box<dyn std::error::Error>>;
    fn report(&self) -> ValidatorReport;
}
