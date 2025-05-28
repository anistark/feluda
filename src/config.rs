//! Configuration handling for Feluda
//!
//! This module provides functionality to load and manage configuration settings.
//! Configuration can be provided through:
//!
//! 1. Default values (built into the binary)
//! 2. `.feluda.toml` file in the project root
//! 3. Environment variables prefixed with `FELUDA_`
//!
//! # Configuration File Example
//!
//! ```toml
//! [licenses]
//! # Override the default list of restrictive licenses
//! restrictive = [
//!     "GPL-3.0",      # GNU General Public License v3.0
//!     "AGPL-3.0",     # GNU Affero General Public License v3.0
//!     "LGPL-3.0",     # GNU Lesser General Public License v3.0
//! ]
//! ```
//!
//! # Environment Variables
//!
//! Configuration can be overridden using environment variables:
//!
//! ```sh
//! # Override restrictive licenses list
//! export FELUDA_LICENSES_RESTRICTIVE='["GPL-3.0","AGPL-3.0"]'
//! ```

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::debug::{log, log_debug, log_error, FeludaError, FeludaResult, LogLevel};

/// Main configuration structure for Feluda
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct FeludaConfig {
    #[serde(default)]
    pub licenses: LicenseConfig,
}

/// Configuration for license-related settings
///
/// By default, the following licenses are considered restrictive:
/// - GPL-3.0
/// - AGPL-3.0
/// - LGPL-3.0
/// - MPL-2.0
/// - SEE LICENSE IN LICENSE
/// - CC-BY-SA-4.0
/// - EPL-2.0
///
/// This can be overridden via `.feluda.toml` or environment variables.
#[derive(Debug, Deserialize, Serialize)]
pub struct LicenseConfig {
    #[serde(default = "default_restrictive_licenses")]
    pub restrictive: Vec<String>,
}

impl Default for LicenseConfig {
    fn default() -> Self {
        Self {
            restrictive: default_restrictive_licenses(),
        }
    }
}

/// Returns the default list of restrictive licenses
fn default_restrictive_licenses() -> Vec<String> {
    let licenses = vec![
        "GPL-3.0",
        "AGPL-3.0",
        "LGPL-3.0",
        "MPL-2.0",
        "SEE LICENSE IN LICENSE",
        "CC-BY-SA-4.0",
        "EPL-2.0",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    log_debug("Default restrictive licenses", &licenses);
    licenses
}

/// Loads the configuration using the following providers (in order of precedence):
///
/// 1. Environment variables prefixed with `FELUDA_`
/// 2. `.feluda.toml` file in the project root
/// 3. Default values
///
/// # Environment Variables
///
/// Environment variables are transformed by:
/// 1. Removing the `FELUDA_` prefix
/// 2. Converting to lowercase
/// 3. Converting underscores to dots for nested keys
///
/// For example:
/// - `FELUDA_LICENSES_RESTRICTIVE` -> `licenses.restrictive`
pub fn load_config() -> FeludaResult<FeludaConfig> {
    log(LogLevel::Info, "Loading Feluda configuration");

    // Start with default values
    let mut figment = Figment::new().merge(Serialized::defaults(FeludaConfig::default()));

    // Check if .feluda.toml exists and add it if it does
    let config_path = Path::new(".feluda.toml");
    if config_path.exists() {
        log(
            LogLevel::Info,
            format!("Found configuration file: {}", config_path.display()),
        );
        figment = figment.merge(Toml::file(config_path));
    } else {
        log(LogLevel::Info, "No .feluda.toml file found, using defaults");
    }

    // Add environment variables
    figment = figment.merge(Env::prefixed("FELUDA_").split("_"));
    log(LogLevel::Info, "Checking for FELUDA_ environment variables");

    // Extract the final configuration
    match figment.extract() {
        Ok(config) => {
            log(LogLevel::Info, "Configuration loaded successfully");
            log_debug("Loaded configuration", &config);
            Ok(config)
        }
        Err(e) => {
            log_error("Failed to extract configuration", &e);
            Err(FeludaError::config(format!(
                "Failed to extract configuration: {}",
                e
            )))
        }
    }
}

// Remove the unused function
// Keep it in the tests but commented out for reference
// pub fn has_env_var(var_name: &str) -> bool {
//     std::env::var(format!("FELUDA_{}", var_name)).is_ok()
// }

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        dir
    }

    #[test]
    fn test_default_config() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.licenses.restrictive.len(), 7);
            assert!(config.licenses.restrictive.contains(&"GPL-3.0".to_string()));
        });
    }

    #[test]
    fn test_toml_config() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["TEST-1.0", "TEST-2.0"]"#,
            )
            .unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.licenses.restrictive.len(), 2);
            assert!(config
                .licenses
                .restrictive
                .contains(&"TEST-1.0".to_string()));
            assert!(config
                .licenses
                .restrictive
                .contains(&"TEST-2.0".to_string()));
        });
    }

    #[test]
    fn test_env_config() {
        temp_env::with_vars(
            vec![(
                "FELUDA_LICENSES_RESTRICTIVE",
                Some(r#"["ENV-1.0","ENV-2.0"]"#),
            )],
            || {
                let dir = setup();
                std::env::set_current_dir(dir.path()).unwrap();

                let config = load_config().unwrap();
                assert_eq!(config.licenses.restrictive.len(), 2);
                assert!(config.licenses.restrictive.contains(&"ENV-1.0".to_string()));
                assert!(config.licenses.restrictive.contains(&"ENV-2.0".to_string()));
            },
        );
    }

    #[test]
    fn test_env_overrides_toml() {
        temp_env::with_var(
            "FELUDA_LICENSES_RESTRICTIVE",
            Some(r#"["ENV-1.0"]"#),
            || {
                let dir = setup();
                std::env::set_current_dir(dir.path()).unwrap();

                fs::write(
                    ".feluda.toml",
                    r#"[licenses]
restrictive = ["TOML-1.0", "TOML-2.0"]"#,
                )
                .unwrap();

                let config = load_config().unwrap();
                assert_eq!(config.licenses.restrictive.len(), 1);
                assert!(config.licenses.restrictive.contains(&"ENV-1.0".to_string()));
            },
        );
    }

    // Commented out the test that would require has_env_var
    /*
    #[test]
    fn test_has_env_var() {
        temp_env::with_var("FELUDA_TEST_VAR", Some("value"), || {
            assert!(has_env_var("TEST_VAR"));
            assert!(!has_env_var("NONEXISTENT_VAR"));
        });
    }
    */
}
