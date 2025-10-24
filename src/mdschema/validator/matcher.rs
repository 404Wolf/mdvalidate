use anyhow::Result;
use core::fmt;
use log::debug;
use regex::Regex;
use std::sync::LazyLock;

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
        debug!(
            "Testing if text '{}' matches pattern '{}'",
            text,
            self.regex.as_str()
        );

        self.regex.is_match(text)
    }
}

impl fmt::Display for Matcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let regex_str = self.regex.as_str();
        let regex_str = format!("{}", &regex_str[1..regex_str.len() - 1]);

        write!(f, "#{}:/{}/", self.id, regex_str)
    }
}
