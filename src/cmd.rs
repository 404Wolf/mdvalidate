use std::io::Read;

use crate::mdschema::{reports::pretty_print::pretty_print_report, Validator};
use anyhow::Result;
use log::{debug, info, trace};

static BUFFER_SIZE: usize = 300;

pub fn validate<R: Read>(schema_str: String, input: &mut R, filename: &str) -> Result<()> {
    debug!("Starting validation for file: {}", filename);
    debug!("Schema length: {} characters", schema_str.len());

    let mut input_str = String::new();
    let mut buffer = [0; BUFFER_SIZE];

    debug!("Creating validator with buffer size: {}", BUFFER_SIZE);
    let mut validator = Validator::new(schema_str.as_str(), input_str.as_str(), false)
        .ok_or_else(|| anyhow::anyhow!("Failed to create validator"))?;

    debug!("Validator created successfully");

    let mut total_bytes_read = 0;
    let mut iteration_count = 0;

    loop {
        iteration_count += 1;
        trace!("Reading iteration #{}", iteration_count);

        let bytes_read = input.read(&mut buffer)?;
        total_bytes_read += bytes_read;

        debug!(
            "Read {} bytes in iteration #{} (total: {} bytes)",
            bytes_read, iteration_count, total_bytes_read
        );

        // If we're done reading, mark EOF
        if bytes_read == 0 {
            debug!("Reached EOF, processing final input");
            validator.read_input(&input_str, true)?;
            break;
        }

        let new_text = std::str::from_utf8(&buffer[..bytes_read])?;
        input_str.push_str(new_text);

        trace!("Input string length now: {} characters", input_str.len());

        debug!(
            "Processing input for validation (iteration #{})",
            iteration_count
        );
        validator.read_input(&input_str, false)?;
        validator.validate()?;
        trace!(
            "Validation step completed for iteration #{}",
            iteration_count
        );
    }

    debug!(
        "Validation loop completed after {} iterations",
        iteration_count
    );
    debug!("Total bytes processed: {}", total_bytes_read);

    debug!("Generating validation report");
    let report = validator.report();
    let pretty = pretty_print_report(&report, filename)
        .map_err(|e| anyhow::anyhow!("Error generating report: {}", e))?;

    if !pretty.is_empty() {
        info!("Validation completed with issues found");
        println!("{}", pretty);
    } else {
        info!("Validation completed successfully - no issues found");
        println!("Validation success!");
    }

    debug!("Validation function completed for file: {}", filename);
    Ok(())
}
