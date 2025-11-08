use clap::Parser;
use env_logger;
use log::{debug, info};
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;

mod tests;
pub mod cmd;
pub mod mdschema;

use crate::cmd::validate;
use colored::Colorize;

#[derive(Parser, Debug)]
#[command(version, about = "Validate MDS files against a schema")]
struct Args {
    /// Schema file (typically your .mds file)
    schema: PathBuf,
    /// Input Markdown file or "-" for stdin
    input: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    info!("Starting mdvalidate application");

    let result = validate_main();
    if let Err(ref e) = result {
        println!("{}", format!("Error! {}", e).red());
    } else {
        debug!("mdvalidate application finished without errors");
    }

    info!("mdvalidate application completed successfully");
    Ok(())
}

fn validate_main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    debug!(
        "Parsed command line arguments: schema={:?}, input={:?}",
        args.schema, args.input
    );

    let schema_str = args.schema.to_str().ok_or("Invalid schema path")?;
    debug!("Loading schema from: {}", schema_str);
    let schema_src = std::fs::read_to_string(schema_str)?.trim_end().to_string();

    debug!(
        "Schema loaded successfully, length: {} characters",
        schema_src.len()
    );

    let filename = {
        if args.input == "-" {
            "stdin"
        } else {
            args.input.as_str()
        }
    };

    debug!("Processing input from: {}", filename);

    // Handle the input source
    if args.input == "-" {
        debug!("Reading from stdin");
        validate(schema_src, &mut io::stdin(), filename)?;
    } else {
        debug!("Opening file: {}", args.input);
        let file = File::open(&args.input)?;
        let mut reader = BufReader::new(file);
        validate(schema_src, &mut reader, filename)?;
    }

    Ok(())
}
