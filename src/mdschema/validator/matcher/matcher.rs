#![allow(dead_code)]

use core::fmt;
use regex::Regex;
use std::{collections::HashSet, sync::LazyLock};
use tree_sitter::TreeCursor;

use crate::mdschema::validator::ts_utils::get_node_and_next_node;

static MATCHER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(((?P<id>[a-zA-Z0-9-_]+)):)?(\/(?P<regex>.+?)\/|(?P<special>ruler))").unwrap()
});

static RANGE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{(\d*),(\d*)\}").unwrap());

pub static MATCHERS_EXTRA_PATTERN: LazyLock<Regex> =
    // We can have a ! instead of matcher extras to indicate that it is a literal match
    LazyLock::new(|| Regex::new(r"^(([+\{\},0-9]*)|\!|)$").unwrap());

pub fn get_everything_after_special_chars(text: &str) -> Option<&str> {
    let captures = MATCHERS_EXTRA_PATTERN.captures(text);
    match captures {
        Some(caps) => {
            let mat = caps.get(0)?;
            Some(&text[mat.end()..])
        }
        None => Some(text),
    }
}

/// Errors specific to matcher construction.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum MatcherError {
    /// The regex pattern for the interior of the matcher is invalid.
    MatcherInteriorRegexInvalid(String),
    /// We had an issue parsing the extras.
    MatcherExtrasError(MatcherExtrasError),
    /// We actually are dealing with literal code, not a matcher.
    ///
    /// We know this because we saw the `!` extra after the matcher.
    WasLiteralCode,
    /// You tried to use a constructor meant for nodes but failed to meet an
    /// invariant of the kind of node or state of the cursor used.
    InvariantViolation(String),
}

impl std::fmt::Display for MatcherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatcherError::MatcherInteriorRegexInvalid(err) => {
                write!(f, "Invalid matcher interior regex: {}", err)
            }
            MatcherError::MatcherExtrasError(err) => {
                write!(f, "Matcher extras error: {}", err)
            }
            MatcherError::WasLiteralCode => {
                write!(f, "Literal code")
            }
            MatcherError::InvariantViolation(err) => {
                write!(f, "Invariant violation: {}", err)
            }
        }
    }
}

/// Errors specific to matcher extras construction
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum MatcherExtrasError {
    /// The extras that came after the matcher were impossible and contained wrong or invalid patterns.
    ///
    /// We get this if we see something like `name:/test/`$%^&*.
    MatcherExtrasInvalid,
}

impl std::fmt::Display for MatcherExtrasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatcherExtrasError::MatcherExtrasInvalid => {
                write!(f, "Invalid matcher extras")
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
        // Split the text into before the first space and after the first space,
        // and only keep the part before
        let text = text.and_then(|s| s.split_once(' ').map(|(before, _)| before).or(Some(s)));

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

#[derive(Debug, Clone)]
pub struct Matcher {
    id: Option<String>,
    /// A compiled regex for the pattern.
    pattern: MatcherType,
    /// Extra flags, which we receive via extra text that corresponds to the matcher
    flags: HashSet<MatcherFlags>,
    /// Extra configuration options
    extras: MatcherExtras,
    /// The length of the matcher and its original extras
    original_str_len: usize,
}

#[derive(Debug, Clone)]
pub enum MatcherType {
    Regex(Regex),
    Special(SpecialMatchers),
}

impl fmt::Display for MatcherType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatcherType::Regex(regex) => write!(f, "{}", regex.as_str()),
            MatcherType::Special(special) => write!(f, "{}", special),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SpecialMatchers {
    Ruler,
}

impl fmt::Display for SpecialMatchers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecialMatchers::Ruler => write!(f, "Ruler"),
        }
    }
}

/// Special matcher types that extend the meaning of a group.
///
/// This is the text that comes directly after the matcher codeblock. For
/// example, '?' indicates that the matcher is optional.
///
/// Make sure to update SPECIAL_CHARS_START when adding new flags.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MatcherFlags {
    /// The {min,max} flag indicates that the matcher has a minimum and maximum number of items.
    MinMax,
}

impl Matcher {
    /// Create a new `Matcher` with all required fields.
    pub fn new(
        id: Option<String>,
        flags: HashSet<MatcherFlags>,
        pattern: MatcherType,
        extras: MatcherExtras,
        original_str_len: usize,
    ) -> Self {
        Matcher {
            id,
            flags,
            pattern,
            extras,
            original_str_len,
        }
    }

    pub fn new_with_empty_flags(
        id: Option<String>,
        pattern: MatcherType,
        extras: MatcherExtras,
        original_str_len: usize,
    ) -> Self {
        Self::new(id, HashSet::new(), pattern, extras, original_str_len)
    }

    /// Create a new Matcher given the text in a matcher codeblock and the text node's contents
    /// immediately proceeding the matcher.
    ///
    /// # Arguments
    /// * `pattern` - The pattern string within the matcher codeblock.
    /// * `extras` - Optional extras string following the pattern. This must
    ///   have a sequence of valid matcher extras, only followed by additional
    ///   text if there is a space in between.
    pub fn try_from_pattern_and_suffix_str(
        pattern_str: &str,
        extras_str: Option<&str>,
    ) -> Result<Matcher, MatcherError> {
        let pattern_str = pattern_str[1..pattern_str.len() - 1].trim(); // Remove surrounding backticks
        let captures = MATCHER_PATTERN.captures(pattern_str);

        let extras =
            MatcherExtras::try_new(extras_str).map_err(|e| MatcherError::MatcherExtrasError(e))?;

        // We are allowed to have an invalid matcher interior if it is literal
        // code, so throw this error before trying to create the matcher
        if extras.is_literal_code {
            return Err(MatcherError::WasLiteralCode);
        }

        let (id, pattern) = match captures {
            Some(caps) => extract_id_and_pattern(&caps, &pattern_str)?,
            None => {
                return Err(MatcherError::MatcherInteriorRegexInvalid(format!(
                    "Expected format: 'id:/regex/'<extras>, got {}", // TODO: don't hard code what we expect
                    pattern_str
                )));
            }
        };

        let original_str_len = pattern_str.len() + extras_str.map_or(0, |s| s.len());

        Ok(Self::new_with_empty_flags(
            id,
            pattern,
            extras,
            original_str_len,
        ))
    }

    /// Get an actual match string for a given text, if it matches.
    pub fn match_str<'a>(&self, text: &'a str) -> Option<&'a str> {
        match &self.pattern {
            MatcherType::Regex(regex) => match regex.find(text) {
                Some(mat) => Some(&text[mat.start()..mat.end()]),
                None => None,
            },
            MatcherType::Special(SpecialMatchers::Ruler) => None,
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
    pub fn id(&self) -> Option<&str> {
        self.id.as_ref().map(|s| s.as_str())
    }

    /// Get a reference to the extras
    pub fn extras(&self) -> &MatcherExtras {
        &self.extras
    }

    /// Get a reference to the pattern
    pub fn pattern(&self) -> &MatcherType {
        &self.pattern
    }

    /// The original string length of the matcher including the `s.
    pub fn original_str_len(&self) -> usize {
        self.original_str_len
    }

    /// Whether the matcher has variable length.
    ///
    /// The matcher has variable length if it does not have a specific max and min
    /// constraint, and the max is not equal to a min. In other words, it does not have a
    /// *specific* count. If we are not a repeating matcher it does not have
    /// variable length.
    pub fn variable_length(&self) -> bool {
        let extras = self.extras();
        match (extras.max_items(), extras.min_items()) {
            (Some(max_items), Some(min_items)) => max_items != min_items,
            _ if !extras.had_min_max() => false, // if we didn't have a min max, we are bounded since non-repeating
            _ => true,
        }
    }
}

impl PartialEq for Matcher {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && format!("{}", self.pattern) == format!("{}", other.pattern)
    }
}

/// Extract the ID and pattern from the regex captures.
fn extract_id_and_pattern(
    captures: &regex::Captures,
    pattern: &str,
) -> Result<(Option<String>, MatcherType), MatcherError> {
    let id = captures.name("id").map(|m| m.as_str().to_string());
    let regex_pattern = captures.name("regex").map(|m| m.as_str().to_string());
    let special = captures.name("special").map(|m| m.as_str().to_string());

    let matcher = match (regex_pattern, special) {
        (Some(regex_pattern), None) => {
            MatcherType::Regex(Regex::new(&format!("^{}", regex_pattern)).unwrap())
        }
        (None, Some(_)) => MatcherType::Special(SpecialMatchers::Ruler),
        (Some(_), Some(_)) => {
            return Err(MatcherError::MatcherInteriorRegexInvalid(format!(
                "Matcher cannot be both regex and special type: {}",
                pattern
            )));
        }
        (None, None) => {
            return Err(MatcherError::MatcherInteriorRegexInvalid(format!(
                "Matcher must be either regex or special type: {}",
                pattern
            )));
        }
    };
    Ok((id, matcher))
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

#[derive(Debug)]
pub enum ExtractorError {
    MatcherError(MatcherError),
    UTF8Error(std::str::Utf8Error),
    InvariantError,
}

/// For a cursor pointed at a code node, extract the matcher with potential extras.
///
/// For the following schema:
/// ```md
/// `name:/\w+/`? is here <-- cursor is here
/// ```
///
/// The matcher would be `/\w+/`?
///
/// We require that the cursor is pointed at a code node, potentially followed by text.
pub fn extract_text_matcher(cursor: &TreeCursor, str: &str) -> Result<Matcher, ExtractorError> {
    // The first node must be a code node
    debug_assert!(cursor.node().kind() == "code_span");
    // We don't need to know anything about the next node

    let node_and_next_node = get_node_and_next_node(cursor);
    match node_and_next_node {
        Some((node, Some(next_node))) if node.kind() == "code_span" => {
            let node_text = node
                .utf8_text(str.as_bytes())
                .map_err(ExtractorError::UTF8Error)?;
            let next_node_text = next_node.utf8_text(str.as_bytes()).ok();
            Matcher::try_from_pattern_and_suffix_str(node_text, next_node_text)
                .map_err(ExtractorError::MatcherError)
        }
        Some((node, None)) if node.kind() == "code_span" => {
            let node_text = node
                .utf8_text(str.as_bytes())
                .map_err(ExtractorError::UTF8Error)?;
            Matcher::try_from_pattern_and_suffix_str(node_text, None)
                .map_err(ExtractorError::MatcherError)
        }
        _ => Err(ExtractorError::InvariantError),
    }
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::{
        matcher::matcher::{
            MATCHERS_EXTRA_PATTERN, Matcher, MatcherError, MatcherExtras, MatcherExtrasError,
            extract_text_matcher,
        },
        ts_utils::new_markdown_parser,
    };

    #[test]
    fn test_matcher_creation_and_matching() {
        let matcher = Matcher::try_from_pattern_and_suffix_str("`word:/\\w+/`", None).unwrap();
        assert_eq!(matcher.id, Some("word".to_string()));
        assert_eq!(matcher.match_str("hello world"), Some("hello"));
        assert_eq!(matcher.match_str("1234"), Some("1234"));
        assert_eq!(matcher.match_str("!@#$"), None);
    }

    #[test]
    fn test_matcher_invalid_pattern() {
        // Test error handling for invalid pattern using try_from_pattern_and_suffix_str
        let result = Matcher::try_from_pattern_and_suffix_str("`invalid_pattern`", None);
        assert!(result.is_err());
        match result.unwrap_err() {
            MatcherError::MatcherInteriorRegexInvalid(_) => {
                // Expected error type
            }
            _ => panic!("Expected MatcherInteriorRegexInvalid error"),
        }
    }

    #[test]
    fn test_new_matcher_with_bullshit_extras() {
        let matches = MATCHERS_EXTRA_PATTERN.is_match("bullshit");
        assert!(!matches);

        let result = MatcherExtras::try_new(Some("bullshit"));
        assert!(result.is_err());

        // obviously bullshit is not a valid extras pattern.
        // `name:/test/`bullshit is invalid! If they wanted "bullshit" to come
        // directly after the matcher they can just put it directly into the
        // regex.
        let result = Matcher::try_from_pattern_and_suffix_str("`name:/test/`", Some("bullshit"));
        assert!(result.is_err());
    }

    #[test]
    fn test_matcher_display() {
        let matcher = Matcher::try_from_pattern_and_suffix_str("`num:/\\d+/`", None).unwrap();
        let display_str = format!("{}", matcher);
        assert_eq!(display_str, "num:/\\d+/");
    }

    #[test]
    fn test_long_complicated_id_and_regex() {
        let matcher = Matcher::try_from_pattern_and_suffix_str(
            "`complicatedID_123-abc:/[a-zA-Z0-9_\\-]{5,15}/`",
            None,
        )
        .unwrap();
        assert_eq!(matcher.id, Some("complicatedID_123-abc".to_string()));
        assert_eq!(
            matcher.match_str("user_12345 is logged in"),
            Some("user_12345")
        );
        assert_eq!(matcher.match_str("tiny"), None); // Only 4 chars, should not match
    }

    #[test]
    fn test_with_no_id() {
        let matcher = Matcher::try_from_pattern_and_suffix_str("`ruler`", None).unwrap();
        assert!(matcher.is_ruler());

        // It doesn't match anything, since it's not a regex matcher. The only
        // way to use a ruler matcher is by calling `.is_ruler()`
        assert_eq!(matcher.id, None);
        assert_eq!(matcher.match_str("ruler"), None);
        assert_eq!(matcher.match_str("***"), None);
        assert_eq!(matcher.match_str("!@#$"), None);

        let matcher = Matcher::try_from_pattern_and_suffix_str("`id:ruler`", None).unwrap();
        assert_eq!(matcher.id, Some("id".to_string()));
        assert_eq!(matcher.match_str("ruler"), None);
        assert_eq!(matcher.match_str("whatever"), None);
    }

    #[test]
    fn test_item_count_limits() {
        // Test {min,max} parsing
        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{2,5}++")).unwrap();
        assert_eq!(matcher.extras().min_items(), Some(2));
        assert_eq!(matcher.extras().max_items(), Some(5));
        assert!(matcher.is_repeated());

        // Test {min,} (no max)
        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{3,}+")).unwrap();
        assert_eq!(matcher.extras().min_items(), Some(3));
        assert_eq!(matcher.extras().max_items(), None);
        assert!(matcher.is_repeated());

        // Test {,max} (no min)
        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{,10}+++")).unwrap();
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), Some(10));
        assert!(matcher.is_repeated());

        // Test with + before {}
        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("++{1,3}")).unwrap();
        assert_eq!(matcher.extras().min_items(), Some(1));
        assert_eq!(matcher.extras().max_items(), Some(3));
        assert!(matcher.is_repeated());

        // Test without {} - should have no limits
        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{,}")).unwrap();
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), None);
        assert!(matcher.is_repeated());

        // No extras text at all - should not have min/max
        let matcher = Matcher::try_from_pattern_and_suffix_str("`foo:/bar/`", None).unwrap();
        assert!(!matcher.is_repeated());
        assert_eq!(matcher.extras().min_items(), None);
        assert_eq!(matcher.extras().max_items(), None);
    }

    #[test]
    fn test_is_variable_length() {
        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{,}")).unwrap();
        assert!(matcher.variable_length());

        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{,2}")).unwrap();
        assert!(matcher.variable_length());

        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{2,}")).unwrap();
        assert!(matcher.variable_length());

        // Min is not the same as max, so it's variable length
        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{1,3}")).unwrap();
        assert!(matcher.variable_length());

        // Non repeaters are implicitly length 1
        let matcher = Matcher::try_from_pattern_and_suffix_str("`foo:/bar/`", None).unwrap();
        assert!(!matcher.variable_length());

        // Finally, this is not variable length but is a repeater
        let matcher =
            Matcher::try_from_pattern_and_suffix_str("`test:/\\d+/`", Some("{3,3}")).unwrap();
        assert!(!matcher.variable_length());
    }

    #[test]
    fn test_extract_text_matcher() {
        // Test without repeater
        let schema_str = "`name:/\\w+/` is here";

        let mut parser = new_markdown_parser();
        let schema_tree = parser.parse(schema_str, None).unwrap();
        let mut schema_cursor = schema_tree.walk();

        assert_eq!(schema_cursor.node().kind(), "document");
        schema_cursor.goto_first_child();
        assert_eq!(schema_cursor.node().kind(), "paragraph");
        schema_cursor.goto_first_child();
        assert_eq!(schema_cursor.node().kind(), "code_span");

        let matcher = extract_text_matcher(&mut schema_cursor, schema_str).unwrap();

        assert_eq!(matcher.id(), Some("name"));
        assert!(!matcher.is_repeated());

        // Test with repeater
        let schema_str = "`name:/\\w+/`{1,3} is here";

        let mut parser = new_markdown_parser();
        let schema_tree = parser.parse(schema_str, None).unwrap();
        let mut schema_cursor = schema_tree.walk();

        assert_eq!(schema_cursor.node().kind(), "document");
        schema_cursor.goto_first_child();
        assert_eq!(schema_cursor.node().kind(), "paragraph");
        schema_cursor.goto_first_child();
        assert_eq!(schema_cursor.node().kind(), "code_span");

        let matcher = extract_text_matcher(&mut schema_cursor, schema_str).unwrap();

        assert_eq!(matcher.id(), Some("name"));
        assert!(matcher.is_repeated());
    }

    #[test]
    fn test_simple_literal_exclamation() {
        // Test that a single ! makes it literal code
        let result = Matcher::try_from_pattern_and_suffix_str("`test:/\\w+/`", Some("!"));
        assert!(result.is_err());
        match result.unwrap_err() {
            MatcherError::WasLiteralCode => {
                // Expected
            }
            _ => panic!("Expected WasLiteralCode error"),
        }
    }

    #[test]
    fn test_invalid_matcher_with_exclamation() {
        // Test that a single ! makes it literal code even though the matcher isn't valid
        let result = Matcher::try_from_pattern_and_suffix_str("`testing!!!`", Some("!"));
        assert!(result.is_err());
        match result.unwrap_err() {
            MatcherError::WasLiteralCode => {
                // Expected
            }
            _ => panic!("Expected WasLiteralCode error"),
        }
    }

    #[test]
    fn test_literal_codeblock_with_exclamation() {
        // Test that ! after a matcher group makes it a literal codeblock
        let result = Matcher::try_from_pattern_and_suffix_str("`test:/\\w+/`", Some("!"));
        assert!(
            result.is_err(),
            "Expected WasLiteralCode error, got {:?}",
            result
        );
        match result.unwrap_err() {
            MatcherError::WasLiteralCode => {
                // Expected - should be treated as literal code
            }
            _ => panic!("Expected WasLiteralCode error"),
        }
    }

    #[test]
    fn test_mixed_literal_and_non_literal_extras() {
        // Mixing literal code with non-literal extras is invalid
        let result = Matcher::try_from_pattern_and_suffix_str("`test:/\\w+/`", Some("!{,}"));
        assert!(result.is_err());
        match result.unwrap_err() {
            MatcherError::MatcherExtrasError(MatcherExtrasError::MatcherExtrasInvalid) => {
                // You can't combine literal code with non-literal extras
            }
            _ => panic!("Expected WasLiteralCode error"),
        }
    }
}
