use serde_json::Value;

use crate::mdschema::validator::errors::Error;

pub mod node_walker;

pub use node_walker::NodeWalker;

mod matcher_vs_list;
mod matcher_vs_text;
mod node_vs_node;
mod text_vs_text;
mod utils;

/// Type alias for validation results containing a Value and iterator of errors
pub type ValidationResult = (Value, Vec<Error>);
