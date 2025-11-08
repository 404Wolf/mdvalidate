use anyhow::Result;
use core::fmt;
use log::debug;
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

use crate::mdschema::reports::errors::{Error, SchemaViolationError};

static MATCHER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<id>[a-zA-Z]+):/(?P<regex>.+?)/$").unwrap());

pub struct Matcher {
    id: String,
    /// A compiled regex for the pattern.
    regex: Regex,
}

impl Matcher {
    pub fn new(pattern: &str) -> Result<Matcher> {
        debug!("Parsing matcher pattern: {}", pattern);

        let pattern = pattern[1..pattern.len() - 1].trim(); // Remove surrounding backticks
        let captures = MATCHER_PATTERN.captures(pattern);

        let (id, regex_pattern) = match captures {
            Some(caps) => {
                let id = caps.name("id").map(|m| m.as_str()).unwrap_or("default");
                let regex_pattern = caps.name("regex").map(|m| m.as_str()).unwrap_or(pattern);
                (id.to_string(), regex_pattern)
            }
            None => {
                return Err(anyhow::anyhow!(
                    "Expected format: 'id:/regex/', got {}",
                    pattern
                ));
            }
        };

        debug!(
            "Creating matcher with id '{}' and regex pattern '{}'",
            id, regex_pattern
        );

        Ok(Matcher {
            id,
            regex: Regex::new(&format!("^{}$", regex_pattern).to_string())?,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Test if the given text matches the matcher pattern.
    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }

    /// Get an actual match string for a given text, if it matches.
    pub fn match_str<'a>(&self, text: &'a str) -> Option<&'a str> {
        if let Some(mat) = self.regex.find(text) {
            Some(&text[mat.start()..mat.end()])
        } else {
            None
        }
    }
}

impl fmt::Display for Matcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let regex_str = self.regex.as_str();
        let regex_str = format!("{}", &regex_str[1..regex_str.len() - 1]);

        write!(f, "#{}:/{}/", self.id, regex_str)
    }
}

/// Validate a matcher node against the input node.
///
/// A matcher node looks like `id:/pattern/` in the schema.
///
/// Pass the parent of the matcher node, and the corresponding input node.
pub fn validate_matcher_node<'b>(
    input_node: &Node<'b>,
    input_node_descendant_index: usize,
    schema_nodes: &[Node<'b>],
    input_str: &'b str,
    schema_str: &'b str,
) -> Vec<Error> {
    let mut errors = Vec::new();

    let code_nodes: Vec<_> = schema_nodes
        .iter()
        .filter(|n| n.kind() == "code_span")
        .collect();

    if code_nodes.len() > 1 {
        return vec![Error::SchemaViolation(
            SchemaViolationError::MultipleMatchers(code_nodes.len()),
        )];
    }

    let code_node = code_nodes[0];
    let matcher_text = &schema_str[code_node.byte_range()];

    let matcher = match Matcher::new(matcher_text) {
        Ok(m) => m,
        Err(_) => {
            return vec![Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    matcher_text.into(),
                ),
            )];
        }
    };

    let schema_start = schema_nodes[0].byte_range().start;
    let matcher_start = code_node.byte_range().start - schema_start;
    let matcher_end = code_node.byte_range().end - schema_start;

    // Validate prefix
    let prefix_schema = &schema_str[schema_start..schema_start + matcher_start];
    let prefix_input = &input_str
        [input_node.byte_range().start..input_node.byte_range().start + matcher_start];
    if prefix_schema != prefix_input {
        errors.push(Error::SchemaViolation(
            SchemaViolationError::NodeContentMismatch(
                input_node_descendant_index,
                prefix_schema.into(),
            ),
        ));
        return errors;
    }

    // Validate matcher against the portion of input that should match
    let input_start = input_node.byte_range().start + matcher_start;
    let input_to_match = &input_str[input_start..];

    match matcher.match_str(input_to_match) {
        Some(matched_str) => {
            // Validate suffix
            let suffix_schema =
                &schema_str[schema_start + matcher_end..schema_nodes[0].byte_range().end];
            let suffix_start = input_start + matched_str.len();
            let suffix_input = &input_str[suffix_start..input_node.byte_range().end];
            if suffix_schema != suffix_input {
                errors.push(Error::SchemaViolation(
                    SchemaViolationError::NodeContentMismatch(
                        input_node_descendant_index,
                        suffix_schema.into(),
                    ),
                ));
            }
        }
        None => {
            errors.push(Error::SchemaViolation(
                SchemaViolationError::NodeContentMismatch(
                    input_node_descendant_index,
                    matcher_text.into(),
                ),
            ));
        }
    };

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdschema::validator::utils::new_markdown_parser;

    #[test]
    fn test_validate_matcher_with_prefix_and_suffix() {
        let schema = "Hello `foo` world";
        let input = "Hello foo world";

        let mut input_parser = new_markdown_parser();
        let input_tree = input_parser.parse(input, None).unwrap();
        let input_node = input_tree.root_node().child(0).unwrap();

        let mut schema_parser = new_markdown_parser();
        let schema_tree = schema_parser.parse(schema, None).unwrap();
        let mut schema_cursor = schema_tree.walk();
        let schema_nodes: Vec<Node> = schema_tree
            .root_node()
            .child(0)
            .unwrap()
            .children(&mut schema_cursor)
            .collect();

        let errors = validate_matcher_node(
            &input_node,
            0, // placeholder
            &schema_nodes,
            input,
            schema,
        );

        assert!(
            errors.is_empty(),
            "Expected no errors but got: {:?}",
            errors
        );
    }
}
