use clap::Parser;
use env_logger;
use std::io::{BufReader, Read};

pub mod cmd;
pub mod mdschema;
mod path_or_stdio;

use crate::cmd::validate;
use crate::path_or_stdio::PathOrStdio;
use colored::Colorize;

#[derive(Parser, Debug)]
#[command(version, about = "Validate MDS files against a schema")]
struct Args {
    /// Schema file (typically your .mds file)
    schema: String,
    /// Input Markdown file or "-" for stdin
    input: String,
    /// Output JSON file for discovered matches or "-" for stdout
    output: String,
    /// Whether to stop validation on the first error encountered
    #[arg(short, long)]
    fast_fail: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let result = validate_main();
    if let Err(ref e) = result {
        println!("{}", format!("Error! {}", e).red());
    }

    Ok(())
}

fn validate_main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let schema_src = PathOrStdio::from(args.schema);
    let schema_src = schema_src.reader().or_else(|e| {
        Err(format!(
            "Failed to open schema file '{}': {}",
            schema_src.filepath(),
            e
        ))
    })?;
    let mut schema_str = String::new();
    BufReader::new(schema_src).read_to_string(&mut schema_str)?;

    let input = PathOrStdio::from(args.input);
    let output = PathOrStdio::from(args.output);

    let mut input_reader = input.reader().or_else(|e| {
        Err(format!(
            "Failed to open input file '{}': {}",
            input.filepath(),
            e
        ))
    })?;

    validate(
        &schema_str,
        &mut input_reader,
        &output.filepath(),
        args.fast_fail,
    )?;

    Ok(())
}
