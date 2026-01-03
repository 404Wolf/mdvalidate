use ptree::{Style, TreeItem};
use std::borrow::Cow;
use std::io::{self, Write};
use tree_sitter::{Node, TreeCursor};

#[allow(dead_code)]
pub trait PrettyPrint {
    fn pretty_print(&self) -> String;
}

#[allow(dead_code)]
#[derive(Clone)]
struct NodeWrapper<'a> {
    cursor: TreeCursor<'a>,
}

impl<'a> TreeItem for NodeWrapper<'a> {
    type Child = Self;

    fn write_self<W: Write>(&self, f: &mut W, style: &Style) -> io::Result<()> {
        let node = self.cursor.node();

        write!(
            f,
            "{}",
            style.paint(format!(
                "({}[{}]{}..{})",
                node.kind(),
                self.cursor.descendant_index(),
                node.byte_range().start,
                node.byte_range().end
            ))
        )
    }

    fn children(&self) -> Cow<'_, [Self::Child]> {
        let mut cursor = self.cursor.clone();
        let mut children = Vec::new();

        // Try to go to the first child
        if cursor.goto_first_child() {
            // Add the first child
            children.push(NodeWrapper {
                cursor: cursor.clone(),
            });

            // Iterate through siblings using goto_next_sibling
            while cursor.goto_next_sibling() {
                children.push(NodeWrapper {
                    cursor: cursor.clone(),
                });
            }
        }

        Cow::from(children)
    }
}

impl PrettyPrint for Node<'_> {
    fn pretty_print(&self) -> String {
        let wrapper = NodeWrapper {
            cursor: self.walk(),
        };
        let mut output = Vec::new();

        // Use ptree to write the tree structure
        ptree::write_tree(&wrapper, &mut output).unwrap();

        String::from_utf8(output).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::ts_utils::parse_markdown;

    use super::*;

    #[test]
    fn test_pretty_print() {
        let tree = parse_markdown("# Test").unwrap();
        let node = tree.root_node();
        let expected = "(document[0])
└─ (atx_heading[1])
   ├─ (atx_h1_marker[2])
   └─ (heading_content[3])
      └─ (text[4])
";
        assert_eq!(node.pretty_print(), expected);
    }
}
