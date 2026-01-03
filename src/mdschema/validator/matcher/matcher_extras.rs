#![allow(dead_code)]

use regex::Regex;
use std::sync::LazyLock;

static RANGE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{(\d*),(\d*)\}").unwrap());

pub static MATCHERS_EXTRA_PATTERN: LazyLock<Regex> =
    // We can have a ! instead of matcher extras to indicate that it is a literal match
    LazyLock::new(|| Regex::new(r#"^((\!)|([+\{\},0-9]+))"#).unwrap());

pub fn partition_at_special_chars(text: &str) -> Option<(&str, &str)> {
    let captures = MATCHERS_EXTRA_PATTERN.captures(text);
    match captures {
        Some(caps) => {
            let mat = caps.get(0)?;
            Some((&text[..mat.end()], &text[mat.end()..]))
        }
        None => Some(("", text)),
    }
}

pub fn get_everything_after_extras(text: &str) -> Result<&str, MatcherExtrasError> {
    let (_extras, after) =
        partition_at_special_chars(text).ok_or(MatcherExtrasError::MatcherExtrasInvalid)?;

    if has_literal_within_extras(text) {
        return Err(MatcherExtrasError::MixedLiteralAndOthers);
    }

    Ok(after)
}

pub fn get_all_extras(text: &str) -> Result<&str, MatcherExtrasError> {
    let (extras, _after) =
        partition_at_special_chars(text).ok_or(MatcherExtrasError::MatcherExtrasInvalid)?;

    if has_literal_within_extras(text) {
        return Err(MatcherExtrasError::MixedLiteralAndOthers);
    }

    Ok(extras)
}

/// Our regular regex for extras will look at your extras, and if it starts with
/// "!" ignore the rest of it and pretend that the only extras
///
/// If the string starts with a !, then try running get_everything_after_extras
pub fn has_literal_within_extras(text: &str) -> bool {
    text.starts_with('!') && text.len() != 1 && get_everything_after_extras(&text[1..]).is_ok()
}

/// Errors specific to matcher extras construction
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum MatcherExtrasError {
    /// The extras that came after the matcher were impossible and contained wrong or invalid patterns.
    ///
    /// We get this if we see something like `name:/test/`$%^&*.
    MatcherExtrasInvalid,
    /// When we have a literal extra, and any other extras. If we are literal we
    /// can *only* be literal.
    MixedLiteralAndOthers,
}

impl std::fmt::Display for MatcherExtrasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatcherExtrasError::MatcherExtrasInvalid => {
                write!(f, "Invalid matcher extras")
            }
            MatcherExtrasError::MixedLiteralAndOthers => {
                write!(f, "Cannot mix literal extras with other extras")
            }
        }
    }
}

/// Features of the given matcher, like max count if it is repeated.
///
/// This struct holds configuration options parsed from the suffix text that appears
/// immediately after a matcher code block in the schema.
///
/// # Item Count Limits
/// The `{min,max}` syntax specifies matcher repetition:
/// - `{2,5}` - min 2, max 5 items
/// - `{3,}` - min 3, no max
/// - `{,10}` - no min, max 10
/// - `{,}` - unbounded but repeatable
///
/// # Literal Code Flag
/// The `!` character indicates that matched content should be treated as literal
/// code blocks in the output, preserving formatting and syntax.
///
/// # Examples
/// ```ignore
/// // Matcher with repeat limits: `name:/\w+/`{2,5}
/// let extras = MatcherExtras::new(Some("{2,5}"));
/// assert_eq!(extras.min_items(), Some(2));
/// assert_eq!(extras.max_items(), Some(5));
///
/// // Matcher with literal code flag: `code:/\w+/`!
/// let extras = MatcherExtras::new(Some("!"));
/// // is_literal_code will be true
/// ```
#[derive(Debug, Clone)]
pub struct MatcherExtras {
    /// Optional minimum number of list items at this level
    min_items: Option<usize>,
    /// Optional maximum number of list items at this level
    max_items: Option<usize>,
    /// Whether min/max constraints were specified
    had_min_max: bool,
    /// Whether it is a literal code block
    is_literal_code: bool,
}

impl MatcherExtras {
    /// Create new MatcherExtras by parsing the text following a matcher.
    ///
    /// # Arguments
    /// * `text` - Optional text following the matcher code block
    pub fn try_new(text: Option<&str>) -> Result<Self, MatcherExtrasError> {
        // Check if text matches the pattern, if text is provided
        if let Some(text) = text {
            if !MATCHERS_EXTRA_PATTERN.is_match(text) {
                return Err(MatcherExtrasError::MatcherExtrasInvalid);
            }
        }

        Ok(match text {
            Some(text) => {
                // TODO: optimization. We could not even bother calling `extract_item_count_limits` if it's literal.
                let is_literal = text.starts_with('!');

                let (min_items, max_items, had_range_syntax) = extract_item_count_limits(text);

                Self {
                    min_items,
                    max_items,
                    had_min_max: had_range_syntax,
                    is_literal_code: is_literal, // We handle literal code at a higher level now
                }
            }
            None => Self {
                min_items: None,
                max_items: None,
                had_min_max: false,
                is_literal_code: false,
            },
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_matcher_with_bullshit_extras() {
        let matches = MATCHERS_EXTRA_PATTERN.is_match("bullshit");
        assert!(!matches);

        let result = MatcherExtras::try_new(Some("bullshit"));
        assert!(result.is_err());
    }

    #[test]
    fn test_get_all_extras_repeating() {
        let result = get_all_extras("{1,} test");
        assert_eq!(result.unwrap(), "{1,}");
    }

    #[test]
    fn test_get_all_extras_with_literal_too() {
        let result = get_all_extras("!{1,} test");
        match result.unwrap_err() {
            MatcherExtrasError::MixedLiteralAndOthers => (),
            error => panic!("expected MixedLiteralAndOthers, got {:?}", error),
        }
    }

    #[test]
    fn test_get_all_extras_no_extras() {
        let result = get_all_extras("");
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_get_all_extras_just_literal() {
        let result = get_all_extras("!");
        assert_eq!(result.unwrap(), "!");
    }

    #[test]
    fn test_empty_range_is_valid() {
        // Test that {,} is valid (empty range with no values)
        let result = MatcherExtras::try_new(Some("{,}"));
        assert!(result.is_ok());

        let extras = result.unwrap();
        assert!(extras.had_min_max());
        assert_eq!(extras.min_items(), None);
        assert_eq!(extras.max_items(), None);
    }

    #[test]
    fn test_item_count_limits() {
        // Test {min,max} parsing
        let extras = MatcherExtras::try_new(Some("{2,5}++")).unwrap();
        assert_eq!(extras.min_items(), Some(2));
        assert_eq!(extras.max_items(), Some(5));
        assert!(extras.had_min_max());

        // Test {min,} (no max)
        let extras = MatcherExtras::try_new(Some("{3,}+")).unwrap();
        assert_eq!(extras.min_items(), Some(3));
        assert_eq!(extras.max_items(), None);

        // Test {,max} (no min)
        let extras = MatcherExtras::try_new(Some("{,10}+++")).unwrap();
        assert_eq!(extras.min_items(), None);
        assert_eq!(extras.max_items(), Some(10));

        // Test with + before {}
        let extras = MatcherExtras::try_new(Some("++{1,3}")).unwrap();
        assert_eq!(extras.min_items(), Some(1));
        assert_eq!(extras.max_items(), Some(3));

        // Test without {} - should have no limits
        let extras = MatcherExtras::try_new(Some("+")).unwrap();
        assert_eq!(extras.min_items(), None);
        assert_eq!(extras.max_items(), None);
    }

    #[test]
    fn test_had_min_max() {
        // No extras text at all - should not have min/max
        let extras = MatcherExtras::try_new(None).unwrap();
        assert!(!extras.had_min_max());
        assert_eq!(extras.min_items(), None);
        assert_eq!(extras.max_items(), None);

        // Extras text without {,} syntax - should not have min/max
        let extras = MatcherExtras::try_new(Some("+")).unwrap();
        assert!(!extras.had_min_max());
        assert_eq!(extras.min_items(), None);
        assert_eq!(extras.max_items(), None);

        // Empty {,} syntax - should have min/max even though values are None
        let extras = MatcherExtras::try_new(Some("{,}")).unwrap();
        assert!(extras.had_min_max());
        assert_eq!(extras.min_items(), None);
        assert_eq!(extras.max_items(), None);

        // {min,max} with actual values
        let extras = MatcherExtras::try_new(Some("{2,5}")).unwrap();
        assert!(extras.had_min_max());
        assert_eq!(extras.min_items(), Some(2));
        assert_eq!(extras.max_items(), Some(5));

        // {min,} with only min
        let extras = MatcherExtras::try_new(Some("{3,}")).unwrap();
        assert!(extras.had_min_max());
        assert_eq!(extras.min_items(), Some(3));
        assert_eq!(extras.max_items(), None);

        // {,max} with only max
        let extras = MatcherExtras::try_new(Some("{,10}")).unwrap();
        assert!(extras.had_min_max());
        assert_eq!(extras.min_items(), None);
        assert_eq!(extras.max_items(), Some(10));

        // {,} with other text before/after
        let extras = MatcherExtras::try_new(Some("++{,}+")).unwrap();
        assert!(extras.had_min_max());
        assert_eq!(extras.min_items(), None);
        assert_eq!(extras.max_items(), None);
    }
}
