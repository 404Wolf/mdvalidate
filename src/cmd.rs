use crate::mdschema::validator::{
    errors::{pretty_print_error, ValidationError},
    nodes::NodeValidationResult,
    validator::Validator,
};
use colored::Colorize;
use std::io::{Read, Write};
use tree_sitter::Tree;

static DEFAULT_BUFFER_SIZE: usize = 2048;

pub fn process<R: Read>(
    schema_str: &String,
    input: &mut R,
    fast_fail: bool,
) -> Result<(NodeValidationResult, Tree, String), ValidationError> {
    let buffer_size = get_buffer_size();

    let mut input_str = String::new();
    let mut buffer = vec![0; buffer_size];

    let mut validator = Validator::new(schema_str.as_str(), input_str.as_str(), false)
        .ok_or(ValidationError::ValidatorCreationFailed)?;

    loop {
        let bytes_read = input.read(&mut buffer)?;

        // If we're done reading, mark EOF
        if bytes_read == 0 {
            if let Err(e) = validator.read_input(&input_str, true) {
                return Err(ValidationError::ReadInputFailed(e));
            }
            validator.validate();

            break;
        }

        let new_text = std::str::from_utf8(&buffer[..bytes_read])?;
        input_str.push_str(new_text);

        if let Err(e) = validator.read_input(&input_str, false) {
            return Err(ValidationError::ReadInputFailed(e));
        }
        validator.validate();

        // Check for fast-fail AFTER validation
        if fast_fail && validator.errors_so_far().count() > 0 {
            break;
        }
    }

    let errors: Vec<_> = validator.errors_so_far().cloned().collect();
    let matches = validator.matches_so_far().clone();
    let input_tree = validator.input_tree;

    Ok(((errors, matches), input_tree, input_str))
}

pub fn process_stdio<R: Read, W: Write>(
    schema_str: &String,
    input: &mut R,
    output: &mut Option<&mut W>,
    filename: &str,
    fast_fail: bool,
    quiet: bool,
) -> Result<(NodeValidationResult, bool), ValidationError> {
    let ((errors, matches), input_tree, input_str) = process(schema_str, input, fast_fail)?;

    let mut errored = false;
    if errors.is_empty() {
        match (output, quiet) {
            (None, false) => {
                println!(
                    "{}",
                    format!("File {} validated successfully! No errors found.", filename).green()
                );
            }
            (Some(out), false) => {
                writeln!(out, "{}", matches)?;
            }
            _ => {}
        }
    } else {
        for error in &errors {
            let pretty = pretty_print_error(&input_tree, error, &input_str, filename)
                .map_err(ValidationError::PrettyPrintFailed)?;
            eprintln!("{}", pretty);
            errored = true;
        }
    }

    Ok(((errors, matches), errored))
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

    fn get_validator<R: Read>(schema: &String, mut input: R, eof: bool) -> NodeValidationResult {
        let ((errors, matches), _, _) =
            process(schema, &mut input, eof).expect("Validation should complete without errors");
        (errors, matches)
    }

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
        let reader = Cursor::new(input_data.as_bytes());

        let (errors, _) = get_validator(&schema_str, reader, false);
        assert!(errors.is_empty(), "Should have no errors");
    }

    #[test]
    fn test_validate_with_two_byte_reads() {
        let schema_str = "# Hi there!".to_string();
        let input_data = "# Hi there!";
        let cursor = Cursor::new(input_data.as_bytes());
        let reader = LimitedReader::new(cursor, 2);

        let (errors, _) = get_validator(&schema_str, reader, false);
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
        let reader = LimitedReader::new(cursor, 1000);

        let (errors, _) = get_validator(&schema_str, reader, false);
        assert!(
            errors.is_empty(),
            "Should have no errors for matching content"
        );
    }

    #[test]
    fn test_validate_stream_input_against_matcher() {
        let schema_str = r#"# CSDS 999 Assignment `assignment_number:/\d+/`

# `title:/(([A-Z][a-z]+ )|and |the )+([A-Z][a-z]+)/`

## `first_name:/[A-Z][a-z]+/`
## `last_name:/[A-Z][a-z]+/`

This is a shopping list:

- `grocery_list_item:/Hello \w+/`
    - `grocery_item_notes:/.*/`"#
            .to_string();

        let input_data = r#"# CSDS 999 Assignment 7

# The Curious and Practical Garden

## Wolf
## Mermelstein

This is a shopping list:

- Hello Apples
    - Fresh from market"#;

        let cursor = Cursor::new(input_data.as_bytes());
        let reader = LimitedReader::new(cursor, 2);

        let (errors, _) = get_validator(&schema_str, reader, false);
        assert!(
            errors.is_empty(),
            "should have no errors but found: {:?}",
            errors
        );
    }

    #[test]
    fn test_streaming_input_with_errors() {
        let schema_str = r#"# CSDS"#.to_string();
        let input_data = r#"# JSDS"#;

        let cursor = Cursor::new(input_data.as_bytes());
        let reader = LimitedReader::new(cursor, 2);

        let (errors, matches) = get_validator(&schema_str, reader, false);
        assert!(
            !errors.is_empty(),
            "Expected validation errors for mismatched input but found none."
        );
        assert!(matches.is_null() || matches.as_object().map_or(true, |obj| obj.is_empty()));
    }

    #[test]
    fn test_multiple_nodes_with_one_error_receives_one_error_once() {
        let schema_str = r#"# CSDS 999 Assignment `assignment_number:/\d+/`

This is a test

This is a test

This is a test"#
            .to_string();
        let input_data = r#"# CSDS 999 Assignment dd

This is a test

This is a test

This is a test"#;

        let cursor = Cursor::new(input_data.as_bytes());
        let reader = LimitedReader::new(cursor, 9);

        let (errors, matches) = get_validator(&schema_str, reader, false);
        assert_eq!(
            errors.len(),
            1,
            "Expected exactly one error but found {:?}",
            errors
        );
        assert!(matches.is_null() || matches.as_object().map_or(true, |obj| obj.is_empty()));
    }

    #[test]
    fn test_process_stdio_with_fake_writer_gets_json_output() {
        let schema_str = "# Hi `name:/[A-Za-z]+/`!".to_string();
        let input_data = "# Hi Wolf!";

        let cursor = Cursor::new(input_data.as_bytes());
        let mut reader = LimitedReader::new(cursor, 4);
        let mut output: Vec<u8> = Vec::new();
        let mut output_option: Option<&mut Vec<u8>> = Some(&mut output);
        let (result, errored) = process_stdio(
            &schema_str,
            &mut reader,
            &mut output_option,
            "test.md",
            false,
            false,
        )
        .expect("Processing should complete without errors");

        assert!(!errored, "There should be no errors for matching input");

        let output_str = String::from_utf8(output).expect("Output should be valid UTF-8");
        assert_eq!(output_str, "{\"name\":\"Wolf\"}\n",);
        assert_eq!(
            output_str,
            result.1.to_string() + "\n",
            "Output JSON should match expected matches"
        );
    }
}
