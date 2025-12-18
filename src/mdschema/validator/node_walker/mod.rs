use serde_json::{json, Value};
use tree_sitter::TreeCursor;

use crate::mdschema::validator::errors::Error;

pub mod node_walker;

pub use node_walker::NodeWalker;
pub use validation_result::ValidationResult;

mod matcher_vs_list;
mod matcher_vs_text;
mod node_vs_node;
mod text_vs_text;
mod utils;
mod validation_result;
