//! Core license analysis functionality and types

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;
use toml::Value as TomlValue;

use crate::cli;
use crate::config;
use crate::debug::{log, log_debug, log_error, FeludaResult, LogLevel};

// Re-export language-specific functions for backward compatibility
// TODO: Remove when 1.8.5 is no longer supported
#[allow(unused_imports)]
pub use crate::languages::{
    analyze_go_licenses, analyze_js_licenses, analyze_python_licenses, analyze_rust_licenses,
    fetch_license_for_go_dependency, fetch_license_for_python_dependency, get_go_dependencies,
    GoPackages, PackageJson,
};

/// License compatibility enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LicenseCompatibility {
    Compatible,
    Incompatible,
    Unknown,
}

impl std::fmt::Display for LicenseCompatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compatible => write!(f, "Compatible"),
            Self::Incompatible => write!(f, "Incompatible"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// License Info of dependencies
#[derive(Serialize, Debug, Clone)]
pub struct LicenseInfo {
    pub name: String,                        // The name of the software or library
    pub version: String,                     // The version of the software or library
    pub license: Option<String>, // An optional field that contains the license type (e.g., MIT, Apache 2.0)
    pub is_restrictive: bool,    // A boolean indicating whether the license is restrictive or not
    pub compatibility: LicenseCompatibility, // Compatibility with project license
}

impl LicenseInfo {
    pub fn get_license(&self) -> String {
        match &self.license {
            Some(license_name) => String::from(license_name),
            None => String::from("No License"),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn is_restrictive(&self) -> &bool {
        &self.is_restrictive
    }

    pub fn compatibility(&self) -> &LicenseCompatibility {
        &self.compatibility
    }
}

/// License Info structure for GitHub API data
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct License {
    pub title: String,            // The full name of the license
    pub spdx_id: String,          // The SPDX identifier for the license
    pub permissions: Vec<String>, // A list of permissions granted by the license
    pub conditions: Vec<String>,  // A list of conditions that must be met under the license
    pub limitations: Vec<String>, // A list of limitations imposed by the license
}

/// Fetch license data from GitHub's official Licenses API
pub fn fetch_licenses_from_github() -> FeludaResult<HashMap<String, License>> {
    log(LogLevel::Info, "Fetching licenses from GitHub Licenses API");

    let licenses_map = cli::with_spinner("Fetching licenses from GitHub API", |indicator| {
        let mut licenses_map = HashMap::new();
        let mut license_count = 0;

        // First, get the list of available licenses
        let client = match reqwest::blocking::Client::builder()
            .user_agent("feluda-license-checker/1.0")
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                log_error("Failed to create HTTP client", &err);
                return licenses_map;
            }
        };

        indicator.update_progress("fetching license list");

        let licenses_list_url = "https://api.github.com/licenses";
        let response = match client.get(licenses_list_url).send() {
            Ok(response) => response,
            Err(err) => {
                log_error("Failed to fetch licenses list from GitHub API", &err);
                return licenses_map;
            }
        };

        if !response.status().is_success() {
            log(
                LogLevel::Error,
                &format!("GitHub API returned error status: {}", response.status()),
            );
            return licenses_map;
        }

        let licenses_list: Vec<serde_json::Value> = match response.json() {
            Ok(list) => list,
            Err(err) => {
                log_error("Failed to parse licenses list JSON", &err);
                return licenses_map;
            }
        };

        let total_licenses = licenses_list.len();
        indicator.update_progress(&format!("found {total_licenses} licenses"));

        for (idx, license_info) in licenses_list.iter().enumerate() {
            if let Some(license_key) = license_info.get("key").and_then(|k| k.as_str()) {
                indicator.update_progress(&format!(
                    "processing {}/{}: {}",
                    idx + 1,
                    total_licenses,
                    license_key
                ));

                log(
                    LogLevel::Info,
                    &format!("Fetching detailed license info: {license_key}"),
                );

                // Fetch detailed license information
                let license_url = format!("https://api.github.com/licenses/{license_key}");

                // Add a small delay to avoid rate limiting
                std::thread::sleep(Duration::from_millis(100));

                match client.get(&license_url).send() {
                    Ok(license_response) => {
                        if license_response.status().is_success() {
                            match license_response.json::<serde_json::Value>() {
                                Ok(license_data) => {
                                    // Extract the license information we need
                                    let title = license_data
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or(license_key)
                                        .to_string();

                                    let spdx_id = license_data
                                        .get("spdx_id")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or(license_key)
                                        .to_string();

                                    let permissions = license_data
                                        .get("permissions")
                                        .and_then(|p| p.as_array())
                                        .map(|arr| {
                                            arr.iter()
                                                .filter_map(|v| v.as_str())
                                                .map(String::from)
                                                .collect()
                                        })
                                        .unwrap_or_default();

                                    let conditions = license_data
                                        .get("conditions")
                                        .and_then(|c| c.as_array())
                                        .map(|arr| {
                                            arr.iter()
                                                .filter_map(|v| v.as_str())
                                                .map(String::from)
                                                .collect()
                                        })
                                        .unwrap_or_default();

                                    let limitations = license_data
                                        .get("limitations")
                                        .and_then(|l| l.as_array())
                                        .map(|arr| {
                                            arr.iter()
                                                .filter_map(|v| v.as_str())
                                                .map(String::from)
                                                .collect()
                                        })
                                        .unwrap_or_default();

                                    let license = License {
                                        title,
                                        spdx_id,
                                        permissions,
                                        conditions,
                                        limitations,
                                    };

                                    // Use the SPDX ID as the key for consistency
                                    let key_to_use = license_data
                                        .get("spdx_id")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or(license_key);

                                    licenses_map.insert(key_to_use.to_string(), license);
                                    license_count += 1;

                                    log(
                                        LogLevel::Info,
                                        &format!("Successfully processed license: {key_to_use}"),
                                    );
                                }
                                Err(err) => {
                                    log_error(
                                        &format!(
                                            "Failed to parse license JSON for {license_key}"
                                        ),
                                        &err,
                                    );
                                }
                            }
                        } else {
                            log(
                                LogLevel::Error,
                                &format!(
                                    "Failed to fetch license {}: HTTP {}",
                                    license_key,
                                    license_response.status()
                                ),
                            );
                        }
                    }
                    Err(err) => {
                        log_error(
                            &format!("Failed to fetch license details for {license_key}"),
                            &err,
                        );
                    }
                }
            }
        }

        indicator.update_progress(&format!("processed {license_count} licenses"));

        log(
            LogLevel::Info,
            &format!(
                "Successfully fetched {license_count} licenses from GitHub API"
            ),
        );
        licenses_map
    });

    Ok(licenses_map)
}

/// Check if a license is considered restrictive based on configuration and known licenses
pub fn is_license_restrictive(
    license: &Option<String>,
    known_licenses: &HashMap<String, License>,
) -> bool {
    log(
        LogLevel::Info,
        &format!("Checking if license is restrictive: {license:?}"),
    );

    let config = match config::load_config() {
        Ok(cfg) => {
            log(LogLevel::Info, "Successfully loaded configuration");
            cfg
        }
        Err(e) => {
            log_error("Error loading configuration", &e);
            log(LogLevel::Warn, "Using default configuration");
            config::FeludaConfig::default()
        }
    };

    if license.as_deref() == Some("No License") {
        log(
            LogLevel::Warn,
            "No license specified, considering as restrictive",
        );
        return true;
    }

    if let Some(license_str) = license {
        log_debug(
            "Checking against known licenses",
            &known_licenses.keys().collect::<Vec<_>>(),
        );

        if let Some(license_data) = known_licenses.get(license_str) {
            log_debug("Found license data", license_data);

            const CONDITIONS: [&str; 2] = ["source-disclosure", "network-use-disclosure"];
            let is_restrictive = CONDITIONS
                .iter()
                .any(|&condition| license_data.conditions.contains(&condition.to_string()));

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    &format!("License {license_str} is restrictive due to conditions"),
                );
            } else {
                log(
                    LogLevel::Info,
                    &format!("License {license_str} is not restrictive"),
                );
            }

            return is_restrictive;
        } else {
            // Check against user-configured restrictive licenses
            log_debug(
                "Checking against configured restrictive licenses",
                &config.licenses.restrictive,
            );

            let is_restrictive = config
                .licenses
                .restrictive
                .iter()
                .any(|restrictive_license| license_str.contains(restrictive_license));

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    &format!(
                        "License {license_str} matches restrictive pattern in config"
                    ),
                );
            } else {
                log(
                    LogLevel::Info,
                    &format!(
                        "License {license_str} does not match any restrictive pattern"
                    ),
                );
            }

            return is_restrictive;
        }
    }

    log(LogLevel::Warn, "No license information available");
    false
}

/// Check if a license is compatible with the base project license
pub fn is_license_compatible(
    dependency_license: &str,
    project_license: &str,
) -> LicenseCompatibility {
    log(
        LogLevel::Info,
        &format!(
            "Checking if license {dependency_license} is compatible with project license {project_license}"
        ),
    );

    // Define a compatibility matrix using a HashMap
    let compatibility_matrix: HashMap<&str, Vec<&str>> = [
        // MIT is compatible with almost everything
        (
            "MIT",
            vec![
                "MIT",
                "BSD-2-Clause",
                "BSD-3-Clause",
                "Apache-2.0",
                "LGPL-3.0",
                "MPL-2.0",
            ],
        ),
        // Apache 2.0 compatibility
        (
            "Apache-2.0",
            vec!["MIT", "BSD-2-Clause", "BSD-3-Clause", "Apache-2.0"],
        ),
        // GPL-3.0 can use code from these licenses
        (
            "GPL-3.0",
            vec!["MIT", "BSD-2-Clause", "BSD-3-Clause", "LGPL-3.0", "GPL-3.0"],
        ),
        // LGPL-3.0 compatibility
        (
            "LGPL-3.0",
            vec!["MIT", "BSD-2-Clause", "BSD-3-Clause", "LGPL-3.0"],
        ),
        // MPL-2.0 compatibility
        (
            "MPL-2.0",
            vec!["MIT", "BSD-2-Clause", "BSD-3-Clause", "MPL-2.0"],
        ),
        // BSD licenses
        ("BSD-3-Clause", vec!["MIT", "BSD-2-Clause", "BSD-3-Clause"]),
        ("BSD-2-Clause", vec!["MIT", "BSD-2-Clause", "BSD-3-Clause"]),
    ]
    .iter()
    .cloned()
    .collect();

    // Normalize license identifiers
    let norm_dependency_license = normalize_license_id(dependency_license);
    let norm_project_license = normalize_license_id(project_license);

    log(
        LogLevel::Info,
        &format!(
            "Normalized licenses: dependency={norm_dependency_license}, project={norm_project_license}"
        ),
    );

    // Check compatibility based on the matrix
    match compatibility_matrix.get(norm_project_license.as_str()) {
        Some(compatible_licenses) => {
            if compatible_licenses.contains(&norm_dependency_license.as_str()) {
                log(
                    LogLevel::Info,
                    &format!(
                        "License {norm_dependency_license} is compatible with project license {norm_project_license}"
                    ),
                );
                LicenseCompatibility::Compatible
            } else {
                log(
                    LogLevel::Warn,
                    &format!(
                        "License {norm_dependency_license} may be incompatible with project license {norm_project_license}"
                    ),
                );
                LicenseCompatibility::Incompatible
            }
        }
        None => {
            log(
                LogLevel::Warn,
                &format!(
                    "Unknown compatibility for project license {norm_project_license}"
                ),
            );
            LicenseCompatibility::Unknown
        }
    }
}

/// Normalize license identifier to a standard format
fn normalize_license_id(license_id: &str) -> String {
    // Handle common variations
    match license_id.trim().to_uppercase().as_str() {
        "MIT" => "MIT".to_string(),
        id if id.contains("APACHE") && id.contains("2.0") => "Apache-2.0".to_string(),
        id if id.contains("GPL") && id.contains("3") && !id.contains("LGPL") => {
            "GPL-3.0".to_string()
        }
        id if id.contains("LGPL") && id.contains("3") => "LGPL-3.0".to_string(),
        id if id.contains("MPL") && id.contains("2.0") => "MPL-2.0".to_string(),
        id if id.contains("BSD") && id.contains("3") => "BSD-3-Clause".to_string(),
        id if id.contains("BSD") && id.contains("2") => "BSD-2-Clause".to_string(),
        _ => license_id.to_string(),
    }
}

/// Detect the project's license
pub fn detect_project_license(project_path: &str) -> FeludaResult<Option<String>> {
    log(
        LogLevel::Info,
        &format!("Detecting license for project at path: {project_path}"),
    );

    // Check LICENSE file
    let license_paths = [
        Path::new(project_path).join("LICENSE"),
        Path::new(project_path).join("LICENSE.txt"),
        Path::new(project_path).join("LICENSE.md"),
        Path::new(project_path).join("license"),
        Path::new(project_path).join("COPYING"),
    ];

    for license_path in &license_paths {
        if license_path.exists() {
            log(
                LogLevel::Info,
                &format!("Found license file: {}", license_path.display()),
            );

            match fs::read_to_string(license_path) {
                Ok(content) => {
                    // Check for MIT license
                    if content.contains("MIT License")
                        || content.contains("Permission is hereby granted, free of charge")
                    {
                        log(LogLevel::Info, "Detected MIT license");
                        return Ok(Some("MIT".to_string()));
                    }

                    // Check for GPL-3.0
                    if content.contains("GNU GENERAL PUBLIC LICENSE")
                        && content.contains("Version 3")
                    {
                        log(LogLevel::Info, "Detected GPL-3.0 license");
                        return Ok(Some("GPL-3.0".to_string()));
                    }

                    // Check for Apache-2.0
                    if content.contains("Apache License") && content.contains("Version 2.0") {
                        log(LogLevel::Info, "Detected Apache-2.0 license");
                        return Ok(Some("Apache-2.0".to_string()));
                    }

                    // Check for BSD-3-Clause
                    if content.contains("BSD")
                        && content.contains("Redistribution and use")
                        && content.contains("Neither the name")
                    {
                        log(LogLevel::Info, "Detected BSD-3-Clause license");
                        return Ok(Some("BSD-3-Clause".to_string()));
                    }

                    // Check for LGPL-3.0
                    if content.contains("GNU LESSER GENERAL PUBLIC LICENSE")
                        && content.contains("Version 3")
                    {
                        log(LogLevel::Info, "Detected LGPL-3.0 license");
                        return Ok(Some("LGPL-3.0".to_string()));
                    }

                    // Check for MPL-2.0
                    if content.contains("Mozilla Public License") && content.contains("Version 2.0")
                    {
                        log(LogLevel::Info, "Detected MPL-2.0 license");
                        return Ok(Some("MPL-2.0".to_string()));
                    }

                    log(
                        LogLevel::Warn,
                        "License file found but could not determine license type",
                    );
                }
                Err(err) => {
                    log(
                        LogLevel::Error,
                        &format!("Failed to read license file: {}", license_path.display()),
                    );
                    log_debug("Error details", &err);
                }
            }
        }
    }

    // Check package.json for Node.js projects
    let package_json_path = Path::new(project_path).join("package.json");
    if package_json_path.exists() {
        log(
            LogLevel::Info,
            &format!("Found package.json at {}", package_json_path.display()),
        );

        match fs::read_to_string(&package_json_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(json) => {
                    if let Some(license) = json.get("license").and_then(|l| l.as_str()) {
                        log(
                            LogLevel::Info,
                            &format!("Detected license from package.json: {license}"),
                        );
                        return Ok(Some(license.to_string()));
                    }
                }
                Err(err) => {
                    log(
                        LogLevel::Error,
                        &format!("Failed to parse package.json: {err}"),
                    );
                }
            },
            Err(err) => {
                log(
                    LogLevel::Error,
                    &format!(
                        "Failed to read package.json: {}",
                        package_json_path.display()
                    ),
                );
                log_debug("Error details", &err);
            }
        }
    }

    // Check Cargo.toml for Rust projects
    let cargo_toml_path = Path::new(project_path).join("Cargo.toml");
    if cargo_toml_path.exists() {
        log(
            LogLevel::Info,
            &format!("Found Cargo.toml at {}", cargo_toml_path.display()),
        );

        match fs::read_to_string(&cargo_toml_path) {
            Ok(content) => match toml::from_str::<TomlValue>(&content) {
                Ok(toml) => {
                    if let Some(package) = toml.as_table().and_then(|t| t.get("package")) {
                        if let Some(license) = package.get("license").and_then(|l| l.as_str()) {
                            log(
                                LogLevel::Info,
                                &format!("Detected license from Cargo.toml: {license}"),
                            );
                            return Ok(Some(license.to_string()));
                        }
                    }
                }
                Err(err) => {
                    log(
                        LogLevel::Error,
                        &format!("Failed to parse Cargo.toml: {err}"),
                    );
                }
            },
            Err(err) => {
                log(
                    LogLevel::Error,
                    &format!("Failed to read Cargo.toml: {}", cargo_toml_path.display()),
                );
                log_debug("Error details", &err);
            }
        }
    }

    // Check pyproject.toml for Python projects
    let pyproject_toml_path = Path::new(project_path).join("pyproject.toml");
    if pyproject_toml_path.exists() {
        log(
            LogLevel::Info,
            &format!("Found pyproject.toml at {}", pyproject_toml_path.display()),
        );

        match fs::read_to_string(&pyproject_toml_path) {
            Ok(content) => match toml::from_str::<TomlValue>(&content) {
                Ok(toml) => {
                    if let Some(project) = toml.as_table().and_then(|t| t.get("project")) {
                        if let Some(license_info) = project.get("license") {
                            if let Some(license) = license_info.as_str() {
                                log(
                                    LogLevel::Info,
                                    &format!("Detected license from pyproject.toml: {license}"),
                                );
                                return Ok(Some(license.to_string()));
                            } else if let Some(license_table) = license_info.as_table() {
                                if let Some(license_text) =
                                    license_table.get("text").and_then(|t| t.as_str())
                                {
                                    log(
                                        LogLevel::Info,
                                        &format!(
                                            "Detected license from pyproject.toml: {license_text}"
                                        ),
                                    );
                                    return Ok(Some(license_text.to_string()));
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    log(
                        LogLevel::Error,
                        &format!("Failed to parse pyproject.toml: {err}"),
                    );
                }
            },
            Err(err) => {
                log(
                    LogLevel::Error,
                    &format!(
                        "Failed to read pyproject.toml: {}",
                        pyproject_toml_path.display()
                    ),
                );
                log_debug("Error details", &err);
            }
        }
    }

    log(LogLevel::Warn, "No license detected for project");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_license_compatibility_display() {
        assert_eq!(LicenseCompatibility::Compatible.to_string(), "Compatible");
        assert_eq!(
            LicenseCompatibility::Incompatible.to_string(),
            "Incompatible"
        );
        assert_eq!(LicenseCompatibility::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_license_info_methods() {
        let info = LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
        };

        assert_eq!(info.name(), "test_package");
        assert_eq!(info.version(), "1.0.0");
        assert_eq!(info.get_license(), "MIT");
        assert!(!info.is_restrictive());
        assert_eq!(info.compatibility(), &LicenseCompatibility::Compatible);
    }

    #[test]
    fn test_license_info_no_license() {
        let info = LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: None,
            is_restrictive: true,
            compatibility: LicenseCompatibility::Unknown,
        };

        assert_eq!(info.get_license(), "No License");
    }

    #[test]
    fn test_normalize_license_id() {
        assert_eq!(normalize_license_id("MIT"), "MIT");
        assert_eq!(normalize_license_id("mit"), "MIT");
        assert_eq!(normalize_license_id("Apache 2.0"), "Apache-2.0");
        assert_eq!(normalize_license_id("APACHE-2.0"), "Apache-2.0");
        assert_eq!(normalize_license_id("GPL 3.0"), "GPL-3.0");
        assert_eq!(normalize_license_id("gpl-3.0"), "GPL-3.0");
        assert_eq!(normalize_license_id("LGPL 3.0"), "LGPL-3.0");
        assert_eq!(normalize_license_id("MPL 2.0"), "MPL-2.0");
        assert_eq!(normalize_license_id("BSD 3-Clause"), "BSD-3-Clause");
        assert_eq!(normalize_license_id("BSD 2-Clause"), "BSD-2-Clause");
        assert_eq!(normalize_license_id("Unknown License"), "Unknown License");
        assert_eq!(normalize_license_id("  MIT  "), "MIT");
    }

    #[test]
    fn test_is_license_compatible_mit_project() {
        assert_eq!(
            is_license_compatible("MIT", "MIT"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("BSD-2-Clause", "MIT"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("BSD-3-Clause", "MIT"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("Apache-2.0", "MIT"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("LGPL-3.0", "MIT"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("MPL-2.0", "MIT"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("GPL-3.0", "MIT"),
            LicenseCompatibility::Incompatible
        );
    }

    #[test]
    fn test_detect_project_license_mit_file() {
        let temp_dir = TempDir::new().unwrap();
        let license_path = temp_dir.path().join("LICENSE");

        std::fs::write(
            &license_path,
            "MIT License\n\nPermission is hereby granted, free of charge...",
        )
        .unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("MIT".to_string()));
    }

    #[test]
    fn test_detect_project_license_no_license() {
        let temp_dir = TempDir::new().unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, None);
    }
}
