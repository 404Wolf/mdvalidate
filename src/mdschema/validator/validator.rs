use crate::mdschema::reports::validation_report::ValidatorReport;

pub trait Validator {
    fn new(
        schema_str: &str,
        input_str: &str,
        eof: bool,
    ) -> Option<Self>
    where
        Self: Sized;
    fn validate(&mut self) -> bool;
    fn read_input(&mut self, input: &str, eof: bool) -> bool;
    fn report(&self) -> ValidatorReport;
}
