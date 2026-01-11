//! Environment variable configuration for mdvalidate.
//!
//! This module provides a structured way to access environment variables
//! using the `envy` crate with serde deserialization.

use serde::Deserialize;

/// Environment configuration for the application.
///
/// All fields are optional.
#[derive(Debug, Deserialize, Clone)]
#[derive(Default)]
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
    use std::env;

    #[test]
    fn test_env_config_with_dev_debug_enabled() {
        unsafe {
            env::set_var("DEV_DEBUG", "true");
        }
        let config = EnvConfig::load();
        assert!(config.is_debug_mode());
        unsafe {
            env::remove_var("DEV_DEBUG");
        }
    }
}
