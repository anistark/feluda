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
}

impl Default for LicenseConfig {
    fn default() -> Self {
        Self {
            restrictive: default_restrictive_licenses(),
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

        // Check for duplicate licenses
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

        // Validate license format (basic SPDX-like validation)
        for license in &self.restrictive {
            if !Self::is_valid_license_identifier(license) {
                log(
                    LogLevel::Warn,
                    &format!("License '{license}' may not be a valid SPDX identifier"),
                );
            }
        }

        log_debug("License configuration validation passed", &self.restrictive);
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
}

impl Default for DependencyConfig {
    fn default() -> Self {
        Self {
            max_depth: default_max_depth(),
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

        log_debug(
            "Dependency configuration validation passed",
            &self.max_depth,
        );
        Ok(())
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
            },
            dependencies: DependencyConfig { max_depth: 5 },
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
        };
        // Empty list should pass validation but generate a warning
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_license_config_validation_empty_license() {
        let config = LicenseConfig {
            restrictive: vec!["MIT".to_string(), "".to_string(), "GPL-3.0".to_string()],
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
        let config = DependencyConfig { max_depth: 0 };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be greater than 0"));
    }

    #[test]
    fn test_dependency_config_validation_excessive_depth() {
        let config = DependencyConfig { max_depth: 150 };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be 100 or less"));
    }

    #[test]
    fn test_dependency_config_validation_high_depth_warning() {
        let config = DependencyConfig { max_depth: 75 };
        // Should pass validation but generate a warning
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_dependency_config_validation_valid_depth() {
        let config = DependencyConfig { max_depth: 10 };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_feluda_config_validation_success() {
        let config = FeludaConfig {
            strict: false,
            licenses: LicenseConfig {
                restrictive: vec!["MIT".to_string(), "GPL-3.0".to_string()],
            },
            dependencies: DependencyConfig { max_depth: 10 },
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_feluda_config_validation_license_failure() {
        let config = FeludaConfig {
            strict: false,
            licenses: LicenseConfig {
                restrictive: vec!["".to_string()], // Invalid empty license
            },
            dependencies: DependencyConfig { max_depth: 10 },
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
            },
            dependencies: DependencyConfig { max_depth: 0 }, // Invalid zero depth
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
}
