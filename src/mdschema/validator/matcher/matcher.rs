#![allow(dead_code)]

use core::fmt;
use regex::Regex;
use std::{collections::HashSet, sync::LazyLock};
use tree_sitter::TreeCursor;

use crate::mdschema::validator::{
    matcher::matcher_extras::{MatcherExtrasError, get_all_extras, partition_at_special_chars},
    ts_utils::{get_next_node, get_node_and_next_node, is_text_node},
};

static REGEX_MATCHER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(((?P<id>[a-zA-Z0-9-_]+)):)?\/(?P<regex>.+?)\/").unwrap());

static RANGE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{(\d*),(\d*)\}").unwrap());

pub const LITERAL_INDICATOR: char = '!';

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

impl From<MatcherExtrasError> for MatcherError {
    fn from(err: MatcherExtrasError) -> Self {
        MatcherError::MatcherExtrasError(err)
    }
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
    /// Create new `MatcherExtras` directly with the extras that come after a matcher.
    ///
    /// # Arguments
    /// * `extras` - The extras string that follows a matcher. Does not include any potential additional text.
    pub fn try_from_extras_str(extras: &str) -> Result<Self, MatcherExtrasError> {
        let is_literal = extras.starts_with(LITERAL_INDICATOR);
        if is_literal {
            // If it's literal, we can't have anything else after the matcher
            if extras.len() > 1 {
                return Err(MatcherExtrasError::MixedLiteralAndOthers);
            }

            Ok(Self {
                min_items: None,
                max_items: None,
                had_min_max: false,
                is_literal_code: true,
            })
        } else {
            let (min_items, max_items, had_range_syntax) = extract_item_count_limits(extras);

            Ok(Self {
                min_items,
                max_items,
                had_min_max: had_range_syntax,
                is_literal_code: is_literal, // We handle literal code at a higher level now
            })
        }
    }

    /// Create new `MatcherExtras` by parsing the text following a matcher.
    ///
    /// # Arguments
    /// * `text` - Optional text following the matcher code block
    pub fn try_from_post_matcher_str(text: Option<&str>) -> Result<Self, MatcherExtrasError> {
        let extras_str = get_all_extras(text.unwrap_or_default())?;
        Self::try_from_extras_str(extras_str)
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
pub struct MatcherType {
    regex: Regex,
}

impl fmt::Display for MatcherType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.regex.as_str())
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
    /// * `after_str` - Optional extras string following the pattern. This must
    ///   have a sequence of valid matcher extras, only followed by additional
    ///   text if there is a space in between.
    pub fn try_from_pattern_and_suffix_str(
        pattern_str: &str,
        after_str: Option<&str>,
    ) -> Result<Matcher, MatcherError> {
        let pattern_str = pattern_str[1..pattern_str.len() - 1].trim(); // Remove surrounding backticks
        let captures = REGEX_MATCHER_PATTERN.captures(pattern_str);

        let extras = MatcherExtras::try_from_post_matcher_str(after_str)?;

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

        let original_str_len = pattern_str.len() + after_str.map_or(0, |s| s.len());

        Ok(Self::new_with_empty_flags(
            id,
            pattern,
            extras,
            original_str_len,
        ))
    }

    /// Given a schema cursor pointing at a `code_span` node, attempt to extract a new `Matcher`.
    ///
    /// We try to treat the `code_span` as a valid matcher, and grab extras from a
    /// proceeding text node if it exists.
    ///
    /// # Arguments
    ///
    /// * `schema_cursor`: A reference to a schema cursor pointing at a `code_span` node.
    /// * `schema_str`: The string contents of the schema.
    ///
    /// # Returns
    ///
    /// If we fail to construct a `Matcher`, we error with a corresponding
    /// `MatcherError`, otherwise the new `Matcher`.
    pub fn try_from_schema_cursor(
        schema_cursor: &TreeCursor,
        schema_str: &str,
    ) -> Result<Self, MatcherError> {
        let pattern_str = schema_cursor
            .node()
            .utf8_text(schema_str.as_bytes())
            .unwrap();
        let next_node = get_next_node(schema_cursor);
        let extras_str = next_node
            .filter(|n| is_text_node(&n)) // don't bother if not text; extras must be in text
            .map(|n| n.utf8_text(schema_str.as_bytes()).unwrap())
            .and_then(|n| partition_at_special_chars(n).map(|(extras, _)| extras));

        Self::try_from_pattern_and_suffix_str(pattern_str, extras_str)
    }

    /// Get an actual match string for a given text, if it matches.
    pub fn match_str<'a>(&self, text: &'a str) -> Option<&'a str> {
        match self.pattern.regex.find(text) {
            Some(mat) => Some(&text[mat.start()..mat.end()]),
            None => None,
        }
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
    let regex_pattern = captures
        .name("regex")
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| {
            MatcherError::MatcherInteriorRegexInvalid(format!(
                "Expected format: 'id:/regex/', got {}",
                pattern
            ))
        })?;

    let matcher = MatcherType {
        regex: Regex::new(&format!("^{}", regex_pattern)).map_err(|e| {
            MatcherError::MatcherInteriorRegexInvalid(format!("Invalid regex pattern: {}", e))
        })?,
    };

    Ok((id, matcher))
}

impl fmt::Display for Matcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let regex_str = self.pattern.regex.as_str();
        // The regex is stored as "^<pattern>", so remove the leading ^
        let pattern_str = if regex_str.starts_with('^') {
            &regex_str[1..]
        } else {
            regex_str
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
    InvariantError(String),
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
    #[cfg(feature = "invariant_violations")]
    if cursor.node().kind() != "code_span" {
        crate::invariant_violation!(
            "extract_text_matcher expects code_span, got {}",
            cursor.node().kind()
        );
    }
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
        #[cfg(feature = "invariant_violations")]
        _ => crate::invariant_violation!(
            "extract_text_matcher expects code_span, got {}",
            cursor.node().kind()
        ),
        #[cfg(not(feature = "invariant_violations"))]
        _ => Err(ExtractorError::InvariantError(format!(
            "extract_text_matcher expects code_span, got {}",
            cursor.node().kind()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use crate::mdschema::validator::{
        matcher::matcher::{
            Matcher, MatcherError, MatcherExtrasError, extract_text_matcher,
            partition_at_special_chars,
        },
        ts_utils::{new_markdown_parser, parse_markdown},
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
        match result.as_ref().unwrap_err() {
            MatcherError::MatcherInteriorRegexInvalid(_) => {
                // Expected error type
            }
            _ => panic!(
                "Expected MatcherInteriorRegexInvalid error, got {:?}",
                result.unwrap_err()
            ),
        }
    }

    #[test]
    fn test_new_matcher_with_bullshit_extras() {
        // For now, this actually is fine. It will assume there are no extras,
        // rather than there being wrong ones. We probably want to change this
        // eventually though.
        let result = Matcher::try_from_pattern_and_suffix_str("`name:/test/`", Some("bullshit"));
        assert!(!result.is_err()); // TODO: for now
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
        match result.as_ref().unwrap_err() {
            MatcherError::WasLiteralCode => {
                // Expected
            }
            _ => panic!(
                "Expected WasLiteralCode error, got {:?}",
                result.as_ref().unwrap_err()
            ),
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
            error => panic!("Expected WasLiteralCode error, got {:?}", error),
        }
    }

    #[test]
    fn test_mixed_literal_and_non_literal_extras() {
        // Mixing literal code with non-literal extras is invalid
        let result = Matcher::try_from_pattern_and_suffix_str("`test:/\\w+/`", Some("!{,}"));
        assert!(&result.is_err());
        match &result.unwrap_err() {
            MatcherError::MatcherExtrasError(MatcherExtrasError::MixedLiteralAndOthers) => {
                // You can't combine literal code with non-literal extras
            }
            error => panic!("Expected MixedLiteralAndOthers error, got {:?}", error),
        }
    }

    #[test]
    fn get_everything_after_special_chars_single_exclamation() {
        let result = partition_at_special_chars("! ");
        assert_eq!(result, Some(("!", " ")));
    }

    #[test]
    fn get_everything_after_special_chars_repeating() {
        let result = partition_at_special_chars("{,1} hi");
        assert_eq!(result, Some(("{,1}", " hi")));
    }

    #[test]
    fn test_try_from_schema_cursor_simple_repeating() {
        let schema_str = "`test:/\\w+/`{1,2}";
        let schema_tree = parse_markdown(schema_str).unwrap();
        let mut schema_cursor = schema_tree.walk();

        schema_cursor.goto_first_child(); // document -> paragraph
        schema_cursor.goto_first_child(); // paragraph -> code_span
        assert_eq!(schema_cursor.node().kind(), "code_span");

        let matcher = Matcher::try_from_schema_cursor(&schema_cursor, schema_str).unwrap();
        assert_eq!(matcher.pattern().to_string(), r"^\w+");
        assert_eq!(matcher.id(), Some("test"));
    }

    #[test]
    fn test_try_from_schema_cursor_literal() {
        match Matcher::try_from_pattern_and_suffix_str("`test:/\\w+/`", Some("!")).unwrap_err() {
            MatcherError::WasLiteralCode => {}
            e => panic!("Expected WasLiteralCode error, got {:?}", e),
        }
    }
}
