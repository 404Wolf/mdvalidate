use clap::Parser;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;

pub mod mdschema;

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
        mdschema::validate(schema_src, &mut io::stdin())?;
    } else {
        let file = File::open(&args.input)?;
        let mut reader = BufReader::new(file);
        mdschema::validate(schema_src, &mut reader)?;
    }

    Ok(())
}
