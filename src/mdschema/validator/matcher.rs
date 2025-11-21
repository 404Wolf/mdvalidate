use core::fmt;
use regex::Regex;
use std::{collections::HashSet, sync::LazyLock};

use super::errors::ValidationError;

static MATCHER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(((?P<id>[a-zA-Z0-9-_]+)):)?(\/(?P<regex>.+?)\/|(?P<special>ruler))").unwrap()
});

pub static SPECIAL_CHARS_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[+\{\},0-9]*").unwrap());

pub fn get_everything_after_special_chars(text: &str) -> &str {
    let captures = SPECIAL_CHARS_START.captures(text);
    match captures {
        Some(caps) => {
            let mat = caps.get(0).unwrap();
            &text[mat.end()..]
        }
        None => text,
    }
}

pub struct Matcher {
    id: Option<String>,
    /// A compiled regex for the pattern.
    pattern: MatcherType,
    /// Extra flags, which we receive via extra text that corresponds to the matcher
    extras: HashSet<MatcherExtras>,
    /// Optional maximum recursion depth for nested lists (parsed from +N after the plus)
    max_depth: Option<usize>,
    /// Optional minimum number of list items at this level
    min_items: Option<usize>,
    /// Optional maximum number of list items at this level
    max_items: Option<usize>,
}

#[derive(Debug, Clone)]
enum MatcherType {
    Regex(Regex),
    Special(SpecialMatchers),
}

#[derive(Debug, Clone)]
enum SpecialMatchers {
    Ruler,
}

/// Special matcher types that extend the meaning of a group.
///
/// This is the text that comes directly after the matcher codeblock.  For
/// example, '+' indicates that the matcher is repeated (and allows many list
/// items).
///
/// Make sure to update SPECIAL_CHARS_START when adding new extras.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MatcherExtras {
    Repeated,
}

impl Matcher {
    /// Create a new Matcher given the text in a matcher codeblock and the text node's contents
    /// immediately proceeding the matcher.
    pub fn new(pattern: &str, extras: Option<&str>) -> Result<Matcher, ValidationError> {
        let pattern = pattern[1..pattern.len() - 1].trim(); // Remove surrounding backticks
        let captures = MATCHER_PATTERN.captures(pattern);

        let (id, matcher) = match captures {
            Some(caps) => Self::extract_id_and_pattern(&caps, &pattern)?,
            None => {
                return Err(ValidationError::InvalidMatcherFormat(format!(
                    "Expected format: 'id:/regex/', got {}",
                    pattern
                )));
            }
        };

        let (extras_set, max_depth) = match extras {
            Some(text) => Self::extract_matcher_extras(text),
            None => (HashSet::new(), None),
        };

        let (min_items, max_items) = match extras {
            Some(text) => Self::extract_item_count_limits(text),
            None => (None, None),
        };

        Ok(Matcher {
            id,
            extras: extras_set,
            pattern: matcher,
            max_depth,
            min_items,
            max_items,
        })
    }

    /// Extract any extra flags from the text following the matcher.
    /// Extract any extra flags and optional max_depth from the text following the matcher.
    /// Returns (extras_set, optional_max_depth)
    /// The max_depth is determined by the number of '+' characters (e.g., ++ = depth 2)
    fn extract_matcher_extras(text: &str) -> (HashSet<MatcherExtras>, Option<usize>) {
        let set_of_chars = text.chars().collect::<HashSet<char>>();
        let mut extras = HashSet::new();

        // Count the number of '+' characters for both repeated flag and max depth
        let plus_count = text.chars().filter(|&c| c == '+').count();

        for char in set_of_chars {
            match char {
                '+' => {
                    extras.insert(MatcherExtras::Repeated);
                }
                _ => {}
            }
        }

        // The max depth is the number of plus signs (if any)
        let max_depth = if plus_count > 0 {
            Some(plus_count)
        } else {
            None
        };

        (extras, max_depth)
    }

    /// Extract item count limits from {min,max} syntax in the text following the matcher.
    /// Returns (min_items, max_items) where either can be None.
    /// Examples: {2,5} = min 2, max 5; {3,} = min 3, no max; {,10} = no min, max 10
    fn extract_item_count_limits(text: &str) -> (Option<usize>, Option<usize>) {
        // Look for {min,max} pattern
        let re = Regex::new(r"\{(\d*),(\d*)\}").unwrap();

        if let Some(caps) = re.captures(text) {
            let min = caps.get(1).and_then(|m| {
                if m.as_str().is_empty() {
                    None
                } else {
                    m.as_str().parse::<usize>().ok()
                }
            });
            let max = caps.get(2).and_then(|m| {
                if m.as_str().is_empty() {
                    None
                } else {
                    m.as_str().parse::<usize>().ok()
                }
            });
            (min, max)
        } else {
            (None, None)
        }
    }

    /// Extract the ID and pattern from the regex captures.
    fn extract_id_and_pattern(
        captures: &regex::Captures,
        pattern: &str,
    ) -> Result<(Option<String>, MatcherType), ValidationError> {
        let id = captures.name("id").map(|m| m.as_str().to_string());
        let regex_pattern = captures.name("regex").map(|m| m.as_str().to_string());
        let special = captures.name("special").map(|m| m.as_str().to_string());

        let matcher = match (regex_pattern, special) {
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
        Ok((id, matcher))
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

    /// Whether the matcher is for a ruler.
    pub fn is_ruler(&self) -> bool {
        matches!(self.pattern, MatcherType::Special(SpecialMatchers::Ruler))
    }

    /// Whether the matcher repeats.
    pub fn is_repeated(&self) -> bool {
        self.extras.contains(&MatcherExtras::Repeated)
    }

    pub fn id(&self) -> Option<&String> {
        self.id.as_ref()
    }

    /// Return optional maximum depth for nested lists
    pub fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }

    /// Return optional minimum number of items at this list level
    pub fn min_items(&self) -> Option<usize> {
        self.min_items
    }

    /// Return optional maximum number of items at this list level
    pub fn max_items(&self) -> Option<usize> {
        self.max_items
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
        let matcher = Matcher::new("`word:/\\w+/`", None).unwrap();
        assert_eq!(matcher.id, Some("word".to_string()));
        assert_eq!(matcher.match_str("hello world"), Some("hello"));
        assert_eq!(matcher.match_str("1234"), Some("1234"));
        assert_eq!(matcher.match_str("!@#$"), None);
    }

    #[test]
    fn test_matcher_invalid_pattern() {
        let result = Matcher::new("`invalid_pattern`", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_matcher_display() {
        let matcher = Matcher::new("`num:/\\d+/`", None).unwrap();
        let display_str = format!("{}", matcher);
        assert_eq!(display_str, "num:/\\d+/");
    }

    #[test]
    fn test_long_complicated_id_and_regex() {
        let matcher =
            Matcher::new("`complicatedID_123-abc:/[a-zA-Z0-9_\\-]{5,15}/`", None).unwrap();
        assert_eq!(matcher.id, Some("complicatedID_123-abc".to_string()));
        assert_eq!(
            matcher.match_str("user_12345 is logged in"),
            Some("user_12345")
        );
        assert_eq!(matcher.match_str("tiny"), None); // Only 4 chars, should not match
    }

    #[test]
    fn test_with_no_id() {
        let matcher = Matcher::new("`ruler`", None).unwrap();
        assert_eq!(matcher.id, None);
        assert_eq!(matcher.match_str("ruler"), Some("ruler"));
        assert_eq!(matcher.match_str("***"), Some("ruler"));
        assert_eq!(matcher.match_str("!@#$"), None);
        assert_eq!(matcher.match_str("whatever"), None);

        let matcher = Matcher::new("'id:ruler'", None).unwrap();
        assert_eq!(matcher.id, Some("id".to_string()));
        assert_eq!(matcher.match_str("ruler"), Some("ruler"));
        assert_eq!(matcher.match_str("!@#$"), None);
        assert_eq!(matcher.match_str("whatever"), None);
    }

    #[test]
    fn test_item_count_limits() {
        // Test {min,max} parsing
        let matcher = Matcher::new("`test:/\\d+/`", Some("{2,5}++")).unwrap();
        assert_eq!(matcher.min_items(), Some(2));
        assert_eq!(matcher.max_items(), Some(5));
        assert_eq!(matcher.max_depth(), Some(2));
        assert!(matcher.is_repeated());

        // Test {min,} (no max)
        let matcher = Matcher::new("`test:/\\d+/`", Some("{3,}+")).unwrap();
        assert_eq!(matcher.min_items(), Some(3));
        assert_eq!(matcher.max_items(), None);
        assert_eq!(matcher.max_depth(), Some(1));

        // Test {,max} (no min)
        let matcher = Matcher::new("`test:/\\d+/`", Some("{,10}+++")).unwrap();
        assert_eq!(matcher.min_items(), None);
        assert_eq!(matcher.max_items(), Some(10));
        assert_eq!(matcher.max_depth(), Some(3));

        // Test with + before {}
        let matcher = Matcher::new("`test:/\\d+/`", Some("++{1,3}")).unwrap();
        assert_eq!(matcher.min_items(), Some(1));
        assert_eq!(matcher.max_items(), Some(3));
        assert_eq!(matcher.max_depth(), Some(2));

        // Test without {} - should have no limits
        let matcher = Matcher::new("`test:/\\d+/`", Some("+")).unwrap();
        assert_eq!(matcher.min_items(), None);
        assert_eq!(matcher.max_items(), None);
        assert_eq!(matcher.max_depth(), Some(1));
    }
}
