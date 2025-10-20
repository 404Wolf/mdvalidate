use std::io::Read;

use crate::mdschema::{reports::pretty_print::pretty_print_report, Validator};
use anyhow::Result;

static BUFFER_SIZE: usize = 300;

pub fn validate<R: Read>(schema_str: String, input: &mut R, filename: &str) -> Result<()> {
    let mut input_str = String::new();
    let mut buffer = [0; BUFFER_SIZE];

    let mut validator = Validator::new(schema_str.as_str(), input_str.as_str(), false)
        .ok_or_else(|| anyhow::anyhow!("Failed to create validator"))?;

    loop {
        let bytes_read = input.read(&mut buffer)?;

        // If we're done reading, mark EOF
        if bytes_read == 0 {
            validator.read_input(&input_str, true)?;
            break;
        }

        let new_text = std::str::from_utf8(&buffer[..bytes_read])?;
        input_str.push_str(new_text);

        validator.read_input(&input_str, false)?;
        validator.validate()?;
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
