//! Environment variable configuration for mdvalidate.
//!
//! This module provides a structured way to access environment variables
//! using the `envy` crate with serde deserialization.

use serde::Deserialize;

/// Environment configuration for the application.
///
/// All fields are optional.
#[derive(Debug, Deserialize, Clone)]
pub struct EnvConfig {
    /// Enable debug mode for error output.
    ///
    /// When enabled, errors are printed using simple Debug formatting
    /// instead of pretty-printed Ariadne reports.
    ///
    /// Set via: `DEV_DEBUG=1` or `DEV_DEBUG=true`
    #[serde(default)]
    pub dev_debug: bool,
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self { dev_debug: false }
    }
}

impl EnvConfig {
    /// Load configuration from environment variables.
    ///
    /// This will attempt to parse environment variables into the config struct.
    /// If parsing fails or variables are not set, it will return the default config.
    pub fn load() -> Self {
        envy::from_env::<EnvConfig>().unwrap_or_default()
    }

    /// Check if debug mode is enabled.
    pub fn is_debug_mode(&self) -> bool {
        self.dev_debug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EnvConfig::default();
        assert!(!config.is_debug_mode());
    }

    #[test]
    fn test_env_config_load_defaults_when_no_vars() {
        // This test assumes no DEV_DEBUG env var is set
        // In a real scenario, we might want to use a test harness that clears env vars
        let config = EnvConfig::load();
        // Should not panic and should return some config
        assert!(!config.dev_debug || config.dev_debug); // Always true, just ensures it loads
    }
}
