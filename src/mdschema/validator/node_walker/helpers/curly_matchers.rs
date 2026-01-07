use regex::Regex;
use std::sync::LazyLock;

use crate::mdschema::validator::matcher::matcher::{Matcher, MatcherError};

static CURLY_MATCHER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\{(?P<inner>.+?)\}(?P<suffix>.*)?$").unwrap());

static CURLY_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\{(?P<id>\w+)\}$").unwrap());

pub fn extract_matcher_from_curly_delineated_text(
    input: &str,
) -> Option<Result<Matcher, MatcherError>> {
    let caps = CURLY_MATCHER.captures(input)?;

    let matcher_str = caps.name("inner").map(|m| m.as_str()).unwrap_or("").trim();
    let suffix = caps.name("suffix").map(|m| m.as_str());

    Some(Matcher::try_from_pattern_and_suffix_str(
        &format!("`{}`{}", matcher_str, suffix.unwrap_or("")),
        suffix,
    ))
}

/// Extract a simple ID from curly braces like `{id}` for code content capture.
pub fn extract_id_from_curly_braces(input: &str) -> Option<&str> {
    let caps = CURLY_ID.captures(input)?;
    caps.name("id").map(|m| m.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_id_from_curly_braces() {
        let input = "{test}";
        let result = extract_id_from_curly_braces(input).unwrap();
        assert_eq!(result, "test");

        let input = "";
        let result = extract_id_from_curly_braces(input);
        assert!(result.is_none());

        let input = "{a}{b}{c}";
        let result = extract_id_from_curly_braces(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_matcher_from_curly_delineated_text() {
        let input = "{id:/test/}{1,2}";
        let result = extract_matcher_from_curly_delineated_text(input)
            .unwrap()
            .unwrap();
        assert_eq!(result.id(), Some("id"));

        // Check that the pattern displays correctly.
        assert_eq!(format!("{}", result.pattern()), "^test");

        assert!(result.extras().had_min_max());
        assert_eq!(result.extras().min_items(), Some(1));
        assert_eq!(result.extras().max_items(), Some(2));
    }
}
