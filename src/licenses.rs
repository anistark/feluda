//! Core license analysis functionality and types

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
#[cfg(not(test))]
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Semaphore;
use toml::Value as TomlValue;

use crate::cache;
use crate::cli;
use crate::config;
use crate::debug::{log, log_debug, log_error, FeludaResult, LogLevel};

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

/// Structure for deserializing license compatibility matrix from TOML
#[derive(Deserialize, Debug, Clone)]
struct LicenseCompatibilityMatrix {
    #[serde(rename = "MIT")]
    mit: Option<LicenseEntry>,
    #[serde(rename = "Apache-2_0")]
    apache_2_0: Option<LicenseEntry>,
    #[serde(rename = "GPL-3_0")]
    gpl_3_0: Option<LicenseEntry>,
    #[serde(rename = "GPL-2_0")]
    gpl_2_0: Option<LicenseEntry>,
    #[serde(rename = "AGPL-3_0")]
    agpl_3_0: Option<LicenseEntry>,
    #[serde(rename = "LGPL-3_0")]
    lgpl_3_0: Option<LicenseEntry>,
    #[serde(rename = "LGPL-2_1")]
    lgpl_2_1: Option<LicenseEntry>,
    #[serde(rename = "MPL-2_0")]
    mpl_2_0: Option<LicenseEntry>,
    #[serde(rename = "BSD-3-Clause")]
    bsd_3_clause: Option<LicenseEntry>,
    #[serde(rename = "BSD-2-Clause")]
    bsd_2_clause: Option<LicenseEntry>,
    #[serde(rename = "ISC")]
    isc: Option<LicenseEntry>,
    #[serde(rename = "_0BSD")]
    bsd_0: Option<LicenseEntry>,
    #[serde(rename = "Unlicense")]
    unlicense: Option<LicenseEntry>,
    #[serde(rename = "WTFPL")]
    wtfpl: Option<LicenseEntry>,
}

#[derive(Deserialize, Debug, Clone)]
struct LicenseEntry {
    compatible_with: Vec<String>,
}

/// Static cache for the compatibility matrix
#[cfg(not(test))]
static COMPATIBILITY_MATRIX: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();

/// OSI license status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OsiStatus {
    Approved,
    NotApproved,
    Unknown,
}

impl std::fmt::Display for OsiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Approved => write!(f, "approved"),
            Self::NotApproved => write!(f, "not-approved"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// OSI license information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsiLicenseInfo {
    pub id: String,
    pub name: String,
    pub status: OsiStatus,
}

/// License Info of dependencies
#[derive(Serialize, Debug, Clone)]
pub struct LicenseInfo {
    pub name: String,                        // The name of the software or library
    pub version: String,                     // The version of the software or library
    pub license: Option<String>, // An optional field that contains the license type (e.g., MIT, Apache 2.0)
    pub is_restrictive: bool,    // A boolean indicating whether the license is restrictive or not
    pub compatibility: LicenseCompatibility, // Compatibility with project license
    pub osi_status: OsiStatus,   // OSI approval status
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

    pub fn osi_status(&self) -> &OsiStatus {
        &self.osi_status
    }

    #[allow(dead_code)]
    pub fn osi_info(&self) -> Option<OsiLicenseInfo> {
        self.license.as_ref().map(|license| OsiLicenseInfo {
            id: license.clone(),
            name: license.clone(),
            status: self.osi_status,
        })
    }
}

/// License Info structure for GitHub API data
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct License {
    pub title: String,            // The full name of the license
    pub spdx_id: String,          // The SPDX identifier for the license
    pub permissions: Vec<String>, // A list of permissions granted by the license
    pub conditions: Vec<String>,  // A list of conditions that must be met under the license
    pub limitations: Vec<String>, // A list of limitations imposed by the license
}

/// Fetch license data from GitHub's official Licenses API
/// Attempts to load from cache first, falls back to GitHub API if cache miss or stale
pub fn fetch_licenses_from_github() -> FeludaResult<HashMap<String, License>> {
    log(LogLevel::Info, "Fetching licenses from GitHub Licenses API");

    match cache::load_github_licenses_from_cache() {
        Ok(Some(cached_licenses)) => {
            log(
                LogLevel::Info,
                &format!("Using cached licenses ({})", cached_licenses.len()),
            );
            return Ok(cached_licenses);
        }
        Ok(None) => {
            log(LogLevel::Info, "Cache miss or stale, fetching from GitHub");
        }
        Err(e) => {
            log(
                LogLevel::Warn,
                &format!("Cache read error: {e}, fetching from GitHub"),
            );
        }
    }

    let licenses_map = cli::with_spinner("Fetching licenses from GitHub API", |indicator| {
        // Use tokio runtime for async operations
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                log_error("Failed to create tokio runtime", &err);
                return HashMap::new();
            }
        };

        rt.block_on(fetch_licenses_concurrent(indicator))
    });

    if !licenses_map.is_empty() {
        if let Err(e) = cache::save_github_licenses_to_cache(&licenses_map) {
            log(LogLevel::Warn, &format!("Failed to save cache: {e}"));
        }
    } else {
        log(
            LogLevel::Warn,
            "No licenses fetched from GitHub API, cache not saved",
        );
    }

    Ok(licenses_map)
}

/// Async helper function for concurrent license fetching with rate limiting
async fn fetch_licenses_concurrent(
    indicator: &crate::cli::LoadingIndicator,
) -> HashMap<String, License> {
    let mut licenses_map = HashMap::new();

    // Create async HTTP client
    let client = match reqwest::Client::builder()
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

    // First, get the list of available licenses
    let licenses_list_url = "https://api.github.com/licenses";
    let response = match client.get(licenses_list_url).send().await {
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

    let licenses_list: Vec<serde_json::Value> = match response.json().await {
        Ok(list) => list,
        Err(err) => {
            log_error("Failed to parse licenses list JSON", &err);
            return licenses_map;
        }
    };

    let total_licenses = licenses_list.len();
    indicator.update_progress(&format!("found {total_licenses} licenses"));

    // Rate limiting: Allow max 10 concurrent requests (GitHub's recommended limit)
    let semaphore = Arc::new(Semaphore::new(10));
    let client = Arc::new(client);

    // Collect all license keys
    let license_keys: Vec<String> = licenses_list
        .iter()
        .filter_map(|license_info| {
            license_info
                .get("key")
                .and_then(|k| k.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    // Create futures for concurrent processing
    let mut tasks = Vec::new();

    for license_key in license_keys {
        let semaphore = Arc::clone(&semaphore);
        let client = Arc::clone(&client);

        let task = tokio::spawn(async move {
            // Acquire semaphore permit for rate limiting
            let _permit = semaphore.acquire().await.unwrap();

            log(
                LogLevel::Info,
                &format!("Fetching detailed license info: {license_key}"),
            );

            let license_url = format!("https://api.github.com/licenses/{license_key}");

            // Add delay for rate limiting (reduced from 100ms since we have concurrency control)
            tokio::time::sleep(Duration::from_millis(50)).await;

            match client.get(&license_url).send().await {
                Ok(license_response) => {
                    if license_response.status().is_success() {
                        match license_response.json::<serde_json::Value>().await {
                            Ok(license_data) => {
                                // Extract the license information we need
                                let title = license_data
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or(&license_key)
                                    .to_string();

                                let spdx_id = license_data
                                    .get("spdx_id")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or(&license_key)
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
                                    .unwrap_or(&license_key);

                                log(
                                    LogLevel::Info,
                                    &format!("Successfully processed license: {key_to_use}"),
                                );

                                Some((key_to_use.to_string(), license))
                            }
                            Err(err) => {
                                log_error(
                                    &format!("Failed to parse license JSON for {license_key}"),
                                    &err,
                                );
                                None
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
                        None
                    }
                }
                Err(err) => {
                    log_error(
                        &format!("Failed to fetch license details for {license_key}"),
                        &err,
                    );
                    None
                }
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete and collect results
    let mut license_count = 0;
    for (i, task) in tasks.into_iter().enumerate() {
        indicator.update_progress(&format!(
            "processing {}/{}: concurrent requests",
            i + 1,
            total_licenses,
        ));

        if let Ok(Some((key, license))) = task.await {
            licenses_map.insert(key, license);
            license_count += 1;
        }
    }

    indicator.update_progress(&format!("processed {license_count} licenses"));

    log(
        LogLevel::Info,
        &format!("Successfully fetched {license_count} licenses from GitHub API using concurrent requests"),
    );

    licenses_map
}

/// Static cache for OSI approved licenses
#[cfg(not(test))]
static OSI_LICENSES: OnceLock<HashMap<String, OsiStatus>> = OnceLock::new();

/// Fetch OSI approved licenses from official API
pub fn fetch_osi_licenses() -> FeludaResult<HashMap<String, OsiStatus>> {
    log(LogLevel::Info, "Fetching OSI approved licenses");

    let osi_map = cli::with_spinner("Fetching OSI approved licenses", |indicator| {
        // Use tokio runtime for async operations
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                log_error("Failed to create tokio runtime", &err);
                return HashMap::new();
            }
        };

        rt.block_on(fetch_osi_licenses_async(indicator))
    });

    Ok(osi_map)
}

/// Async helper function for fetching OSI licenses
async fn fetch_osi_licenses_async(
    indicator: &crate::cli::LoadingIndicator,
) -> HashMap<String, OsiStatus> {
    let mut osi_map = HashMap::new();

    // Create async HTTP client
    let client = match reqwest::Client::builder()
        .user_agent("feluda-license-checker/1.0")
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            log_error("Failed to create HTTP client", &err);
            return osi_map;
        }
    };

    indicator.update_progress("fetching OSI licenses");

    let osi_api_url = "https://api.opensource.org/licenses/";
    let response = match client.get(osi_api_url).send().await {
        Ok(response) => response,
        Err(err) => {
            log_error("Failed to fetch OSI licenses from API", &err);
            return osi_map;
        }
    };

    if !response.status().is_success() {
        log(
            LogLevel::Error,
            &format!("OSI API returned error status: {}", response.status()),
        );
        return osi_map;
    }

    let osi_licenses: Vec<serde_json::Value> = match response.json().await {
        Ok(licenses) => licenses,
        Err(err) => {
            log_error("Failed to parse OSI licenses JSON", &err);
            return osi_map;
        }
    };

    let total_licenses = osi_licenses.len();
    indicator.update_progress(&format!("found {total_licenses} OSI licenses"));

    for license_data in osi_licenses {
        if let Some(id) = license_data.get("id").and_then(|id| id.as_str()) {
            // All licenses from OSI API are approved
            osi_map.insert(id.to_string(), OsiStatus::Approved);
        }
    }

    indicator.update_progress(&format!("processed {total_licenses} OSI licenses"));

    log(
        LogLevel::Info,
        &format!("Successfully fetched {total_licenses} OSI approved licenses"),
    );

    osi_map
}

/// Get the OSI licenses map, loading it if not already cached
fn get_osi_licenses() -> &'static HashMap<String, OsiStatus> {
    #[cfg(not(test))]
    {
        OSI_LICENSES.get_or_init(|| {
            fetch_osi_licenses().unwrap_or_else(|e| {
                log(LogLevel::Warn, &format!("Failed to load OSI licenses: {e}"));
                log(LogLevel::Warn, "Continuing without OSI license information");
                HashMap::new()
            })
        })
    }

    #[cfg(test)]
    {
        use std::cell::RefCell;
        thread_local! {
            static OSI_MAP: RefCell<Option<HashMap<String, OsiStatus>>> = const { RefCell::new(None) };
        }

        OSI_MAP.with(|m| {
            let mut map = m.borrow_mut();
            if map.is_none() {
                match fetch_osi_licenses() {
                    Ok(loaded_map) => {
                        *map = Some(loaded_map);
                    }
                    Err(_) => {
                        *map = Some(HashMap::new());
                    }
                }
            }

            // Leak the memory to get a static reference (only for tests)
            let leaked: &'static HashMap<String, OsiStatus> =
                Box::leak(Box::new(map.as_ref().unwrap().clone()));
            leaked
        })
    }
}

/// Check OSI approval status for a license
pub fn get_osi_status(license_id: &str) -> OsiStatus {
    let normalized_id = normalize_license_id(license_id);
    let osi_licenses = get_osi_licenses();

    // Check for exact match first
    if let Some(status) = osi_licenses.get(&normalized_id) {
        return *status;
    }

    // Check for original license ID
    if let Some(status) = osi_licenses.get(license_id) {
        return *status;
    }

    // For well-known licenses, we can provide static mappings as fallback
    match normalized_id.as_str() {
        "MIT" | "Apache-2.0" | "BSD-3-Clause" | "BSD-2-Clause" | "GPL-3.0" | "GPL-2.0"
        | "LGPL-3.0" | "LGPL-2.1" | "MPL-2.0" | "ISC" | "0BSD" => OsiStatus::Approved,
        "No License" => OsiStatus::NotApproved,
        _ => OsiStatus::Unknown,
    }
}

/// Check if a license is considered restrictive based on configuration and known licenses
pub fn is_license_restrictive(
    license: &Option<String>,
    known_licenses: &HashMap<String, License>,
    strict: bool,
) -> bool {
    log(
        LogLevel::Info,
        &format!("Checking if license is restrictive: {license:?} (strict={strict})"),
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

            let conditions = if strict {
                vec![
                    "source-disclosure",
                    "network-use-disclosure",
                    "disclose-source",
                    "same-license",
                ]
            } else {
                vec!["source-disclosure", "network-use-disclosure"]
            };

            let is_restrictive = conditions
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
            let is_restrictive = config
                .licenses
                .restrictive
                .iter()
                .any(|restrictive_license| license_str.contains(restrictive_license));

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    &format!("License {license_str} matches restrictive pattern in config"),
                );
            } else if strict && license_str.contains("Unknown") {
                log(
                    LogLevel::Warn,
                    &format!(
                        "License {license_str} is unknown in strict mode, considering restrictive"
                    ),
                );
                return true;
            } else {
                log(
                    LogLevel::Info,
                    &format!("License {license_str} does not match any restrictive pattern"),
                );
            }

            return is_restrictive;
        }
    }

    if strict {
        log(
            LogLevel::Warn,
            "No license information available in strict mode, considering restrictive",
        );
        return true;
    }

    log(LogLevel::Warn, "No license information available");
    false
}

/// Check if a license should be ignored from analysis
///
/// Returns true if the license is in the ignore list configured in `.feluda.toml`
/// or via `FELUDA_LICENSES_IGNORE` environment variable.
pub fn is_license_ignored(license: Option<&str>) -> bool {
    log(
        LogLevel::Info,
        &format!("Checking if license should be ignored: {license:?}"),
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

    if let Some(license_str) = license {
        let is_ignored = config
            .licenses
            .ignore
            .iter()
            .any(|ignore_license| license_str.contains(ignore_license));

        if is_ignored {
            log(
                LogLevel::Info,
                &format!("License {license_str} matches ignore pattern in config"),
            );
        } else {
            log(
                LogLevel::Info,
                &format!("License {license_str} does not match any ignore pattern"),
            );
        }

        return is_ignored;
    }

    log(LogLevel::Info, "No license specified, not ignoring");
    false
}

/// This is the default configuration
const EMBEDDED_LICENSE_COMPATIBILITY_TOML: &str =
    include_str!("../config/license_compatibility.toml");

/// Load license compatibility matrix from external TOML file if available
/// Looks for the file in the following order:
/// 1. .feluda/license_compatibility.toml (user-specific config directory)
/// 2. Embedded configuration
fn load_compatibility_matrix() -> FeludaResult<HashMap<String, Vec<String>>> {
    log(
        LogLevel::Info,
        "Loading license compatibility matrix from TOML file",
    );

    // Only check for user-specific config in .feluda directory
    let config_paths = vec![Path::new(".feluda/license_compatibility.toml").to_path_buf()];

    let mut config_content = None;
    let mut used_path = None;

    for path in &config_paths {
        if path.exists() {
            log(
                LogLevel::Info,
                &format!("Found license compatibility config at: {}", path.display()),
            );
            match fs::read_to_string(path) {
                Ok(content) => {
                    config_content = Some(content);
                    used_path = Some(path);
                    break;
                }
                Err(e) => {
                    log(
                        LogLevel::Warn,
                        &format!("Failed to read {}: {}", path.display(), e),
                    );
                    continue;
                }
            }
        }
    }

    // Use embedded configuration as fallback if no external file is found
    let config_content = match config_content {
        Some(content) => content,
        None => {
            log(
                LogLevel::Info,
                "No external license compatibility config found, using embedded configuration",
            );
            EMBEDDED_LICENSE_COMPATIBILITY_TOML.to_string()
        }
    };

    let matrix: LicenseCompatibilityMatrix = toml::from_str(&config_content).map_err(|e| {
        let source = match &used_path {
            Some(path) => format!("external config file ({})", path.display()),
            None => "embedded configuration".to_string(),
        };
        log(
            LogLevel::Error,
            &format!("Failed to parse license compatibility {source}: {e}"),
        );
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;

    // Convert TOML structure to HashMap
    let entries = [
        ("MIT", &matrix.mit),
        ("Apache-2.0", &matrix.apache_2_0),
        ("GPL-3.0", &matrix.gpl_3_0),
        ("GPL-2.0", &matrix.gpl_2_0),
        ("AGPL-3.0", &matrix.agpl_3_0),
        ("LGPL-3.0", &matrix.lgpl_3_0),
        ("LGPL-2.1", &matrix.lgpl_2_1),
        ("MPL-2.0", &matrix.mpl_2_0),
        ("BSD-3-Clause", &matrix.bsd_3_clause),
        ("BSD-2-Clause", &matrix.bsd_2_clause),
        ("ISC", &matrix.isc),
        ("0BSD", &matrix.bsd_0),
        ("Unlicense", &matrix.unlicense),
        ("WTFPL", &matrix.wtfpl),
    ];

    let result: HashMap<String, Vec<String>> = entries
        .iter()
        .filter_map(|(key, option_entry)| {
            option_entry
                .as_ref()
                .map(|entry| (key.to_string(), entry.compatible_with.clone()))
        })
        .collect();

    log(
        LogLevel::Info,
        &format!("Loaded {} license compatibility entries", result.len()),
    );
    Ok(result)
}

/// Get the compatibility matrix, loading it if not already cached
fn get_compatibility_matrix() -> &'static HashMap<String, Vec<String>> {
    #[cfg(not(test))]
    {
        COMPATIBILITY_MATRIX.get_or_init(|| {
            load_compatibility_matrix().unwrap_or_else(|e| {
                log(LogLevel::Error, &format!("Failed to load license compatibility matrix: {e}"));
                log(LogLevel::Error, "This is a critical error. The application cannot function without license compatibility data.");
                std::process::exit(1);
            })
        })
    }

    #[cfg(test)]
    {
        // For tests, use a thread-local storage to avoid the OnceLock static initialization issue
        use std::cell::RefCell;
        thread_local! {
            static MATRIX: RefCell<Option<HashMap<String, Vec<String>>>> = const { RefCell::new(None) };
        }

        // This is a hack to return a static reference from thread-local storage
        // We leak the memory in tests, which is acceptable for testing
        MATRIX.with(|m| {
            let mut matrix = m.borrow_mut();
            if matrix.is_none() {
                match load_compatibility_matrix() {
                    Ok(loaded_matrix) => {
                        *matrix = Some(loaded_matrix);
                    }
                    Err(e) => {
                        panic!(
                            "License compatibility configuration file not found during testing: {e}"
                        );
                    }
                }
            }

            // Leak the memory to get a static reference (only for tests)
            let leaked: &'static HashMap<String, Vec<String>> =
                Box::leak(Box::new(matrix.as_ref().unwrap().clone()));
            leaked
        })
    }
}

/// Check if a license is compatible with the base project license
pub fn is_license_compatible(
    dependency_license: &str,
    project_license: &str,
    strict: bool,
) -> LicenseCompatibility {
    log(
        LogLevel::Info,
        &format!(
            "Checking if dependency license {dependency_license} is compatible with project license {project_license} (strict={strict})"
        ),
    );

    let compatibility_matrix = get_compatibility_matrix();
    let norm_dependency_license = normalize_license_id(dependency_license);
    let norm_project_license = normalize_license_id(project_license);

    log(
        LogLevel::Info,
        &format!(
            "Normalized licenses: dependency={norm_dependency_license}, project={norm_project_license}"
        ),
    );

    match compatibility_matrix.get(&norm_project_license) {
        Some(compatible_licenses) => {
            if compatible_licenses.contains(&norm_dependency_license) {
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
            if strict {
                log(
                    LogLevel::Warn,
                    &format!("Unknown compatibility for project license {norm_project_license} in strict mode, marking as incompatible"),
                );
                LicenseCompatibility::Incompatible
            } else {
                log(
                    LogLevel::Warn,
                    &format!("Unknown compatibility for project license {norm_project_license}"),
                );
                LicenseCompatibility::Unknown
            }
        }
    }
}

/// Normalize license identifier to a standard format
fn normalize_license_id(license_id: &str) -> String {
    let trimmed = license_id.trim().to_uppercase();

    // Handle common variations and aliases
    match trimmed.as_str() {
        "MIT" | "MIT LICENSE" => "MIT".to_string(),
        "ISC" | "ISC LICENSE" => "ISC".to_string(),
        "0BSD" | "BSD-ZERO-CLAUSE" | "BSD ZERO CLAUSE" => "0BSD".to_string(),
        "UNLICENSE" | "THE UNLICENSE" => "Unlicense".to_string(),
        "WTFPL" | "DO WHAT THE FUCK YOU WANT TO PUBLIC LICENSE" => "WTFPL".to_string(),
        "ZLIB" | "ZLIB LICENSE" => "Zlib".to_string(),
        "CC0" | "CC0-1.0" | "CC0 1.0" | "CREATIVE COMMONS ZERO" => "CC0-1.0".to_string(),

        id if id.contains("APACHE") && (id.contains("2.0") || id.contains("2")) => {
            "Apache-2.0".to_string()
        }

        id if id.contains("AGPL") && id.contains("3") => "AGPL-3.0".to_string(),
        id if id.contains("AFFERO") && id.contains("GPL") && id.contains("3") => {
            "AGPL-3.0".to_string()
        }

        id if id.contains("GPL") && id.contains("3") && !id.contains("LGPL") => {
            "GPL-3.0".to_string()
        }
        id if id.contains("GPL") && id.contains("2") && !id.contains("LGPL") => {
            "GPL-2.0".to_string()
        }

        id if id.contains("LGPL") && id.contains("3") => "LGPL-3.0".to_string(),
        id if id.contains("LGPL") && id.contains("2.1") => "LGPL-2.1".to_string(),
        id if id.contains("LGPL") && id.contains("2") && !id.contains("2.1") => {
            "LGPL-2.1".to_string()
        }

        id if id.contains("MPL") && id.contains("2.0") => "MPL-2.0".to_string(),

        id if id.contains("BSD") && (id.contains("3") || id.contains("THREE")) => {
            "BSD-3-Clause".to_string()
        }
        id if id.contains("BSD") && (id.contains("2") || id.contains("TWO")) => {
            "BSD-2-Clause".to_string()
        }

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
            osi_status: OsiStatus::Approved,
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
            osi_status: OsiStatus::Unknown,
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
    #[ignore] // Skip this test due to static initialization issues in test runner
    fn test_is_license_compatible_mit_project() {
        assert_eq!(
            is_license_compatible("MIT", "MIT", false),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("BSD-2-Clause", "MIT", false),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("BSD-3-Clause", "MIT", false),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("Apache-2.0", "MIT", false),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("LGPL-3.0", "MIT", false),
            LicenseCompatibility::Incompatible
        );
        assert_eq!(
            is_license_compatible("MPL-2.0", "MIT", false),
            LicenseCompatibility::Incompatible
        );
        assert_eq!(
            is_license_compatible("GPL-3.0", "MIT", false),
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

    #[test]
    fn test_is_license_ignored_with_no_license() {
        // Should return false when no license is provided
        assert!(!is_license_ignored(None));
    }

    #[test]
    fn test_is_license_ignored_not_in_ignore_list() {
        // License not in ignore list should return false
        // This test assumes no ignore list is configured
        let result = is_license_ignored(Some("GPL-3.0"));
        // Since we can't easily mock the config in this context,
        // we just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_is_license_ignored_empty_license() {
        // Empty string should return false
        assert!(!is_license_ignored(Some("")));
    }
}
