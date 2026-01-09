#![allow(dead_code)]

use ptree::{Style, TreeItem};
use std::borrow::Cow;
use std::io::{self, Write};
use tree_sitter::Node;

pub struct Printer<'a> {
    node: Node<'a>,
    highlight_indices: Vec<usize>,
    show_text: bool,
}

impl<'a> Printer<'a> {
    fn new(node: Node<'a>) -> Self {
        Self {
            node,
            highlight_indices: Vec::new(),
            show_text: false,
        }
    }

    pub fn highlight(mut self, indices: &[usize]) -> Self {
        self.highlight_indices = indices.to_vec();
        self
    }

    pub fn show_text(mut self) -> Self {
        self.show_text = true;
        self
    }

    pub fn print(self, source: &str) -> String {
        let mut next_index = 0;
        let wrapper = build_tree_with_config(
            self.node,
            &mut next_index,
            &self.highlight_indices,
            if self.show_text { Some(source) } else { None },
        );
        let mut output = Vec::new();

        ptree::write_tree(&wrapper, &mut output).expect("Failed to write tree");
        String::from_utf8(output).expect("Failed to decode tree")
    }
}

pub trait PrettyPrint {
    fn get_pretty_printer(&self) -> Printer<'_>;
}

#[derive(Clone)]
struct NodeWrapper<'a> {
    node: Node<'a>,
    index: usize,
    children: Vec<NodeWrapper<'a>>,
    highlight_indices: Vec<usize>,
    source: Option<&'a str>,
}

impl<'a> TreeItem for NodeWrapper<'a> {
    type Child = Self;

    fn write_self<W: Write>(&self, f: &mut W, style: &Style) -> io::Result<()> {
        let highlight_marker = if self.highlight_indices.contains(&self.index) {
            " <--"
        } else {
            ""
        };

        let text_display = if let Some(source) = self.source {
            let text = self.node.utf8_text(source.as_bytes()).unwrap_or("");
            let text_preview = if text.len() > 50 {
                format!(" \"{}...\"", &text[..47])
            } else {
                format!(" \"{}\"", text)
            };
            text_preview.replace('\n', "\\n")
        } else {
            String::new()
        };

        write!(
            f,
            "{}",
            style.paint(format!(
                "({}[{}]{}..{}){}{}",
                self.node.kind(),
                self.index,
                self.node.byte_range().start,
                self.node.byte_range().end,
                text_display,
                highlight_marker
            ))
        )
    }

    fn children(&self) -> Cow<'_, [Self::Child]> {
        Cow::Borrowed(&self.children)
    }
}

fn build_tree_with_config<'a>(
    node: Node<'a>,
    next_index: &mut usize,
    highlight_indices: &[usize],
    source: Option<&'a str>,
) -> NodeWrapper<'a> {
    let index = *next_index;
    *next_index += 1;

    let mut children = Vec::new();
    let mut cursor = node.walk();

    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            children.push(build_tree_with_config(
                child,
                next_index,
                highlight_indices,
                source,
            ));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    NodeWrapper {
        node,
        index,
        children,
        highlight_indices: highlight_indices.to_vec(),
        source,
    }
}

impl PrettyPrint for Node<'_> {
    fn get_pretty_printer(&self) -> Printer<'_> {
        Printer::new(*self)
    }
}
