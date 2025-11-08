use std::io::Read;

use crate::mdschema::{
    reports::{errors::Error, pretty_print::pretty_print_error},
    Validator,
};
use anyhow::Result;
use colored::*;
use log::{debug, info, trace};

static DEFAULT_BUFFER_SIZE: usize = 2048;

pub fn validate<R: Read>(
    schema_str: String,
    input: &mut R,
    filename: &str,
) -> Result<Vec<Error>> {
    let buffer_size = get_buffer_size();

    debug!("Starting validation for file: {}", filename);
    debug!("Schema length: {} characters", schema_str.len());

    let mut input_str = String::new();
    let mut buffer = vec![0; buffer_size];

    debug!("Creating validator with buffer size: {}", buffer_size);
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
            if let Err(e) = validator.read_input(&input_str, true) {
                return Err(anyhow::anyhow!("Error reading input at EOF: {:?}", e));
            }
            break;
        }

        let new_text = std::str::from_utf8(&buffer[..bytes_read])?;
        input_str.push_str(new_text);

        trace!("Input string length now: {} characters", input_str.len());

        debug!(
            "Processing input for validation (iteration #{})",
            iteration_count
        );
        if let Err(e) = validator.read_input(&input_str, false) {
            return Err(anyhow::anyhow!("Error reading input: {:?}", e));
        }
        if let Err(e) = validator.validate() {
            return Err(anyhow::anyhow!("Validation errors: {:?}", e));
        }
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
    let errors = validator.errors();
    let input_tree = validator.input_tree.clone();

    let mut pretty_output = String::new();
    for error in &errors {
        let pretty = pretty_print_error(input_tree.clone(), error, &input_str, filename)
            .map_err(|e| anyhow::anyhow!("Error generating report: {}", e))?;
        pretty_output.push_str(&pretty);
    }

    if !pretty_output.is_empty() {
        info!("Validation completed with issues found");
        println!("{}", pretty_output);
    } else {
        println!("{}", "Validation success! Input matches schema.".green());
    }

    debug!("Validation function completed for file: {}", filename);

    Ok(errors)
}

fn get_buffer_size() -> usize {
    std::env::var("BUFFER_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_BUFFER_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, Read};

    /// A custom reader that only reads a specific number of bytes at a time
    struct LimitedReader<R> {
        inner: R,
        max_bytes_per_read: usize,
    }

    impl<R: Read> LimitedReader<R> {
        fn new(inner: R, max_bytes_per_read: usize) -> Self {
            Self {
                inner,
                max_bytes_per_read,
            }
        }
    }

    impl<R: Read> Read for LimitedReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let bytes_to_read = std::cmp::min(self.max_bytes_per_read, buf.len());
            self.inner.read(&mut buf[..bytes_to_read])
        }
    }

    #[test]
    fn test_limited_reader_actually_limits_bytes() {
        let data = "Hello, world! This is a longer string.";
        let cursor = Cursor::new(data.as_bytes());
        let mut limited_reader = LimitedReader::new(cursor, 3);

        // First read should only get 3 bytes
        let mut buf = [0u8; 10];
        let bytes_read = limited_reader.read(&mut buf).unwrap();
        assert_eq!(bytes_read, 3);
        assert_eq!(&buf[..bytes_read], b"Hel");

        // Second read should get next 3 bytes
        let bytes_read = limited_reader.read(&mut buf).unwrap();
        assert_eq!(bytes_read, 3);
        assert_eq!(&buf[..bytes_read], b"lo,");

        // Third read should get next 3 bytes
        let bytes_read = limited_reader.read(&mut buf).unwrap();
        assert_eq!(bytes_read, 3);
        assert_eq!(&buf[..bytes_read], b" wo");

        // Now keep reading to make sure we get the full thing
        let mut total_data = Vec::new();
        total_data.extend_from_slice(&buf[..3]);
        total_data.extend_from_slice(&buf[..3]);
        total_data.extend_from_slice(&buf[..3]);

        loop {
            let bytes_read = limited_reader.read(&mut buf).unwrap();
            if bytes_read == 0 {
                break;
            }
            total_data.extend_from_slice(&buf[..bytes_read]);
        }
    }

    #[test]
    fn test_validate_with_cursor() {
        let schema_str = "# Hi there!".to_string();
        let input_data = "# Hi there!";
        let mut reader = Cursor::new(input_data.as_bytes());

        let result = validate(schema_str, &mut reader, "test_file.md");

        let errors = result.unwrap();
        assert!(errors.is_empty(), "Should have no errors");
    }

    #[test]
    fn test_validate_with_two_byte_reads() {
        let schema_str = "# Hi there!".to_string();
        let input_data = "# Hi there!";
        let cursor = Cursor::new(input_data.as_bytes());
        let mut reader = LimitedReader::new(cursor, 2);

        // This test should not panic and should complete successfully
        let result = validate(schema_str, &mut reader, "test_file.md");

        // The validation should succeed (no errors in the function execution)
        let errors = result.expect("Validation should complete without errors");
        assert!(
            errors.is_empty(),
            "Should have no errors for matching content"
        );
    }

    #[test]
    fn test_validate_with_thousand_byte_reads() {
        let schema_str = "# Hi there!".to_string();
        let input_data = "# Hi there!";
        let cursor = Cursor::new(input_data.as_bytes());
        let mut reader = LimitedReader::new(cursor, 1000);

        // This test should not panic and should complete successfully
        let result = validate(schema_str, &mut reader, "test_file.md");

        // The validation should succeed (no errors in the function execution)
        let errors = result.expect("Validation should complete without errors");
        assert!(
            errors.is_empty(),
            "Should have no errors for matching content"
        );
    }
}
