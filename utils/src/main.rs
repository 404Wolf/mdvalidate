mod node_print;

use std::env;
use std::fs;
use std::io::{self, Read};

use node_print::PrettyPrint;
use tree_sitter::Parser;
use tree_sitter_markdown::language;

fn read_input() -> io::Result<String> {
    let mut args = env::args().skip(1);
    if let Some(path) = args.next() {
        fs::read_to_string(path)
    } else {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        Ok(input)
    }
}

fn main() {
    let input = read_input().expect("Failed to read input");

    let mut parser = Parser::new();
    parser
        .set_language(&language())
        .expect("Failed to load Markdown language");

    let tree = parser.parse(&input, None).expect("Failed to parse");
    let node = tree.root_node();

    print!("{}", node.pretty_print());
}
