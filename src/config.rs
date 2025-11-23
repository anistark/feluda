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
//!
//! # Licenses to ignore from analysis
//! ignore = [
//!     "MIT",          # MIT License
//!     "Apache-2.0",   # Apache License 2.0
//! ]
//!
//! [[dependencies.ignore]]
//! name = "github.com/opcotech/elemo-pre-mailer"
//! version = "v1.0.0"
//! reason = "This is within the same repo as the project, hence it shares the same license."
//!
//! [[dependencies.ignore]]
//! name = "something-else"
//! version = ""  # Empty version means ignore all versions of this dependency
//! reason = "We have a written acknowledgment from the author that we may use their code under our license."
//! ```
//!
//! # Environment Variables
//!
//! Configuration can be overridden using environment variables:
//!
//! ```sh
//! # Override restrictive licenses list
//! export FELUDA_LICENSES_RESTRICTIVE='["GPL-3.0","AGPL-3.0"]'
//! # Override ignore licenses list
//! export FELUDA_LICENSES_IGNORE='["MIT","Apache-2.0"]'
//! ```

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::debug::{log, log_debug, log_error, FeludaError, FeludaResult, LogLevel};

/// Main configuration structure for Feluda
#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct FeludaConfig {
    #[serde(default)]
    pub licenses: LicenseConfig,
    #[serde(default)]
    pub dependencies: DependencyConfig,
    #[serde(default)]
    pub strict: bool,
}

impl FeludaConfig {
    /// Validates the configuration for logical consistency and correctness
    pub fn validate(&self) -> FeludaResult<()> {
        self.licenses.validate()?;
        self.dependencies.validate()?;
        Ok(())
    }
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
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LicenseConfig {
    #[serde(default = "default_restrictive_licenses")]
    pub restrictive: Vec<String>,
    #[serde(default)]
    pub ignore: Vec<String>,
}

impl Default for LicenseConfig {
    fn default() -> Self {
        Self {
            restrictive: default_restrictive_licenses(),
            ignore: Vec::new(),
        }
    }
}

impl LicenseConfig {
    /// Validates the license configuration
    pub fn validate(&self) -> FeludaResult<()> {
        // Check for empty restrictive licenses list
        if self.restrictive.is_empty() {
            log(
                LogLevel::Warn,
                "No restrictive licenses configured - all licenses will be considered acceptable",
            );
        }

        // Check for duplicate licenses in restrictive list
        let mut seen = std::collections::HashSet::new();
        let mut duplicates = Vec::new();

        for license in &self.restrictive {
            if license.trim().is_empty() {
                return Err(FeludaError::Config(
                    "Empty license string found in restrictive licenses list".to_string(),
                ));
            }

            if !seen.insert(license) {
                duplicates.push(license.clone());
            }
        }

        if !duplicates.is_empty() {
            return Err(FeludaError::Config(format!(
                "Duplicate licenses found in restrictive list: {}",
                duplicates.join(", ")
            )));
        }

        // Validate license format for restrictive licenses (basic SPDX-like validation)
        for license in &self.restrictive {
            if !Self::is_valid_license_identifier(license) {
                log(
                    LogLevel::Warn,
                    &format!("License '{license}' may not be a valid SPDX identifier"),
                );
            }
        }

        // Validate ignore licenses list
        let mut ignore_seen = std::collections::HashSet::new();
        let mut ignore_duplicates = Vec::new();

        for license in &self.ignore {
            if license.trim().is_empty() {
                return Err(FeludaError::Config(
                    "Empty license string found in ignore licenses list".to_string(),
                ));
            }

            if !ignore_seen.insert(license) {
                ignore_duplicates.push(license.clone());
            }
        }

        if !ignore_duplicates.is_empty() {
            return Err(FeludaError::Config(format!(
                "Duplicate licenses found in ignore list: {}",
                ignore_duplicates.join(", ")
            )));
        }

        // Validate license format for ignore licenses
        for license in &self.ignore {
            if !Self::is_valid_license_identifier(license) {
                log(
                    LogLevel::Warn,
                    &format!(
                        "License '{license}' in ignore list may not be a valid SPDX identifier"
                    ),
                );
            }
        }

        // Check for overlap between restrictive and ignore lists
        let restrictive_set: std::collections::HashSet<_> = self.restrictive.iter().collect();
        let ignore_set: std::collections::HashSet<_> = self.ignore.iter().collect();
        let overlap: Vec<_> = restrictive_set
            .intersection(&ignore_set)
            .map(|s| s.to_string())
            .collect();

        if !overlap.is_empty() {
            log(
                LogLevel::Warn,
                &format!(
                    "Licenses found in both restrictive and ignore lists will be ignored: {}",
                    overlap.join(", ")
                ),
            );
        }

        log_debug("License configuration validation passed", &self.restrictive);
        log_debug("Ignore licenses configuration", &self.ignore);
        Ok(())
    }

    /// Basic validation for license identifiers
    fn is_valid_license_identifier(license: &str) -> bool {
        let license = license.trim();

        // Special cases that are valid but don't follow standard patterns
        if matches!(
            license,
            "SEE LICENSE IN LICENSE" | "UNLICENSED" | "NOASSERTION"
        ) {
            return true;
        }

        // Basic pattern: should contain only alphanumeric, dots, hyphens, plus, parentheses
        // TODO: Improve with a full SPDX regex
        license
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '+' | '(' | ')' | '_'))
            && !license.is_empty()
            && license.len() <= 100
    }
}

/// Configuration for dependency-related settings
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DependencyConfig {
    /// Maximum depth for transitive dependency resolution
    /// Default is 10 levels deep
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    /// Dependencies to exclude from license scanning
    #[serde(default)]
    pub ignore: Vec<IgnoreDependency>,
}

/// Configuration for a dependency to ignore
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IgnoreDependency {
    /// The name/identifier of the dependency (e.g., "github.com/opcotech/elemo-pre-mailer")
    pub name: String,
    /// The version of the dependency. Leave empty to ignore all versions.
    #[serde(default)]
    pub version: String,
    /// Reason for ignoring this dependency
    #[serde(default)]
    pub reason: String,
}

impl Default for DependencyConfig {
    fn default() -> Self {
        Self {
            max_depth: default_max_depth(),
            ignore: Vec::new(),
        }
    }
}

impl DependencyConfig {
    /// Validates the dependency configuration
    pub fn validate(&self) -> FeludaResult<()> {
        // Validate max_depth is within reasonable bounds
        if self.max_depth == 0 {
            return Err(FeludaError::Config(
                "max_depth must be greater than 0".to_string(),
            ));
        }

        if self.max_depth > 100 {
            return Err(FeludaError::Config(
                "max_depth must be 100 or less to prevent excessive resource usage".to_string(),
            ));
        }

        if self.max_depth > 50 {
            log(
                LogLevel::Warn,
                &format!(
                    "max_depth of {} is quite high and may impact performance",
                    self.max_depth
                ),
            );
        }

        // Validate ignore dependencies
        for dep in self.ignore.iter() {
            if dep.name.trim().is_empty() {
                return Err(FeludaError::Config(
                    "Empty dependency name found in ignore list".to_string(),
                ));
            }

            // Warn if reason is empty
            if dep.reason.trim().is_empty() {
                log(
                    LogLevel::Warn,
                    &format!(
                        "Dependency '{}' in ignore list has no reason specified",
                        dep.name
                    ),
                );
            }
        }

        // Check for duplicate dependencies in ignore list
        let mut seen = std::collections::HashSet::new();
        let mut duplicates = Vec::new();

        for dep in &self.ignore {
            let key = (dep.name.clone(), dep.version.clone());
            if !seen.insert(key.clone()) {
                duplicates.push(format!("{}@{}", dep.name, dep.version));
            }
        }

        if !duplicates.is_empty() {
            return Err(FeludaError::Config(format!(
                "Duplicate dependencies found in ignore list: {}",
                duplicates.join(", ")
            )));
        }

        if !self.ignore.is_empty() {
            log_debug("Dependency ignore list", &self.ignore.len());
        }

        log_debug(
            "Dependency configuration validation passed",
            &self.max_depth,
        );
        Ok(())
    }

    /// Check if a dependency should be ignored based on configuration
    /// Returns true if the dependency matches an ignore rule (name and optionally version)
    pub fn should_ignore_dependency(&self, name: &str, version: Option<&str>) -> bool {
        self.ignore.iter().any(|ignored| {
            // Match by name (case-sensitive)
            if ignored.name != name {
                return false;
            }

            // If version is specified in ignore rule, match exactly
            if !ignored.version.is_empty() {
                return version.is_some_and(|v| v == ignored.version);
            }

            // If version is empty in ignore rule, ignore all versions
            true
        })
    }
}

/// Returns the default maximum depth for dependency resolution
fn default_max_depth() -> u32 {
    10
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
            &format!("Found configuration file: {}", config_path.display()),
        );
        figment = figment.merge(Toml::file(config_path));
    } else {
        log(LogLevel::Info, "No .feluda.toml file found, using defaults");
    }

    // Add environment variables
    figment = figment.merge(Env::prefixed("FELUDA_").split("_"));
    log(LogLevel::Info, "Checking for FELUDA_ environment variables");

    // Extract the final configuration
    match figment.extract::<FeludaConfig>() {
        Ok(config) => {
            log(LogLevel::Info, "Configuration loaded successfully");
            log_debug("Loaded configuration", &config);

            // Validate the configuration
            if let Err(e) = config.validate() {
                log_error("Configuration validation failed", &e);
                return Err(e);
            }

            log(LogLevel::Info, "Configuration validation passed");
            Ok(config)
        }
        Err(e) => {
            log_error("Failed to extract configuration", &e);
            Err(FeludaError::Config(format!(
                "Failed to extract configuration: {e}"
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
restrictive = ["TEST-1.0", "TEST-2.0"]

[dependencies]
max_depth = 5"#,
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
            assert_eq!(config.dependencies.max_depth, 5);
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

    #[test]
    fn test_license_config_default() {
        let config = LicenseConfig::default();
        assert_eq!(config.restrictive.len(), 7);
        assert!(config.restrictive.contains(&"GPL-3.0".to_string()));
        assert!(config.restrictive.contains(&"AGPL-3.0".to_string()));
        assert!(config.restrictive.contains(&"LGPL-3.0".to_string()));
        assert!(config.restrictive.contains(&"MPL-2.0".to_string()));
        assert!(config
            .restrictive
            .contains(&"SEE LICENSE IN LICENSE".to_string()));
        assert!(config.restrictive.contains(&"CC-BY-SA-4.0".to_string()));
        assert!(config.restrictive.contains(&"EPL-2.0".to_string()));
    }

    #[test]
    fn test_feluda_config_default() {
        let config = FeludaConfig::default();
        assert_eq!(config.licenses.restrictive.len(), 7);
    }

    #[test]
    fn test_default_restrictive_licenses() {
        let licenses = default_restrictive_licenses();
        assert_eq!(licenses.len(), 7);
        assert!(licenses.contains(&"GPL-3.0".to_string()));
        assert!(licenses.contains(&"AGPL-3.0".to_string()));
        assert!(licenses.contains(&"LGPL-3.0".to_string()));
        assert!(licenses.contains(&"MPL-2.0".to_string()));
        assert!(licenses.contains(&"SEE LICENSE IN LICENSE".to_string()));
        assert!(licenses.contains(&"CC-BY-SA-4.0".to_string()));
        assert!(licenses.contains(&"EPL-2.0".to_string()));
    }

    #[test]
    fn test_load_config_missing_file() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            let config = load_config().unwrap();

            assert_eq!(config.licenses.restrictive.len(), 7);
            assert!(config.licenses.restrictive.contains(&"GPL-3.0".to_string()));
        });
    }

    #[test]
    fn test_load_config_invalid_toml() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(".feluda.toml", "invalid toml content [[[").unwrap();

            let result = load_config();
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_load_config_partial_toml() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"# This is a comment
[other_section]
some_field = "value"
"#,
            )
            .unwrap();

            let config = load_config().unwrap();

            assert_eq!(config.licenses.restrictive.len(), 7);
        });
    }

    #[test]
    fn test_load_config_empty_restrictive_list() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = []"#,
            )
            .unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.licenses.restrictive.len(), 0);
        });
    }

    #[test]
    fn test_load_config_env_invalid_json() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", Some("invalid json"), || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            let result = load_config();
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_load_config_nested_env_variables() {
        temp_env::with_vars(
            vec![
                ("FELUDA_LICENSES_RESTRICTIVE", Some(r#"["CUSTOM-1.0"]"#)),
                ("FELUDA_OTHER_SETTING", Some("value")),
            ],
            || {
                let dir = tempfile::tempdir().unwrap();
                std::env::set_current_dir(dir.path()).unwrap();

                let config = load_config().unwrap();
                assert_eq!(config.licenses.restrictive.len(), 1);
                assert!(config
                    .licenses
                    .restrictive
                    .contains(&"CUSTOM-1.0".to_string()));
            },
        );
    }

    #[test]
    fn test_load_config_precedence_order() {
        // Test that environment variables override TOML config
        temp_env::with_var(
            "FELUDA_LICENSES_RESTRICTIVE",
            Some(r#"["ENV-LICENSE"]"#),
            || {
                let dir = tempfile::tempdir().unwrap();
                std::env::set_current_dir(dir.path()).unwrap();

                // Create TOML with different values
                fs::write(
                    ".feluda.toml",
                    r#"[licenses]
restrictive = ["TOML-LICENSE-1", "TOML-LICENSE-2"]"#,
                )
                .unwrap();

                let config = load_config().unwrap();

                // Should use environment variable value, not TOML
                assert_eq!(config.licenses.restrictive.len(), 1);
                assert!(config
                    .licenses
                    .restrictive
                    .contains(&"ENV-LICENSE".to_string()));
                assert!(!config
                    .licenses
                    .restrictive
                    .contains(&"TOML-LICENSE-1".to_string()));
            },
        );
    }

    #[test]
    fn test_config_serialization() {
        let config = FeludaConfig {
            strict: false,
            licenses: LicenseConfig {
                restrictive: vec!["TEST-1.0".to_string(), "TEST-2.0".to_string()],
                ignore: Vec::new(),
            },
            dependencies: DependencyConfig {
                max_depth: 5,
                ignore: Vec::new(),
            },
        };

        // Test that config can be serialized and deserialized
        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("TEST-1.0"));
        assert!(serialized.contains("TEST-2.0"));

        let deserialized: FeludaConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.licenses.restrictive.len(), 2);
        assert!(deserialized
            .licenses
            .restrictive
            .contains(&"TEST-1.0".to_string()));
    }

    #[test]
    fn test_config_debug_output() {
        let config = FeludaConfig::default();
        let debug_str = format!("{config:?}");

        assert!(debug_str.contains("FeludaConfig"));
        assert!(debug_str.contains("licenses"));
        assert!(debug_str.contains("restrictive"));
    }

    #[test]
    fn test_license_config_serde() {
        let config = LicenseConfig {
            restrictive: vec!["MIT".to_string(), "Apache-2.0".to_string()],
            ignore: Vec::new(),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("MIT"));
        assert!(json.contains("Apache-2.0"));

        let deserialized: LicenseConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.restrictive.len(), 2);
    }

    #[test]
    fn test_load_config_with_comments() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"# Feluda configuration file
# This file configures license checking behavior

[licenses]
# List of licenses that are considered restrictive
restrictive = [
    "GPL-3.0",      # GNU General Public License
    "CUSTOM-1.0",   # Custom restrictive license
]
"#,
            )
            .unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.licenses.restrictive.len(), 2);
            assert!(config.licenses.restrictive.contains(&"GPL-3.0".to_string()));
            assert!(config
                .licenses
                .restrictive
                .contains(&"CUSTOM-1.0".to_string()));
        });
    }

    #[test]
    fn test_load_config_env_array_format() {
        temp_env::with_var(
            "FELUDA_LICENSES_RESTRICTIVE",
            Some(r#"["License-1", "License-2", "License-3"]"#),
            || {
                let dir = tempfile::tempdir().unwrap();
                std::env::set_current_dir(dir.path()).unwrap();

                let config = load_config().unwrap();
                assert_eq!(config.licenses.restrictive.len(), 3);
                assert!(config
                    .licenses
                    .restrictive
                    .contains(&"License-1".to_string()));
                assert!(config
                    .licenses
                    .restrictive
                    .contains(&"License-2".to_string()));
                assert!(config
                    .licenses
                    .restrictive
                    .contains(&"License-3".to_string()));
            },
        );
    }

    #[test]
    fn test_load_config_case_sensitivity() {
        temp_env::with_vars(
            vec![
                ("FELUDA_LICENSES_RESTRICTIVE", None::<&str>),
                ("OTHER_LICENSES_RESTRICTIVE", Some(r#"["WRONG-PREFIX"]"#)),
            ],
            || {
                let dir = tempfile::tempdir().unwrap();
                std::env::set_current_dir(dir.path()).unwrap();

                let config = load_config().unwrap();

                assert_eq!(config.licenses.restrictive.len(), 7);
                assert!(config.licenses.restrictive.contains(&"GPL-3.0".to_string()));
                assert!(!config
                    .licenses
                    .restrictive
                    .contains(&"WRONG-PREFIX".to_string()));
            },
        );
    }

    // Validation tests
    #[test]
    fn test_license_config_validation_empty_list() {
        let config = LicenseConfig {
            restrictive: vec![],
            ignore: Vec::new(),
        };
        // Empty list should pass validation but generate a warning
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_license_config_validation_empty_license() {
        let config = LicenseConfig {
            restrictive: vec!["MIT".to_string(), "".to_string(), "GPL-3.0".to_string()],
            ignore: Vec::new(),
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty license string"));
    }

    #[test]
    fn test_license_config_validation_duplicate_licenses() {
        let config = LicenseConfig {
            restrictive: vec![
                "MIT".to_string(),
                "GPL-3.0".to_string(),
                "MIT".to_string(),
                "Apache-2.0".to_string(),
            ],
            ignore: Vec::new(),
        };
        let result = config.validate();
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Duplicate licenses"));
        assert!(error_msg.contains("MIT"));
    }

    #[test]
    fn test_license_config_validation_valid_licenses() {
        let config = LicenseConfig {
            restrictive: vec![
                "MIT".to_string(),
                "Apache-2.0".to_string(),
                "GPL-3.0".to_string(),
                "SEE LICENSE IN LICENSE".to_string(),
            ],
            ignore: Vec::new(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_license_identifier_validation() {
        assert!(LicenseConfig::is_valid_license_identifier("MIT"));
        assert!(LicenseConfig::is_valid_license_identifier("Apache-2.0"));
        assert!(LicenseConfig::is_valid_license_identifier("GPL-3.0+"));
        assert!(LicenseConfig::is_valid_license_identifier(
            "SEE LICENSE IN LICENSE"
        ));
        assert!(LicenseConfig::is_valid_license_identifier("UNLICENSED"));
        assert!(LicenseConfig::is_valid_license_identifier("NOASSERTION"));

        assert!(!LicenseConfig::is_valid_license_identifier(""));
        assert!(!LicenseConfig::is_valid_license_identifier(
            &"x".repeat(101)
        )); // Too long
    }

    #[test]
    fn test_dependency_config_validation_zero_depth() {
        let config = DependencyConfig {
            max_depth: 0,
            ignore: Vec::new(),
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be greater than 0"));
    }

    #[test]
    fn test_dependency_config_validation_excessive_depth() {
        let config = DependencyConfig {
            max_depth: 150,
            ignore: Vec::new(),
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be 100 or less"));
    }

    #[test]
    fn test_dependency_config_validation_high_depth_warning() {
        let config = DependencyConfig {
            max_depth: 75,
            ignore: Vec::new(),
        };
        // Should pass validation but generate a warning
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_dependency_config_validation_valid_depth() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: Vec::new(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_feluda_config_validation_success() {
        let config = FeludaConfig {
            strict: false,
            licenses: LicenseConfig {
                restrictive: vec!["MIT".to_string(), "GPL-3.0".to_string()],
                ignore: Vec::new(),
            },
            dependencies: DependencyConfig {
                max_depth: 10,
                ignore: Vec::new(),
            },
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_feluda_config_validation_license_failure() {
        let config = FeludaConfig {
            strict: false,
            licenses: LicenseConfig {
                restrictive: vec!["".to_string()], // Invalid empty license
                ignore: Vec::new(),
            },
            dependencies: DependencyConfig {
                max_depth: 10,
                ignore: Vec::new(),
            },
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty license string"));
    }

    #[test]
    fn test_feluda_config_validation_dependency_failure() {
        let config = FeludaConfig {
            strict: false,
            licenses: LicenseConfig {
                restrictive: vec!["MIT".to_string()],
                ignore: Vec::new(),
            },
            dependencies: DependencyConfig {
                max_depth: 0,
                ignore: Vec::new(),
            }, // Invalid zero depth
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be greater than 0"));
    }

    #[test]
    fn test_load_config_validation_integration() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["MIT", "GPL-3.0"]

[dependencies]
max_depth = 15"#,
            )
            .unwrap();

            // Should pass validation
            let config = load_config().unwrap();
            assert_eq!(config.licenses.restrictive.len(), 2);
            assert_eq!(config.dependencies.max_depth, 15);
        });
    }

    #[test]
    fn test_load_config_validation_failure() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["MIT", ""]

[dependencies]
max_depth = 5"#,
            )
            .unwrap();

            // Should fail validation due to empty license string
            let result = load_config();
            assert!(result.is_err());
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("Empty license string"));
        });
    }

    #[test]
    fn test_load_config_correct_env_var() {
        temp_env::with_var(
            "FELUDA_LICENSES_RESTRICTIVE",
            Some(r#"["CUSTOM-LICENSE"]"#),
            || {
                let dir = tempfile::tempdir().unwrap();
                std::env::set_current_dir(dir.path()).unwrap();

                let config = load_config().unwrap();
                assert_eq!(config.licenses.restrictive.len(), 1);
                assert!(config
                    .licenses
                    .restrictive
                    .contains(&"CUSTOM-LICENSE".to_string()));
            },
        );
    }

    // Tests for ignore licenses functionality
    #[test]
    fn test_toml_config_with_ignore() {
        temp_env::with_var("FELUDA_LICENSES_IGNORE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["GPL-3.0"]
ignore = ["MIT", "Apache-2.0"]"#,
            )
            .unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.licenses.ignore.len(), 2);
            assert!(config.licenses.ignore.contains(&"MIT".to_string()));
            assert!(config.licenses.ignore.contains(&"Apache-2.0".to_string()));
        });
    }

    #[test]
    fn test_env_config_with_ignore() {
        temp_env::with_var(
            "FELUDA_LICENSES_IGNORE",
            Some(r#"["MIT","BSD-3-Clause"]"#),
            || {
                let dir = setup();
                std::env::set_current_dir(dir.path()).unwrap();

                let config = load_config().unwrap();
                assert_eq!(config.licenses.ignore.len(), 2);
                assert!(config.licenses.ignore.contains(&"MIT".to_string()));
                assert!(config.licenses.ignore.contains(&"BSD-3-Clause".to_string()));
            },
        );
    }

    #[test]
    fn test_env_ignore_overrides_toml() {
        temp_env::with_var("FELUDA_LICENSES_IGNORE", Some(r#"["ENV-IGNORE"]"#), || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
ignore = ["TOML-IGNORE-1", "TOML-IGNORE-2"]"#,
            )
            .unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.licenses.ignore.len(), 1);
            assert!(config.licenses.ignore.contains(&"ENV-IGNORE".to_string()));
        });
    }

    #[test]
    fn test_empty_ignore_list() {
        temp_env::with_var("FELUDA_LICENSES_IGNORE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
ignore = []"#,
            )
            .unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.licenses.ignore.len(), 0);
        });
    }

    #[test]
    fn test_license_config_validation_ignore_empty_license() {
        let config = LicenseConfig {
            restrictive: vec!["GPL-3.0".to_string()],
            ignore: vec!["MIT".to_string(), "".to_string(), "Apache-2.0".to_string()],
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty license string"));
    }

    #[test]
    fn test_license_config_validation_ignore_duplicate_licenses() {
        let config = LicenseConfig {
            restrictive: vec!["GPL-3.0".to_string()],
            ignore: vec![
                "MIT".to_string(),
                "Apache-2.0".to_string(),
                "MIT".to_string(),
            ],
        };
        let result = config.validate();
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Duplicate licenses"));
        assert!(error_msg.contains("ignore list"));
    }

    #[test]
    fn test_license_config_validation_ignore_overlap_with_restrictive() {
        let config = LicenseConfig {
            restrictive: vec!["GPL-3.0".to_string(), "MIT".to_string()],
            ignore: vec!["MIT".to_string(), "Apache-2.0".to_string()],
        };
        // Should pass validation but generate a warning
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_license_config_with_all_fields() {
        let config = LicenseConfig {
            restrictive: vec!["GPL-3.0".to_string(), "AGPL-3.0".to_string()],
            ignore: vec!["MIT".to_string(), "Apache-2.0".to_string()],
        };
        assert!(config.validate().is_ok());
        assert_eq!(config.restrictive.len(), 2);
        assert_eq!(config.ignore.len(), 2);
    }

    #[test]
    fn test_load_config_toml_with_comments() {
        temp_env::with_var("FELUDA_LICENSES_IGNORE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"# Feluda configuration file
[licenses]
# List of restrictive licenses
restrictive = ["GPL-3.0"]
# Licenses to ignore
ignore = [
    "MIT",          # MIT License
    "Apache-2.0",   # Apache License
]"#,
            )
            .unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.licenses.restrictive.len(), 1);
            assert_eq!(config.licenses.ignore.len(), 2);
            assert!(config.licenses.ignore.contains(&"MIT".to_string()));
            assert!(config.licenses.ignore.contains(&"Apache-2.0".to_string()));
        });
    }

    #[test]
    fn test_default_ignore_list_is_empty() {
        let config = FeludaConfig::default();
        assert!(config.licenses.ignore.is_empty());
    }

    #[test]
    fn test_load_config_ignore_serde() {
        let config = LicenseConfig {
            restrictive: vec!["GPL-3.0".to_string()],
            ignore: vec!["MIT".to_string(), "Apache-2.0".to_string()],
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("MIT"));
        assert!(json.contains("Apache-2.0"));

        let deserialized: LicenseConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ignore.len(), 2);
        assert!(deserialized.ignore.contains(&"MIT".to_string()));
    }

    // Tests for dependency ignore functionality
    #[test]
    fn test_dependency_config_ignore_basic() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: vec![IgnoreDependency {
                name: "lodash".to_string(),
                version: "4.17.21".to_string(),
                reason: "Test reason".to_string(),
            }],
        };
        assert!(config.should_ignore_dependency("lodash", Some("4.17.21")));
        assert!(!config.should_ignore_dependency("lodash", Some("4.17.20")));
        assert!(!config.should_ignore_dependency("underscore", Some("4.17.21")));
    }

    #[test]
    fn test_dependency_config_ignore_all_versions() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: vec![IgnoreDependency {
                name: "lodash".to_string(),
                version: "".to_string(),
                reason: "Ignore all versions".to_string(),
            }],
        };
        assert!(config.should_ignore_dependency("lodash", Some("4.17.21")));
        assert!(config.should_ignore_dependency("lodash", Some("4.17.20")));
        assert!(config.should_ignore_dependency("lodash", None));
        assert!(!config.should_ignore_dependency("underscore", Some("1.0.0")));
    }

    #[test]
    fn test_dependency_config_should_ignore_dependency_multiple() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: vec![
                IgnoreDependency {
                    name: "lodash".to_string(),
                    version: "4.17.21".to_string(),
                    reason: "Specific version".to_string(),
                },
                IgnoreDependency {
                    name: "underscore".to_string(),
                    version: "".to_string(),
                    reason: "All versions".to_string(),
                },
            ],
        };
        assert!(config.should_ignore_dependency("lodash", Some("4.17.21")));
        assert!(!config.should_ignore_dependency("lodash", Some("4.17.20")));
        assert!(config.should_ignore_dependency("underscore", Some("1.0.0")));
        assert!(config.should_ignore_dependency("underscore", None));
    }

    #[test]
    fn test_dependency_config_validation_empty_ignore() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: Vec::new(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_dependency_config_validation_empty_name() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: vec![IgnoreDependency {
                name: "".to_string(),
                version: "1.0.0".to_string(),
                reason: "Test".to_string(),
            }],
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty dependency name"));
    }

    #[test]
    fn test_dependency_config_validation_duplicate_dependencies() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: vec![
                IgnoreDependency {
                    name: "lodash".to_string(),
                    version: "4.17.21".to_string(),
                    reason: "First".to_string(),
                },
                IgnoreDependency {
                    name: "lodash".to_string(),
                    version: "4.17.21".to_string(),
                    reason: "Second".to_string(),
                },
            ],
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Duplicate dependencies"));
    }

    #[test]
    fn test_dependency_config_validation_no_reason_warning() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: vec![IgnoreDependency {
                name: "lodash".to_string(),
                version: "4.17.21".to_string(),
                reason: "".to_string(),
            }],
        };
        // Should pass validation but generate a warning
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_toml_config_with_dependency_ignore() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["GPL-3.0"]

[[dependencies.ignore]]
name = "lodash"
version = "4.17.21"
reason = "Dependency within same repo"

[[dependencies.ignore]]
name = "underscore"
version = ""
reason = "All versions ignored"
"#,
            )
            .unwrap();

            let config = load_config().unwrap();
            assert_eq!(config.dependencies.ignore.len(), 2);
            assert!(config
                .dependencies
                .should_ignore_dependency("lodash", Some("4.17.21")));
            assert!(!config
                .dependencies
                .should_ignore_dependency("lodash", Some("4.17.20")));
            assert!(config
                .dependencies
                .should_ignore_dependency("underscore", Some("1.0.0")));
        });
    }

    #[test]
    fn test_feluda_config_with_dependency_ignore() {
        let config = FeludaConfig {
            strict: false,
            licenses: LicenseConfig {
                restrictive: vec!["GPL-3.0".to_string()],
                ignore: Vec::new(),
            },
            dependencies: DependencyConfig {
                max_depth: 10,
                ignore: vec![IgnoreDependency {
                    name: "lodash".to_string(),
                    version: "4.17.21".to_string(),
                    reason: "Test".to_string(),
                }],
            },
        };
        assert!(config.validate().is_ok());
        assert!(config
            .dependencies
            .should_ignore_dependency("lodash", Some("4.17.21")));
    }

    #[test]
    fn test_dependency_ignore_serialization() {
        let dep = IgnoreDependency {
            name: "lodash".to_string(),
            version: "4.17.21".to_string(),
            reason: "Test reason".to_string(),
        };

        let json = serde_json::to_string(&dep).unwrap();
        assert!(json.contains("lodash"));
        assert!(json.contains("4.17.21"));
        assert!(json.contains("Test reason"));

        let deserialized: IgnoreDependency = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "lodash");
        assert_eq!(deserialized.version, "4.17.21");
        assert_eq!(deserialized.reason, "Test reason");
    }

    #[test]
    fn test_default_dependency_config() {
        let config = DependencyConfig::default();
        assert_eq!(config.max_depth, 10);
        assert!(config.ignore.is_empty());
    }

    #[test]
    fn test_dependency_ignore_empty_version_field() {
        let config = DependencyConfig {
            max_depth: 10,
            ignore: vec![
                IgnoreDependency {
                    name: "package1".to_string(),
                    version: "".to_string(),
                    reason: "Ignore all versions".to_string(),
                },
                IgnoreDependency {
                    name: "package2".to_string(),
                    version: "1.0.0".to_string(),
                    reason: "Ignore specific version".to_string(),
                },
            ],
        };

        assert!(config.should_ignore_dependency("package1", Some("any-version")));
        assert!(config.should_ignore_dependency("package1", None));
        assert!(config.should_ignore_dependency("package2", Some("1.0.0")));
        assert!(!config.should_ignore_dependency("package2", Some("2.0.0")));
    }
}
