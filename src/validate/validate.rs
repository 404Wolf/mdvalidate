use std::io::Read;
use tree_sitter::{InputEdit, Parser, Point, Tree};

static BUFFER_SIZE: usize = 3;

/// Validate an input Markdown file stream against a Markdown schema.
pub fn validate<R: Read>(schema: String, input: &mut R) -> Result<(), Box<dyn std::error::Error>> {
    let mut input_parser = Parser::new();
    input_parser
        .set_language(tree_sitter_markdown::language())
        .map_err(|_| "Failed to set language for input parser")?;

    let mut input_tree: Option<Tree> = None;
    let mut input_str = String::new();
    let mut buffer = [0; BUFFER_SIZE];
    let mut offset = 0;

    loop {
        let bytes_read = input.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        let new_text = std::str::from_utf8(&buffer[..bytes_read])?;
        input_str.push_str(new_text);

        if let Some(old_tree) = input_tree.as_mut() {
            let edit = InputEdit {
                start_byte: offset,
                old_end_byte: offset,
                new_end_byte: offset + bytes_read,
                start_position: Point::new(0, offset),
                old_end_position: Point::new(0, offset),
                new_end_position: Point::new(0, offset + bytes_read),
            };
            old_tree.edit(&edit);
        }

        input_tree = input_parser.parse(&input_str, input_tree.as_ref());
        offset += bytes_read;
    }

    Ok(())
}
