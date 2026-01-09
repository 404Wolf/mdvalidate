mod node_print;

use clap::{Parser, Subcommand};
use node_print::PrettyPrint;
use std::fs;
use std::io::{self, Read};
use tree_sitter::Parser as TSParser;
use tree_sitter_markdown::language;

#[derive(Parser)]
#[command(name = "mdvalidate-dev-utils")]
#[command(about = "Development helper utilities for working on mdvalidate", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the markdown syntax tree
    PrintTree {
        /// Input file path (reads from stdin if not provided)
        file: Option<String>,

        /// Show text content of nodes in the tree
        #[arg(short, long)]
        show_text: bool,
    },
}

fn read_input(path: Option<String>) -> io::Result<String> {
    if let Some(path) = path {
        fs::read_to_string(path)
    } else {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        Ok(input)
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::PrintTree { file, show_text } => {
            let input = read_input(file).expect("Failed to read input");

            let mut parser = TSParser::new();
            parser
                .set_language(&language())
                .expect("Failed to load Markdown language");

            let tree = parser.parse(&input, None).expect("Failed to parse");
            let node = tree.root_node();

            let mut printer = node.get_pretty_printer();
            if show_text {
                printer = printer.show_text();
            }
            print!("{}", printer.print(&input));
        }
    }
}
