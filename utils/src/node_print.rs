use ptree::{Style, TreeItem};
use std::borrow::Cow;
use std::io::{self, Write};
use tree_sitter::Node;

pub trait PrettyPrint {
    fn pretty_print(&self) -> String;
    fn pretty_print_with_highlight(&self, indices: &[usize]) -> String;
}

#[derive(Clone)]
struct NodeWrapper<'a> {
    node: Node<'a>,
    index: usize,
    children: Vec<NodeWrapper<'a>>,
    highlight_indices: Vec<usize>,
}

impl<'a> TreeItem for NodeWrapper<'a> {
    type Child = Self;

    fn write_self<W: Write>(&self, f: &mut W, style: &Style) -> io::Result<()> {
        let highlight_marker = if self.highlight_indices.contains(&self.index) {
            " <--"
        } else {
            ""
        };

        write!(
            f,
            "{}",
            style.paint(format!(
                "({}[{}]{}..{}){}",
                self.node.kind(),
                self.index,
                self.node.byte_range().start,
                self.node.byte_range().end,
                highlight_marker
            ))
        )
    }

    fn children(&self) -> Cow<'_, [Self::Child]> {
        Cow::Borrowed(&self.children)
    }
}

fn build_tree<'a>(
    node: Node<'a>,
    next_index: &mut usize,
    highlight_indices: &[usize],
) -> NodeWrapper<'a> {
    let index = *next_index;
    *next_index += 1;

    let mut children = Vec::new();
    let mut cursor = node.walk();

    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            children.push(build_tree(child, next_index, highlight_indices));
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
    }
}

impl PrettyPrint for Node<'_> {
    fn pretty_print(&self) -> String {
        self.pretty_print_with_highlight(&[])
    }

    fn pretty_print_with_highlight(&self, highlight_indices: &[usize]) -> String {
        let mut next_index = 0;
        let wrapper = build_tree(*self, &mut next_index, highlight_indices);
        let mut output = Vec::new();

        ptree::write_tree(&wrapper, &mut output).expect("Failed to write tree");
        String::from_utf8(output).expect("Failed to decode tree")
    }
}
