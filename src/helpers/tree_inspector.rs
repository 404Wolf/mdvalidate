use std::io::{self, Read};

use mdvalidate::helpers::node_print::PrettyPrint;

pub fn main() {
    use tree_sitter::Parser;
    use tree_sitter_markdown::language;

    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .expect("Failed to read from stdin");

    let mut parser = Parser::new();
    parser.set_language(&language()).unwrap();
    let tree = parser
        .parse(&input, None)
        .expect("Failed to parse markdown");

    let node = tree.root_node();
    println!("{}", node.pretty_print());
    print!("{}", input);
}
