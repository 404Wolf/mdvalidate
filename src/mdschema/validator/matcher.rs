use core::fmt;
use regex::Regex;
use std::sync::LazyLock;

use super::errors::ValidationError;

static MATCHER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<id>[a-zA-Z0-9-_]+):/(?P<regex>.+?)/$").unwrap());

pub struct Matcher {
    id: String,
    /// A compiled regex for the pattern.
    regex: Regex,
}

impl Matcher {
    pub fn new(pattern: &str) -> Result<Matcher, ValidationError> {
        let pattern = pattern[1..pattern.len() - 1].trim(); // Remove surrounding backticks
        let captures = MATCHER_PATTERN.captures(pattern);

        let (id, regex_pattern) = match captures {
            Some(caps) => {
                let id = caps.name("id").map(|m| m.as_str()).unwrap_or("default");
                let regex_pattern = caps.name("regex").map(|m| m.as_str()).unwrap_or(pattern);
                (id.to_string(), regex_pattern)
            }
            None => {
                return Err(ValidationError::InvalidMatcherFormat(format!(
                    "Expected format: 'id:/regex/', got {}",
                    pattern
                )));
            }
        };

        Ok(Matcher {
            id,
            regex: Regex::new(&format!("^{}", regex_pattern))?,
        })
    }

    /// Get an actual match string for a given text, if it matches.
    pub fn match_str<'a>(&self, text: &'a str) -> Option<&'a str> {
        if let Some(mat) = self.regex.find(text) {
            Some(&text[mat.start()..mat.end()])
        } else {
            None
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for Matcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let regex_str = self.regex.as_str();
        // The regex is stored as "^<pattern>", so remove the leading ^
        let regex_pattern = if regex_str.starts_with('^') {
            &regex_str[1..]
        } else {
            regex_str
        };

        write!(f, "#{}:/{}/", self.id, regex_pattern)
    }
}

mod tests {
    #[cfg(test)]
    use crate::mdschema::validator::matcher::Matcher;

    #[test]
    fn test_matcher_creation_and_matching() {
        let matcher = Matcher::new("`word:/\\w+/`").unwrap();
        assert_eq!(matcher.id, "word");
        assert_eq!(matcher.match_str("hello world"), Some("hello"));
        assert_eq!(matcher.match_str("1234"), Some("1234"));
        assert_eq!(matcher.match_str("!@#$"), None);
    }

    #[test]
    fn test_matcher_invalid_pattern() {
        let result = Matcher::new("`invalid_pattern`");
        assert!(result.is_err());
    }

    #[test]
    fn test_matcher_display() {
        let matcher = Matcher::new("`num:/\\d+/`").unwrap();
        let display_str = format!("{}", matcher);
        assert_eq!(display_str, "#num:/\\d+/");
    }

    #[test]
    fn test_long_complicated_id_and_regex() {
        let matcher = Matcher::new("`complicatedID_123-abc:/[a-zA-Z0-9_\\-]{5,15}/`").unwrap();
        assert_eq!(matcher.id, "complicatedID_123-abc");
        assert_eq!(
            matcher.match_str("user_12345 is logged in"),
            Some("user_12345")
        );
        assert_eq!(matcher.match_str("tiny"), None); // Only 4 chars, should not match
    }
}
