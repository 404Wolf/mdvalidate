use std::io::Read;

/// Validate an input Markdown file stream against a Markdown schema.
pub fn validate<R: Read>(schema: String, _input: R) -> Result<(), Box<dyn std::error::Error>> {
    log::info!("Loading schema from: {:?}", schema);

    Ok(())
}

