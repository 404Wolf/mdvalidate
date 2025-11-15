use core::fmt;
use regex::Regex;
use std::sync::LazyLock;

use super::errors::ValidationError;

static MATCHER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(((?P<id>[a-zA-Z0-9-_]+)):)?(\/(?P<regex>.+?)\/|(?P<special>ruler))").unwrap()
});

pub struct Matcher {
    id: Option<String>,
    /// A compiled regex for the pattern.
    pattern: MatcherType,
}

enum MatcherType {
    Regex(Regex),
    Special(SpecialMatchers),
}

enum SpecialMatchers {
    Ruler,
}

impl Matcher {
    pub fn new(pattern: &str) -> Result<Matcher, ValidationError> {
        let pattern = pattern[1..pattern.len() - 1].trim(); // Remove surrounding backticks
        let captures = MATCHER_PATTERN.captures(pattern);

        let (id, matcher) = match captures {
            Some(caps) => {
                println!("caps: {:?}", caps.name("id"));
                let id = caps.name("id").map(|m| m.as_str().to_string());
                let regex_pattern = caps.name("regex").map(|m| m.as_str().to_string());
                let special = caps.name("special").map(|m| m.as_str().to_string());

                let pattern = match (regex_pattern, special) {
                    (Some(regex_pattern), None) => {
                        MatcherType::Regex(Regex::new(&format!("^{}", regex_pattern)).unwrap())
                    }
                    (None, Some(_)) => MatcherType::Special(SpecialMatchers::Ruler),
                    (Some(_), Some(_)) => {
                        return Err(ValidationError::InvalidMatcherFormat(format!(
                            "Matcher cannot be both regex and special type: {}",
                            pattern
                        )))
                    }
                    (None, None) => {
                        return Err(ValidationError::InvalidMatcherFormat(format!(
                            "Matcher must be either regex or special type: {}",
                            pattern
                        )))
                    }
                };
                println!(
                    "pattern is of type: {}",
                    std::any::type_name_of_val(&pattern)
                );
                (id, pattern)
            }
            None => {
                return Err(ValidationError::InvalidMatcherFormat(format!(
                    "Expected format: 'id:/regex/', got {}",
                    pattern
                )));
            }
        };

        println!("id: {:?}", id,);
        Ok(Matcher {
            id,
            pattern: matcher,
        })
    }

    /// Get an actual match string for a given text, if it matches.
    pub fn match_str<'a>(&self, text: &'a str) -> Option<&'a str> {
        match &self.pattern {
            MatcherType::Regex(regex) => match regex.find(text) {
                Some(mat) => Some(&text[mat.start()..mat.end()]),
                None => None,
            },
            MatcherType::Special(SpecialMatchers::Ruler) => {
                if text == "ruler" || text.matches(['-', '*', '_']).count() >= 3 {
                    Some("ruler")
                } else {
                    None
                }
            }
        }
    }

    /// Check whether the matcher is for a ruler.
    pub fn is_ruler(&self) -> bool {
        matches!(self.pattern, MatcherType::Special(SpecialMatchers::Ruler))
    }

    pub fn id(&self) -> Option<&String> {
        self.id.as_ref()
    }
}

impl fmt::Display for Matcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pattern_str = match &self.pattern {
            MatcherType::Regex(regex) => {
                let regex_str = regex.as_str();
                // The regex is stored as "^<pattern>", so remove the leading ^
                if regex_str.starts_with('^') {
                    &regex_str[1..]
                } else {
                    regex_str
                }
            }
            MatcherType::Special(SpecialMatchers::Ruler) => "ruler",
        };

        match &self.id {
            Some(id) => write!(f, "{}:/{}/", id, pattern_str),
            None => write!(f, "/{}/", pattern_str),
        }
    }
}

mod tests {
    #[cfg(test)]
    use crate::mdschema::validator::matcher::Matcher;

    #[test]
    fn test_matcher_creation_and_matching() {
        let matcher = Matcher::new("`word:/\\w+/`").unwrap();
        assert_eq!(matcher.id, Some("word".to_string()));
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
        assert_eq!(display_str, "num:/\\d+/");
    }

    #[test]
    fn test_long_complicated_id_and_regex() {
        let matcher = Matcher::new("`complicatedID_123-abc:/[a-zA-Z0-9_\\-]{5,15}/`").unwrap();
        assert_eq!(matcher.id, Some("complicatedID_123-abc".to_string()));
        assert_eq!(
            matcher.match_str("user_12345 is logged in"),
            Some("user_12345")
        );
        assert_eq!(matcher.match_str("tiny"), None); // Only 4 chars, should not match
    }

    #[test]
    fn test_with_no_id() {
        let matcher = Matcher::new("`ruler`").unwrap();
        assert_eq!(matcher.id, None);
        assert_eq!(matcher.match_str("ruler"), Some("ruler"));
        assert_eq!(matcher.match_str("***"), Some("ruler"));
        assert_eq!(matcher.match_str("!@#$"), None);
        assert_eq!(matcher.match_str("whatever"), None);

        let matcher = Matcher::new("'id:ruler'").unwrap();
        assert_eq!(matcher.id, Some("id".to_string()));
        assert_eq!(matcher.match_str("ruler"), Some("ruler"));
        assert_eq!(matcher.match_str("!@#$"), None);
        assert_eq!(matcher.match_str("whatever"), None);
    }
}
