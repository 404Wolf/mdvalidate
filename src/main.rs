use clap::Parser;
use std::io::{BufReader, Read, Write};
use std::process::exit;
use tracing_subscriber::EnvFilter;

pub mod cmd;
pub mod env;
pub mod mdschema;
mod path_or_stdio;

use crate::cmd::process_stdio;
use crate::env::EnvConfig;
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
    output: Option<String>,
    /// Whether to stop validation on the first error encountered
    #[arg(short, long)]
    fast_fail: bool,
    /// Whether to suppress non-error output
    #[arg(short, long)]
    quiet: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .without_time()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::ENTER
                | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
        )
        .init();

    let args = Args::parse();

    // Load environment configuration
    let env_config = EnvConfig::load();

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
    let mut input_reader = input.reader()?;

    let mut output_writer: &mut Option<&mut Box<dyn Write>> = match args.output {
        Some(ref output_path) => {
            let output_pos = PathOrStdio::from(output_path.clone());
            &mut Some(&mut output_pos.writer()?)
        }
        None => &mut None,
    };

    match process_stdio(
        &schema_str,
        &mut input_reader,
        &mut output_writer,
        input.filepath(),
        args.fast_fail,
        args.quiet,
        env_config.is_debug_mode(),
    ) {
        Err(err) => {
            println!("{}", format!("Error! {}", err).red());
            return Err(Box::new(err));
        }
        Ok((_, errored)) => {
            if errored {
                exit(1)
            }
        }
    }

    Ok(())
}
