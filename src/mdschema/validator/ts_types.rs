#![allow(dead_code)]

use tree_sitter::Node;

/// Macro to generate `is_*_node` and `both_are_*` helpers for node kinds.
macro_rules! node_kind_pair {
    ($is_fn:ident, $both_fn:ident, $doc:expr, [$($kind:expr),+ $(,)?]) => {
        pub fn $is_fn(node: &Node) -> bool {
            matches!(node.kind(), $($kind)|+)
        }

        #[doc = $doc]
        pub fn $both_fn(schema_node: &Node, input_node: &Node) -> bool {
            $is_fn(schema_node) && $is_fn(input_node)
        }
    };
}

/// Macro to generate `is_*_node` and `both_are_*` helpers for predicates.
macro_rules! node_predicate_pair {
    ($is_fn:ident, $both_fn:ident, $doc:expr, $predicate:expr) => {
        pub fn $is_fn(node: &Node) -> bool {
            $predicate(node)
        }

        #[doc = $doc]
        pub fn $both_fn(schema_node: &Node, input_node: &Node) -> bool {
            $is_fn(schema_node) && $is_fn(input_node)
        }
    };
}

node_kind_pair!(
    is_text_node,
    both_are_text_nodes,
    "Check if both nodes are text nodes.",
    ["text"]
);
node_kind_pair!(
    is_inline_code_node,
    both_are_inline_code,
    "Check if both nodes are inline code nodes.",
    ["code_span"]
);
node_kind_pair!(
    is_block_code_node,
    both_are_block_code_nodes,
    "Check if both nodes are block code nodes.",
    ["fenced_code_block"]
);
node_kind_pair!(
    is_codeblock_node,
    both_are_codeblocks,
    "Check if both nodes are codeblock nodes.",
    ["fenced_code_block"]
);
node_kind_pair!(
    is_link_node,
    both_are_link_nodes,
    "Check if both nodes are link nodes.",
    ["link"]
);
node_kind_pair!(
    is_link_destination_node,
    both_are_link_destination_nodes,
    "Check if both nodes are link destination nodes.",
    ["link_destination"]
);
node_kind_pair!(
    is_link_text_node,
    both_are_link_text_nodes,
    "Check if both nodes are link text nodes.",
    ["link_text"]
);
node_kind_pair!(
    is_image_node,
    both_are_image_nodes,
    "Check if both nodes are image nodes.",
    ["image"]
);
node_kind_pair!(
    is_image_description_node,
    both_are_image_description_nodes,
    "Check if both nodes are image description nodes.",
    ["image_description"]
);
node_kind_pair!(
    is_link_description_node,
    both_are_link_description_nodes,
    "Check if both nodes are link description nodes.",
    ["link_text", "image_description"]
);
node_kind_pair!(
    is_paragraph_node,
    both_are_paragraphs,
    "Check if both nodes are paragraph nodes.",
    ["paragraph"]
);
node_kind_pair!(
    is_heading_content_node,
    both_are_heading_contents,
    "Check if both nodes are heading content nodes.",
    ["heading_content"]
);
node_kind_pair!(
    is_list_marker_node,
    both_are_list_markers,
    "Check if both nodes are list marker nodes.",
    ["list_marker"]
);
node_kind_pair!(
    is_list_item_node,
    both_are_list_items,
    "Check if both nodes are list item nodes.",
    ["list_item"]
);
node_kind_pair!(
    is_heading_node,
    both_are_headings,
    "Check if both nodes are headings.",
    ["atx_heading"]
);
node_kind_pair!(
    is_ruler_node,
    both_are_rulers,
    "Check if both nodes are rulers.",
    ["thematic_break"]
);
node_kind_pair!(
    is_list_node,
    both_are_list_nodes,
    "Check if both nodes are list nodes.",
    ["tight_list", "loose_list"]
);
node_kind_pair!(
    is_textual_node,
    both_are_textual_nodes,
    "Check if both nodes are textual nodes.",
    [
        "text",
        "emphasis",
        "strong_emphasis",
        "code_span",
        "list_item"
    ]
);
node_kind_pair!(
    is_textual_container_node,
    both_are_textual_containers,
    "Check if both nodes are textual containers.",
    ["paragraph", "heading_content", "list_item"]
);
node_predicate_pair!(
    is_marker_node,
    both_are_markers,
    "Check if both nodes are marker nodes.",
    |node: &Node| node.kind().ends_with("_marker")
);

/// Check if both nodes are top-level nodes (document or heading).
pub fn both_are_matching_top_level_nodes(schema_node: &Node, input_node: &Node) -> bool {
    if schema_node.kind() != input_node.kind() {
        return false;
    }

    match schema_node.kind() {
        "document" => true,
        "atx_heading" => true,
        _ => false,
    }
}
