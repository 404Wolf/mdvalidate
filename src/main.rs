use clap::Parser;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;

pub mod validate;

#[derive(Parser, Debug)]
#[command(version, about = "Validate MDS files against a schema")]
struct Args {
    /// Schema file (typically your .mds file)
    #[arg(short, long)]
    schema: PathBuf,
    /// Input Markdown file or "-" for stdin
    input: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let schema_str = args.schema.to_str().ok_or("Invalid schema path")?;
    let schema_src = std::fs::read_to_string(schema_str)?;

    // Handle the input source
    if args.input == "-" {
        // Use stdin
        println!("Reading from stdin...");
        validate::validate::validate(schema_src, io::stdin())?;
    } else {
        // Use file
        println!("Reading from file: {}", args.input);
        let file = File::open(&args.input)?;
        let reader = BufReader::new(file);
        validate::validate::validate(schema_src, reader)?;
    }

    Ok(())
}
