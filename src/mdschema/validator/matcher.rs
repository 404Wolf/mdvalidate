use core::fmt;
use regex::Regex;
use std::{collections::HashSet, sync::LazyLock};

use super::errors::ValidationError;

static MATCHER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(((?P<id>[a-zA-Z0-9-_]+)):)?(\/(?P<regex>.+?)\/|(?P<special>ruler))").unwrap()
});

static RANGE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{(\d*),(\d*)\}").unwrap()
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

/// Features of the given matcher, like max count if it is repeated
#[derive(Debug, Clone)]
pub struct MatcherExtras {
    /// Optional minimum number of list items at this level
    min_items: Option<usize>,
    /// Optional maximum number of list items at this level
    max_items: Option<usize>,
    /// Whether min/max constraints were specified
    had_min_max: bool,
}

impl MatcherExtras {
    /// Create new MatcherExtras by parsing the text following a matcher.
    /// Extract item count limits from {min,max} syntax in the text.
    /// Examples: {2,5} = min 2, max 5; {3,} = min 3, no max; {,10} = no min, max 10
    pub fn new(text: Option<&str>) -> Self {
        match text {
            Some(text) => {
                let (min_items, max_items, had_range_syntax) = Self::extract_item_count_limits(text);
                Self {
                    min_items,
                    max_items,
                    had_min_max: had_range_syntax,
                }
            }
            None => Self {
                min_items: None,
                max_items: None,
                had_min_max: false,
            },
        }
    }

    /// Extract item count limits from {min,max} syntax in the text following the matcher.
    /// Returns (min_items, max_items, had_range_syntax) where the first two can be None.
    /// had_range_syntax is true if the {min,max} pattern was found, even if both are empty.
    fn extract_item_count_limits(text: &str) -> (Option<usize>, Option<usize>, bool) {
        // Look for {min,max} pattern
        if let Some(caps) = RANGE_PATTERN.captures(text) {
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
            (min, max, true)
        } else {
            (None, None, false)
        }
    }

    /// Return optional minimum number of items at this list level
    pub fn min_items(&self) -> Option<usize> {
        self.min_items
    }

    /// Return optional maximum number of items at this list level
    pub fn max_items(&self) -> Option<usize> {
        self.max_items
    }

    /// Whether min/max constraints were specified
    pub fn had_min_max(&self) -> bool {
        self.had_min_max
    }
}

#[derive(Debug, Clone)]
pub struct Matcher {
    id: Option<String>,
    /// A compiled regex for the pattern.
    pattern: MatcherType,
    /// Extra flags, which we receive via extra text that corresponds to the matcher
    flags: HashSet<MatcherFlags>,
    /// Extra configuration options
    extras: MatcherExtras,
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
/// This is the text that comes directly after the matcher codeblock. For
/// example, '?' indicates that the matcher is optional.
///
/// Make sure to update SPECIAL_CHARS_START when adding new flags.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MatcherFlags {
    /// The {min,max} flag indicates that the matcher has a minimum and maximum number of items.
    MinMax,
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

        Ok(Matcher {
            id,
            flags: HashSet::new(),
            pattern: matcher,
            extras: MatcherExtras::new(extras),
        })
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
        self.extras().had_min_max()
    }

    /// The ID of the matcher. This is the key in the final JSON.
    pub fn id(&self) -> Option<&String> {
        self.id.as_ref()
    }

    /// Get a reference to the extras
    pub fn extras(&self) -> &MatcherExtras {
        &self.extras
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
        assert_eq!(matcher.extras().min_items(), Some(2));
        assert_eq!(matcher.extras().max_items(), Some(5));
        assert!(matcher.is_repeated());

        // Test {min,} (no max)
        let matcher = Matcher::new("`test:/\\d+/`", Some("{3,}+")).unwrap();
        assert_eq!(matcher.extras().min_items(), Some(3));
        assert_eq!(matcher.extras().max_items(), None);

        // Test {,max} (no min)
        let matcher = Matcher::new("`test:/\\d+/`", Some("{,10}+++")).unwrap();
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), Some(10));

        // Test with + before {}
        let matcher = Matcher::new("`test:/\\d+/`", Some("++{1,3}")).unwrap();
        assert_eq!(matcher.extras().min_items(), Some(1));
        assert_eq!(matcher.extras().max_items(), Some(3));

        // Test without {} - should have no limits
        let matcher = Matcher::new("`test:/\\d+/`", Some("+")).unwrap();
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), None);
    }

    #[test]
    fn test_had_min_max() {
        // No extras text at all - should not have min/max
        let matcher = Matcher::new("`foo:/bar/`", None).unwrap();
        assert!(!matcher.is_repeated());
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), None);

        // Extras text without {,} syntax - should not have min/max
        let matcher = Matcher::new("`foo:/bar/`", Some("+")).unwrap();
        assert!(!matcher.is_repeated());
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), None);

        // Empty {,} syntax - should have min/max even though values are None
        let matcher = Matcher::new("`foo:/bar/`", Some("{,}")).unwrap();
        assert!(matcher.is_repeated());
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), None);

        // {min,max} with actual values
        let matcher = Matcher::new("`foo:/bar/`", Some("{2,5}")).unwrap();
        assert!(matcher.is_repeated());
        assert_eq!(matcher.extras().min_items(), Some(2));
        assert_eq!(matcher.extras().max_items(), Some(5));

        // {min,} with only min
        let matcher = Matcher::new("`foo:/bar/`", Some("{3,}")).unwrap();
        assert!(matcher.is_repeated());
        assert_eq!(matcher.extras().min_items(), Some(3));
        assert_eq!(matcher.extras().max_items(), None);

        // {,max} with only max
        let matcher = Matcher::new("`foo:/bar/`", Some("{,10}")).unwrap();
        assert!(matcher.is_repeated());
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), Some(10));

        // {,} with other text before/after
        let matcher = Matcher::new("`foo:/bar/`", Some("++{,}+")).unwrap();
        assert!(matcher.extras().had_min_max());
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), None);
    }
}
