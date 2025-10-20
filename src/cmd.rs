use std::io::Read;

use crate::mdschema::{
    reports::pretty_print::pretty_print_report, validator::validator::Validator,
    ValidationZipperTree,
};
use anyhow::Result;

static BUFFER_SIZE: usize = 3;

pub fn validate<R: Read>(schema_str: String, input: &mut R, filename: &str) -> Result<()> {
    let mut input_str = String::new();
    let mut buffer = [0; BUFFER_SIZE];

    let mut validator = ValidationZipperTree::new(schema_str.as_str(), input_str.as_str(), false)
        .map_err(|e| anyhow::anyhow!("Failed to create validator: {}", e))?;

    loop {
        validator.validate();

        let bytes_read = input.read(&mut buffer)?;
        if bytes_read == 0 {
            if !validator.read_input(&input_str, true) {
                return Err(anyhow::anyhow!("Failed to read final input"));
            }
            break;
        }

        let new_text = std::str::from_utf8(&buffer[..bytes_read])?;
        input_str.push_str(new_text);

        if !validator.read_input(&input_str, false) {
            return Err(anyhow::anyhow!("Failed to read input"));
        }
    }

    let report = validator.report();
    let pretty = pretty_print_report(&report, filename)
        .map_err(|e| anyhow::anyhow!("Error generating report: {}", e))?;

    if !pretty.is_empty() {
        println!("{}", pretty);
    } else {
        println!("Validation success!");
    }

    Ok(())
}
