use std::io::Read;

use crate::mdschema::{
    reports::pretty_print::pretty_print_report, validator::validator::Validator,
    ValidationZipperTree,
};

static BUFFER_SIZE: usize = 3;

pub fn validate<R: Read>(
    schema_str: String,
    input: &mut R,
    filename: &str,
) -> Result<(), std::io::Error> {
    let mut input_str = String::new();
    let mut buffer = [0; BUFFER_SIZE];

    let mut validator = ValidationZipperTree::new(schema_str.as_str(), input_str.as_str(), false)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;

    loop {
        validator
            .validate()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let bytes_read = input.read(&mut buffer)?;
        if bytes_read == 0 {
            validator // call one last time and tell it this is EOF
                .read_input(&input_str, true)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
            break;
        }

        let new_text = std::str::from_utf8(&buffer[..bytes_read])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        input_str.push_str(new_text);

        validator
            .read_input(&input_str, false)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    }

    let report = validator.report();
    match pretty_print_report(&report, filename) {
        Ok(pretty) => {
            if !pretty.is_empty() {
                println!("{}", pretty);
            } else {
                println!("Validation success!");
            }
        }
        Err(e) => eprintln!("Error generating report: {}", e),
    }

    Ok(())
}
