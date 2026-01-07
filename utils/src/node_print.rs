use ptree::{Style, TreeItem};
use std::borrow::Cow;
use std::io::{self, Write};
use tree_sitter::Node;

pub trait PrettyPrint {
    fn pretty_print(&self) -> String;
}

#[derive(Clone)]
struct NodeWrapper<'a> {
    node: Node<'a>,
    index: usize,
    children: Vec<NodeWrapper<'a>>,
}

impl<'a> TreeItem for NodeWrapper<'a> {
    type Child = Self;

    fn write_self<W: Write>(&self, f: &mut W, style: &Style) -> io::Result<()> {
        write!(
            f,
            "{}",
            style.paint(format!(
                "({}[{}]{}..{})",
                self.node.kind(),
                self.index,
                self.node.byte_range().start,
                self.node.byte_range().end
            ))
        )
    }

    fn children(&self) -> Cow<'_, [Self::Child]> {
        Cow::Borrowed(&self.children)
    }
}

fn build_tree<'a>(node: Node<'a>, next_index: &mut usize) -> NodeWrapper<'a> {
    let index = *next_index;
    *next_index += 1;

    let mut children = Vec::new();
    let mut cursor = node.walk();

    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            children.push(build_tree(child, next_index));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    NodeWrapper {
        node,
        index,
        children,
    }
}

impl PrettyPrint for Node<'_> {
    fn pretty_print(&self) -> String {
        let mut next_index = 0;
        let wrapper = build_tree(*self, &mut next_index);
        let mut output = Vec::new();

        ptree::write_tree(&wrapper, &mut output).expect("Failed to write tree");
        String::from_utf8(output).expect("Failed to decode tree")
    }
}
