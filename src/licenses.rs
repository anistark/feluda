use cargo_metadata::Package;
use rayon::prelude::*;
use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use toml::Value as TomlValue;

use crate::cli;
use crate::config;
use crate::debug::{log, log_debug, log_error, FeludaResult, LogLevel};

// This is used to deserialize the license files from the choosealicense.com repository
#[derive(Debug, Deserialize, Serialize)]
struct License {
    title: String,            // The full name of the license
    spdx_id: String,          // The SPDX identifier for the license
    permissions: Vec<String>, // A list of permissions granted by the license
    conditions: Vec<String>,  // A list of conditions that must be met under the license
    limitations: Vec<String>, // A list of limitations imposed by the license
}

// License compatibility enum to categorize compatibility status
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

// This struct is used to store information about the licenses of dependencies
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

pub fn analyze_rust_licenses(packages: Vec<Package>) -> Vec<LicenseInfo> {
    if packages.is_empty() {
        log(
            LogLevel::Warn,
            "No Rust packages found for license analysis",
        );
        return vec![];
    }

    log(
        LogLevel::Info,
        format!("Analyzing licenses for {} Rust packages", packages.len()),
    );

    let known_licenses = match fetch_licenses_from_github() {
        Ok(licenses) => {
            log(
                LogLevel::Info,
                format!("Fetched {} known licenses from GitHub", licenses.len()),
            );
            licenses
        }
        Err(err) => {
            log_error("Failed to fetch licenses from GitHub", &err);
            HashMap::new()
        }
    };

    packages
        .par_iter()
        .map(|package| {
            log(
                LogLevel::Info,
                format!("Analyzing package: {} ({})", package.name, package.version),
            );

            let is_restrictive = is_license_restrictive(&package.license, &known_licenses);

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    format!(
                        "Restrictive license found: {:?} for {}",
                        package.license, package.name
                    ),
                );
            }

            LicenseInfo {
                name: package.name.to_string(),
                version: package.version.to_string(),
                license: package.license.clone(),
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown, // Initial value, updated later
            }
        })
        .collect()
}

#[derive(Deserialize, Serialize, Debug)]
struct PackageJson {
    dependencies: Option<HashMap<String, String>>,
    dev_dependencies: Option<HashMap<String, String>>,
}

impl PackageJson {
    fn get_all_dependencies(self) -> HashMap<String, String> {
        let mut all_dependencies: HashMap<String, String> = HashMap::new();
        if let Some(deps) = self.dev_dependencies {
            all_dependencies.extend(deps)
        };
        if let Some(deps) = self.dependencies {
            all_dependencies.extend(deps)
        };
        all_dependencies
    }
}

/// Analyze the licenses of Python dependencies
pub fn analyze_python_licenses(package_file_path: impl Into<String>) -> Vec<LicenseInfo> {
    let package_file_path = package_file_path.into();
    let mut licenses = Vec::new();
    log(
        LogLevel::Info,
        format!("Analyzing Python dependencies from: {}", package_file_path),
    );

    let known_licenses = match fetch_licenses_from_github() {
        Ok(licenses) => {
            log(
                LogLevel::Info,
                format!("Fetched {} known licenses from GitHub", licenses.len()),
            );
            licenses
        }
        Err(err) => {
            log_error("Failed to fetch licenses from GitHub", &err);
            HashMap::new()
        }
    };

    // Check if it's a pyproject.toml file
    if package_file_path.ends_with("pyproject.toml") {
        match fs::read_to_string(package_file_path) {
            Ok(content) => {
                match toml::from_str::<TomlValue>(&content) {
                    Ok(config) => {
                        if let Some(project) = config.as_table().and_then(|t| t.get("project")) {
                            if let Some(deps) = project
                                .as_table()
                                .and_then(|t| t.get("dependencies"))
                                .and_then(|d| d.as_array())
                            {
                                log(
                                    LogLevel::Info,
                                    format!("Found {} Python dependencies", deps.len()),
                                );
                                log_debug("Dependencies", deps);

                                for dep in deps {
                                    if let Some(dep_str) = dep.as_str() {
                                        log(
                                            LogLevel::Info,
                                            format!("Processing dependency: {}", dep_str),
                                        );

                                        // Split on typical comparison operators (>=, ==, etc.)
                                        let (name, version) = if let Some((n, v)) = dep_str
                                            .split_once("==")
                                            .or_else(|| dep_str.split_once(">="))
                                            .or_else(|| dep_str.split_once(">"))
                                            .or_else(|| dep_str.split_once("~="))
                                            .or_else(|| dep_str.split_once("<="))
                                            .or_else(|| dep_str.split_once("<"))
                                        {
                                            (n.trim(), v.trim())
                                        } else {
                                            (dep_str.trim(), "latest")
                                        };

                                        let version_clean = version
                                            .trim_matches('"')
                                            .replace("^", "")
                                            .replace("~", "");

                                        log(
                                            LogLevel::Info,
                                            format!(
                                                "Fetching license for Python dependency: {} ({})",
                                                name, version_clean
                                            ),
                                        );

                                        let license_result = fetch_license_for_python_dependency(
                                            name,
                                            &version_clean,
                                        );
                                        let license = Some(license_result);
                                        let is_restrictive =
                                            is_license_restrictive(&license, &known_licenses);

                                        if is_restrictive {
                                            log(
                                                LogLevel::Warn,
                                                format!(
                                                    "Restrictive license found: {:?} for {}",
                                                    license, name
                                                ),
                                            );
                                        }

                                        licenses.push(LicenseInfo {
                                            name: name.to_string(),
                                            version: version_clean,
                                            license,
                                            is_restrictive,
                                            compatibility: LicenseCompatibility::Unknown,
                                        });
                                    }
                                }
                            } else {
                                log(
                                    LogLevel::Warn,
                                    "Failed to find dependencies in pyproject.toml",
                                );
                            }
                        } else {
                            log(
                                LogLevel::Warn,
                                "No 'project' section found in pyproject.toml",
                            );
                        }
                    }
                    Err(err) => {
                        log_error("Failed to parse pyproject.toml", &err);
                    }
                }
            }
            Err(err) => {
                log_error("Failed to read pyproject.toml file", &err);
            }
        }
    } else {
        // Handle requirements.txt format
        log(LogLevel::Info, "Processing requirements.txt format");

        match File::open(package_file_path) {
            Ok(file) => {
                let reader = io::BufReader::new(file);
                let mut dep_count = 0;

                for line_result in reader.lines() {
                    match line_result {
                        Ok(line) => {
                            let parts: Vec<&str> = line.split("==").collect();
                            if parts.len() >= 2 {
                                let name = parts[0].to_string();
                                let version = parts[1].to_string();

                                log(
                                    LogLevel::Info,
                                    format!("Processing requirement: {} {}", name, version),
                                );

                                let license_result =
                                    fetch_license_for_python_dependency(&name, &version);
                                let license = Some(license_result);
                                let is_restrictive =
                                    is_license_restrictive(&license, &known_licenses);

                                if is_restrictive {
                                    log(
                                        LogLevel::Warn,
                                        format!(
                                            "Restrictive license found: {:?} for {}",
                                            license, name
                                        ),
                                    );
                                }

                                licenses.push(LicenseInfo {
                                    name,
                                    version,
                                    license,
                                    is_restrictive,
                                    compatibility: LicenseCompatibility::Unknown,
                                });

                                dep_count += 1;
                            } else {
                                log(
                                    LogLevel::Warn,
                                    format!("Invalid requirement line: {}", line),
                                );
                            }
                        }
                        Err(err) => {
                            log_error("Failed to read line from requirements.txt", &err);
                        }
                    }
                }

                log(
                    LogLevel::Info,
                    format!("Processed {} requirements from requirements.txt", dep_count),
                );
            }
            Err(err) => {
                log_error("Failed to open requirements.txt file", &err);
            }
        }
    }

    log(
        LogLevel::Info,
        format!("Found {} Python dependencies with licenses", licenses.len()),
    );
    licenses
}

/// Analyze the licenses of JavaScript dependencies
pub fn analyze_js_licenses(package_json_path: impl Into<String>) -> Vec<LicenseInfo> {
    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";

    let package_json_path = package_json_path.into();
    log(
        LogLevel::Info,
        format!(
            "Analyzing JavaScript dependencies from: {}",
            package_json_path
        ),
    );

    let content = match fs::read_to_string(package_json_path) {
        Ok(content) => content,
        Err(err) => {
            log_error("Failed to read package.json file", &err);
            return Vec::new();
        }
    };

    let package_json: PackageJson = match serde_json::from_str(&content) {
        Ok(parsed) => parsed,
        Err(err) => {
            log_error("Failed to parse package.json", &err);
            return Vec::new();
        }
    };

    let all_dependencies = package_json.get_all_dependencies();
    log(
        LogLevel::Info,
        format!("Found {} JavaScript dependencies", all_dependencies.len()),
    );

    let known_licenses = match fetch_licenses_from_github() {
        Ok(licenses) => {
            log(
                LogLevel::Info,
                format!("Fetched {} known licenses from GitHub", licenses.len()),
            );
            licenses
        }
        Err(err) => {
            log_error("Failed to fetch licenses from GitHub", &err);
            HashMap::new()
        }
    };

    all_dependencies
        .par_iter()
        .map(|(name, version)| {
            log(
                LogLevel::Info,
                format!("Checking license for JS dependency: {} ({})", name, version),
            );

            let output = match Command::new(NPM)
                .arg("view")
                .arg(name)
                .arg("version")
                .arg(version)
                .arg("license")
                .output()
            {
                Ok(output) => output,
                Err(err) => {
                    log_error(format!("Failed to execute npm command for {}", name), &err);
                    return LicenseInfo {
                        name: name.clone(),
                        version: version.clone(),
                        license: Some("Unknown (npm command failed)".to_string()),
                        is_restrictive: true, // Assume restrictive if we can't determine
                        compatibility: LicenseCompatibility::Unknown,
                    };
                }
            };

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log(
                    LogLevel::Error,
                    format!("npm command failed for {}: {}", name, stderr),
                );
                return LicenseInfo {
                    name: name.clone(),
                    version: version.clone(),
                    license: Some("Unknown (npm command failed)".to_string()),
                    is_restrictive: true, // Assume restrictive if we can't determine
                    compatibility: LicenseCompatibility::Unknown,
                };
            }

            let output_str = String::from_utf8_lossy(&output.stdout);
            let license = output_str
                .lines()
                .find(|line| line.starts_with("license ="))
                .map(|line| {
                    line.replace("license =", "")
                        .replace("\'", "")
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| "No License".to_string());

            log(
                LogLevel::Info,
                format!("License for {} ({}): {}", name, version, license),
            );

            let is_restrictive = is_license_restrictive(&Some(license.clone()), &known_licenses);

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    format!("Restrictive license found: {} for {}", license, name),
                );
            }

            LicenseInfo {
                name: name.clone(),
                version: version.clone(),
                license: Some(license),
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
            }
        })
        .collect()
}

// Structure to hold license details for Go
#[derive(Debug)]
pub struct GoPackages {
    name: String,
    version: String,
}

/// Analyze the licenses of Go dependencies
pub fn analyze_go_licenses(go_mod_path: impl Into<String>) -> Vec<LicenseInfo> {
    let go_mod_path = go_mod_path.into();
    log(
        LogLevel::Info,
        format!("Analyzing Go dependencies from: {}", go_mod_path),
    );

    let known_licenses = match fetch_licenses_from_github() {
        Ok(licenses) => {
            log(
                LogLevel::Info,
                format!("Fetched {} known licenses from GitHub", licenses.len()),
            );
            licenses
        }
        Err(err) => {
            log_error("Failed to fetch licenses from GitHub", &err);
            HashMap::new()
        }
    };

    let content = match fs::read_to_string(&go_mod_path) {
        Ok(content) => content,
        Err(err) => {
            log_error(
                format!("Failed to read go.mod file: {}", go_mod_path),
                &err,
            );
            return Vec::new();
        }
    };

    let dependencies = get_go_dependencies(content);
    log(
        LogLevel::Info,
        format!("Found {} Go dependencies", dependencies.len()),
    );
    log_debug("Go dependencies", &dependencies);

    dependencies
        .par_iter()
        .map(|dependency| -> LicenseInfo {
            let name = dependency.name.clone();
            let version = dependency.version.clone();

            log(
                LogLevel::Info,
                format!("Fetching license for Go dependency: {} ({})", name, version),
            );

            let license_result = fetch_license_for_go_dependency(name.as_str(), version.as_str());
            let license = Some(license_result);

            let is_restrictive = is_license_restrictive(&license, &known_licenses);

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    format!("Restrictive license found: {:?} for {}", license, name),
                );
            }

            LicenseInfo {
                name,
                version,
                license,
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
            }
        })
        .collect()
}

pub fn get_go_dependencies(content_string: impl Into<String>) -> Vec<GoPackages> {
    log(LogLevel::Info, "Parsing Go dependencies");

    let content_string = content_string.into();
    let re_comment = match Regex::new(r"(?m)^(.*?)\s*(//|#).*?$") {
        Ok(re) => re,
        Err(err) => {
            log_error("Failed to compile comment regex", &err);
            return Vec::new();
        }
    };

    // Removes everything after // or #
    let cleaned = re_comment.replace_all(content_string.as_str(), "$1");

    let re = match Regex::new(
        r"require\s*(?:\(\s*)?((?:[\w./-]+\s+v[\d][\w\d.-]+(?:-\w+)?(?:\+\w+)?\s*)+)\)?",
    ) {
        Ok(re) => re,
        Err(err) => {
            log_error("Failed to compile require regex", &err);
            return Vec::new();
        }
    };

    let re_dependency = match Regex::new(r"([\w./-]+)\s+(v[\d]+(?:\.\d+)*(?:-\S+)?)") {
        Ok(re) => re,
        Err(err) => {
            log_error("Failed to compile dependency regex", &err);
            return Vec::new();
        }
    };

    let mut dependency = vec![];
    for cap in re.captures_iter(&cleaned) {
        let dependency_block = &cap[1];
        log_debug("Dependency block", &dependency_block);

        for dep_cap in re_dependency.captures_iter(dependency_block) {
            let name = dep_cap[1].to_string();
            let version = dep_cap[2].to_string();

            log(
                LogLevel::Info,
                format!("Found Go dependency: {} ({})", name, version),
            );

            dependency.push(GoPackages { name, version });
        }
    }

    log(
        LogLevel::Info,
        format!("Parsed {} Go dependencies", dependency.len()),
    );
    dependency
}

/// Fetch the license for a Python dependency from the Python Package Index (PyPI)
pub fn fetch_license_for_python_dependency(name: impl Into<String>, version: impl Into<String>) -> String {
    let name = name.into();
    let version = version.into();
    let api_url = format!("https://pypi.org/pypi/{}/{}/json", name, version);
    log(
        LogLevel::Info,
        format!("Fetching license from PyPI: {}", api_url),
    );

    match reqwest::blocking::get(&api_url) {
        Ok(response) => {
            let status = response.status();
            log(
                LogLevel::Info,
                format!("PyPI API response status: {}", status),
            );

            if status.is_success() {
                match response.json::<Value>() {
                    Ok(json) => match json["info"]["license"].as_str() {
                        Some(license_str) if !license_str.is_empty() => {
                            log(
                                LogLevel::Info,
                                format!("License found for {}: {}", name, license_str),
                            );
                            license_str.to_string()
                        }
                        _ => {
                            log(
                                LogLevel::Warn,
                                format!("No license found for {} ({})", name, version),
                            );
                            format!("Unknown license for {}: {}", name, version)
                        }
                    },
                    Err(err) => {
                        log_error(
                            format!("Failed to parse JSON for {}: {}", name, version),
                            &err,
                        );
                        String::from("Unknown")
                    }
                }
            } else {
                log(
                    LogLevel::Error,
                    format!("Failed to fetch metadata for {}: HTTP {}", name, status),
                );
                String::from("Unknown")
            }
        }
        Err(err) => {
            log_error(format!("Failed to fetch metadata for {}", name), &err);
            String::from("Unknown")
        }
    }
}

/// Fetch the license for a Go dependency from the Go Package Index (pkg.go.dev)
pub fn fetch_license_for_go_dependency(
    name: impl Into<String>,
    _version: impl Into<String>,
) -> String {
    let name = name.into();
    let _version = _version.into();

    let api_url = format!("https://pkg.go.dev/{}?tab=licenses", name);
    log(
        LogLevel::Info,
        format!("Fetching license from Go Package Index: {}", api_url),
    );

    let client = match Client::builder()
        .user_agent("feluda.anirudha.dev/1")
        .connect_timeout(Duration::from_secs(60))
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            log_error("Failed to build HTTP client", &err);
            return "Unknown".into();
        }
    };

    let mut attempts = 0;
    let max_attempts = 7; // Retry max 7 times. Thala for a reason 🙌
    let wait_time = 12;

    while attempts < max_attempts {
        let response = client
            .get(&api_url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (compatible; Feluda-Bot/1.0; +https://github.com/anistark/feluda)",
            )
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Referer", "https://pkg.go.dev/")
            .send();

        match response {
            Ok(response) => {
                let status = response.status();
                log(
                    LogLevel::Info,
                    format!("Go Package Index API response status: {}", status),
                );

                if status.as_u16() == 429 {
                    log(
                        LogLevel::Warn,
                        format!(
                            "Received 429 Too Many Requests, retrying... (attempt {}/{})",
                            attempts + 1,
                            max_attempts
                        ),
                    );
                    sleep(Duration::from_secs(wait_time));
                    attempts += 1;
                    continue;
                }

                if status.is_success() {
                    match response.text() {
                        Ok(html_content) => {
                            if let Some(license) = extract_license_from_html(&html_content) {
                                log(
                                    LogLevel::Info,
                                    format!("License found for {}: {}", name, license),
                                );
                                return license;
                            } else {
                                log(
                                    LogLevel::Warn,
                                    format!("No license found in HTML for {}", name),
                                );
                            }
                        }
                        Err(err) => {
                            log_error(
                                format!("Failed to extract HTML content for {}", name),
                                &err,
                            );
                        }
                    }
                } else {
                    log(
                        LogLevel::Error,
                        format!("Unexpected HTTP status: {} for {}", status, name),
                    );
                }

                break;
            }
            Err(err) => {
                log_error(format!("Failed to fetch metadata for {}", name), &err);
                break;
            }
        }
    }

    log(
        LogLevel::Warn,
        format!(
            "Unable to determine license for {} after {} attempts",
            name, attempts
        ),
    );
    "Unknown".into()
}

/// Extract license information from the HTML content
fn extract_license_from_html(html: &str) -> Option<String> {
    log(LogLevel::Info, "Extracting license from HTML content");

    let document = Html::parse_document(html);

    // Select the <section> with class "License"
    let section_selector = match Selector::parse("section.License") {
        Ok(selector) => selector,
        Err(err) => {
            log_error("Failed to parse section selector", &err);
            return None;
        }
    };

    let div_selector = match Selector::parse("h2.go-textTitle div") {
        Ok(selector) => selector,
        Err(err) => {
            log_error("Failed to parse div selector", &err);
            return None;
        }
    };

    if let Some(section) = document.select(&section_selector).next() {
        if let Some(div) = section.select(&div_selector).next() {
            let license_text = div.text().collect::<Vec<_>>().join(" ").trim().to_string();
            log(
                LogLevel::Info,
                format!("License found in HTML: {}", license_text),
            );
            return Some(license_text);
        } else {
            log(LogLevel::Warn, "Found section but no license div");
        }
    } else {
        log(LogLevel::Warn, "No license section found in HTML");
    }

    None
}

/// Check if a license is compatible with the base project license
pub fn is_license_compatible(
    dependency_license: &str,
    project_license: &str,
) -> LicenseCompatibility {
    log(
        LogLevel::Info,
        format!(
            "Checking if license {} is compatible with project license {}",
            dependency_license, project_license
        ),
    );

    // Define a compatibility matrix using a HashMap
    // This is a simplified model of license compatibility
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
        format!(
            "Normalized licenses: dependency={}, project={}",
            norm_dependency_license, norm_project_license
        ),
    );

    // Check compatibility based on the matrix
    match compatibility_matrix.get(norm_project_license.as_str()) {
        Some(compatible_licenses) => {
            if compatible_licenses.contains(&norm_dependency_license.as_str()) {
                log(
                    LogLevel::Info,
                    format!(
                        "License {} is compatible with project license {}",
                        norm_dependency_license, norm_project_license
                    ),
                );
                LicenseCompatibility::Compatible
            } else {
                log(
                    LogLevel::Warn,
                    format!(
                        "License {} may be incompatible with project license {}",
                        norm_dependency_license, norm_project_license
                    ),
                );
                LicenseCompatibility::Incompatible
            }
        }
        None => {
            log(
                LogLevel::Warn,
                format!(
                    "Unknown compatibility for project license {}",
                    norm_project_license
                ),
            );
            LicenseCompatibility::Unknown
        }
    }
}

/// Normalize license identifier to a standard format
fn normalize_license_id(license_id: &str) -> String {
    // Handle common variations and abbreviations
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

/// Detect the project's license from the repository
pub fn detect_project_license(project_path: impl Into<String>) -> FeludaResult<Option<String>> {
    let project_path = project_path.into();
    log(
        LogLevel::Info,
        format!("Detecting license for project at path: {}", project_path),
    );

    // Check LICENSE file
    let license_paths = [
        Path::new(&project_path).join("LICENSE"),
        Path::new(&project_path).join("LICENSE.txt"),
        Path::new(&project_path).join("LICENSE.md"),
        Path::new(&project_path).join("license"),
        Path::new(&project_path).join("COPYING"),
    ];

    for license_path in &license_paths {
        if license_path.exists() {
            log(
                LogLevel::Info,
                format!("Found license file: {}", license_path.display()),
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
                    log_error(
                        format!("Failed to read license file: {}", license_path.display()),
                        &err,
                    );
                }
            }
        }
    }

    // Check package.json for Node.js projects
    let package_json_path = Path::new(&project_path).join("package.json");
    if package_json_path.exists() {
        log(
            LogLevel::Info,
            format!("Found package.json at {}", package_json_path.display()),
        );

        match fs::read_to_string(&package_json_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(json) => {
                    if let Some(license) = json.get("license").and_then(|l| l.as_str()) {
                        log(
                            LogLevel::Info,
                            format!("Detected license from package.json: {}", license),
                        );
                        return Ok(Some(license.to_string()));
                    }
                }
                Err(err) => {
                    log_error("Failed to parse package.json", &err);
                }
            },
            Err(err) => {
                log_error(
                    format!(
                        "Failed to read package.json: {}",
                        package_json_path.display()
                    ),
                    &err,
                );
            }
        }
    }

    // Check Cargo.toml for Rust projects
    let cargo_toml_path = Path::new(&project_path).join("Cargo.toml");
    if cargo_toml_path.exists() {
        log(
            LogLevel::Info,
            format!("Found Cargo.toml at {}", cargo_toml_path.display()),
        );

        match fs::read_to_string(&cargo_toml_path) {
            Ok(content) => match toml::from_str::<TomlValue>(&content) {
                Ok(toml) => {
                    if let Some(package) = toml.as_table().and_then(|t| t.get("package")) {
                        if let Some(license) = package.get("license").and_then(|l| l.as_str()) {
                            log(
                                LogLevel::Info,
                                format!("Detected license from Cargo.toml: {}", license),
                            );
                            return Ok(Some(license.to_string()));
                        }
                    }
                }
                Err(err) => {
                    log_error("Failed to parse Cargo.toml", &err);
                }
            },
            Err(err) => {
                log_error(
                    format!("Failed to read Cargo.toml: {}", cargo_toml_path.display()),
                    &err,
                );
            }
        }
    }

    // Check pyproject.toml for Python projects
    let pyproject_toml_path = Path::new(&project_path).join("pyproject.toml");
    if pyproject_toml_path.exists() {
        log(
            LogLevel::Info,
            format!("Found pyproject.toml at {}", pyproject_toml_path.display()),
        );

        match fs::read_to_string(&pyproject_toml_path) {
            Ok(content) => match toml::from_str::<TomlValue>(&content) {
                Ok(toml) => {
                    if let Some(project) = toml.as_table().and_then(|t| t.get("project")) {
                        if let Some(license_info) = project.get("license") {
                            if let Some(license) = license_info.as_str() {
                                log(
                                    LogLevel::Info,
                                    format!("Detected license from pyproject.toml: {}", license),
                                );
                                return Ok(Some(license.to_string()));
                            } else if let Some(license_table) = license_info.as_table() {
                                if let Some(license_text) =
                                    license_table.get("text").and_then(|t| t.as_str())
                                {
                                    log(
                                        LogLevel::Info,
                                        format!(
                                            "Detected license from pyproject.toml: {}",
                                            license_text
                                        ),
                                    );
                                    return Ok(Some(license_text.to_string()));
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    log_error("Failed to parse pyproject.toml", &err);
                }
            },
            Err(err) => {
                log_error(
                    format!(
                        "Failed to read pyproject.toml: {}",
                        pyproject_toml_path.display()
                    ),
                    &err,
                );
            }
        }
    }

    log(LogLevel::Warn, "No license detected for project");
    Ok(None)
}

fn is_license_restrictive(
    license: &Option<String>,
    known_licenses: &HashMap<String, License>,
) -> bool {
    log(
        LogLevel::Info,
        format!("Checking if license is restrictive: {:?}", license),
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
                    format!("License {} is restrictive due to conditions", license_str),
                );
            } else {
                log(
                    LogLevel::Info,
                    format!("License {} is not restrictive", license_str),
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
                    format!(
                        "License {} matches restrictive pattern in config",
                        license_str
                    ),
                );
            } else {
                log(
                    LogLevel::Info,
                    format!(
                        "License {} does not match any restrictive pattern",
                        license_str
                    ),
                );
            }

            return is_restrictive;
        }
    }

    log(LogLevel::Warn, "No license information available");
    false
}

fn fetch_licenses_from_github() -> FeludaResult<HashMap<String, License>> {
    log(
        LogLevel::Info,
        "Fetching licenses from GitHub choosealicense repository",
    );

    let licenses_url =
        "https://raw.githubusercontent.com/github/choosealicense.com/gh-pages/_licenses/";

    // Use the new loading indicator
    let licenses_map = cli::with_spinner("Fetching licenses from GitHub", |indicator| {
        let mut licenses_map = HashMap::new();
        let mut license_count = 0;

        match reqwest::blocking::get(licenses_url) {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    log(
                        LogLevel::Error,
                        format!("GitHub API returned error status: {}", status),
                    );
                    return licenses_map; // Return empty map on error
                }

                match response.text() {
                    Ok(content) => {
                        indicator.update_progress("parsing license list");

                        let mut license_files = Vec::new();
                        for line in content.lines() {
                            if line.ends_with(".txt") {
                                license_files.push(line.to_string());
                            }
                        }

                        let total_licenses = license_files.len();
                        indicator.update_progress(format!("found {} licenses", total_licenses));

                        for (idx, line) in license_files.iter().enumerate() {
                            let license_name = line.replace(".txt", "");
                            let license_url = format!("{}{}", licenses_url, line);

                            log(
                                LogLevel::Info,
                                format!("Fetching license: {}", license_name),
                            );

                            indicator.update_progress(format!(
                                "processing {}/{}: {}",
                                idx + 1,
                                total_licenses,
                                license_name
                            ));

                            let license_response = match reqwest::blocking::get(&license_url) {
                                Ok(res) => res,
                                Err(err) => {
                                    log_error(
                                        format!(
                                            "Failed to fetch license content for {}",
                                            license_name
                                        ),
                                        &err,
                                    );
                                    continue;
                                }
                            };

                            if !license_response.status().is_success() {
                                log(
                                    LogLevel::Error,
                                    format!(
                                        "Failed to fetch license {}: HTTP {}",
                                        license_name,
                                        license_response.status()
                                    ),
                                );
                                continue;
                            }

                            let license_content = match license_response.text() {
                                Ok(text) => text,
                                Err(err) => {
                                    log_error(
                                        format!(
                                            "Failed to read license content for {}",
                                            license_name
                                        ),
                                        &err,
                                    );
                                    continue;
                                }
                            };

                            match serde_yaml::from_str::<License>(&license_content) {
                                Ok(license) => {
                                    licenses_map.insert(license_name, license);
                                    license_count += 1;
                                }
                                Err(err) => {
                                    log_error(
                                        format!(
                                            "Failed to parse license content for {}",
                                            license_name
                                        ),
                                        &err,
                                    );
                                }
                            }
                        }

                        indicator.update_progress(format!("processed {} licenses", license_count));
                    }
                    Err(err) => {
                        log_error("Failed to read response text", &err);
                    }
                }
            }
            Err(err) => {
                log_error("Failed to fetch licenses list", &err);
            }
        }

        log(
            LogLevel::Info,
            format!("Successfully fetched {} licenses", license_count),
        );
        licenses_map
    });

    Ok(licenses_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use mockall::predicate::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_license_restrictive_with_default_config() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("GPL-3.0".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("MIT".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_license_restrictive_with_toml_config() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["CUSTOM-1.0"]"#,
            )
            .unwrap();

            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("CUSTOM-1.0".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("GPL-3.0".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_license_restrictive_with_env_config() {
        temp_env::with_vars(
            vec![("FELUDA_LICENSES_RESTRICTIVE", Some(r#"["ENV-LICENSE"]"#))],
            || {
                let dir = setup();
                std::env::set_current_dir(dir.path()).unwrap();

                let known_licenses = HashMap::new();
                assert!(is_license_restrictive(
                    &Some("ENV-LICENSE".to_string()),
                    &known_licenses
                ));
                assert!(!is_license_restrictive(
                    &Some("GPL-3.0".to_string()),
                    &known_licenses
                ));
            },
        );
    }

    #[test]
    fn test_env_overrides_toml() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", Some("[\"ENV-1.0\"]"), || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["TOML-1.0", "TOML-2.0"]"#,
            )
            .unwrap();

            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("ENV-1.0".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("TOML-1.0".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_license_restrictive_no_license() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("No License".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_license_restrictive_with_known_licenses() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            let mut known_licenses = HashMap::new();
            known_licenses.insert(
                "TEST-LICENSE".to_string(),
                License {
                    title: "Test License".to_string(),
                    spdx_id: "TEST-LICENSE".to_string(),
                    permissions: vec![],
                    conditions: vec!["source-disclosure".to_string()],
                    limitations: vec![],
                },
            );

            assert!(is_license_restrictive(
                &Some("TEST-LICENSE".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("OTHER-LICENSE".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_extract_license_from_html() {
        let html_content = r#"
            <html>
                <body>
                    <section class="License">
                        <h2 class="go-textTitle">
                            <div>MIT</div>
                        </h2>
                    </section>
                </body>
            </html>
        "#;

        let license = extract_license_from_html(html_content);
        assert_eq!(license, Some("MIT".to_string()));
    }

    #[test]
    fn test_extract_license_from_html_no_license() {
        let html_content = r#"
            <html>
                <body>
                    <span class="go-Main-headerDetailItem" data-test-id="UnitHeader-licenses">
                    </span>
                </body>
            </html>
        "#;
        let license = extract_license_from_html(html_content);
        assert_eq!(license, None);
    }

    pub trait HttpClient {
        #[allow(dead_code)]
        fn get(&self, url: &str) -> Result<reqwest::blocking::Response, reqwest::Error>;
    }

    mock! {
        pub HttpClient {
            fn get(&self, url: &str) -> Result<reqwest::blocking::Response, reqwest::Error>;
        }
    }

    impl HttpClient for MockHttpClient {
        fn get(&self, url: &str) -> Result<reqwest::blocking::Response, reqwest::Error> {
            self.get(url)
        }
    }

    #[test]
    fn test_fetch_license_for_go_dependency() {
        let mut mock_http_client = MockHttpClient::new();

        mock_http_client
            .expect_get()
            .with(eq("https://pkg.go.dev/github.com/stretchr/testify"))
            .returning(|_| {
                let response = Client::new()
                    .get("https://pkg.go.dev/github.com/stretchr/testify")
                    .send()
                    .unwrap();
                Ok(response)
            });

        let license = fetch_license_for_go_dependency("github.com/stretchr/testify", "v1.7.0");
        assert_eq!(license, "MIT");
    }

    #[test]
    fn test_fetch_license_for_python_dependency() {
        let mut mock_http_client = MockHttpClient::new();

        mock_http_client
            .expect_get()
            .with(eq("https://pypi.org/pypi/requests/2.25.1/json"))
            .returning(|_| {
                let response = Client::new()
                    .get("https://pypi.org/pypi/requests/2.25.1/json")
                    .send()
                    .unwrap();
                Ok(response)
            });

        let license = fetch_license_for_python_dependency("requests", "2.25.1");
        assert_eq!(license, "Apache 2.0");
    }

    #[test]
    fn test_analyze_python_licenses_pyproject_toml() {
        let temp_dir = setup();
        let pyproject_toml_path = temp_dir.path().join("pyproject.toml");

        fs::write(
            &pyproject_toml_path,
            r#"[project]
    name = "test-project"
    version = "0.1.0"
    dependencies = [
        "requests>=2.31.0",
        "flask~=2.0.0"
    ]
    "#,
        )
        .unwrap();

        let result = analyze_python_licenses(pyproject_toml_path.to_str().unwrap());
        assert!(!result.is_empty());
        assert!(result.iter().any(|info| info.name == "requests"));
        assert!(result.iter().any(|info| info.name == "flask"));
    }

    #[test]
    fn test_get_go_dependencies() {
        let content = r#"require (
            github.com/user/repo v1.0.0
            github.com/another/pkg v2.3.4
        )"#;

        let deps = get_go_dependencies(content.to_string());
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "github.com/user/repo");
        assert_eq!(deps[0].version, "v1.0.0");
    }

    #[test]
    fn test_analyze_js_licenses_empty_file() {
        let temp_dir = setup();
        let package_json_path = temp_dir.path().join("package.json");

        fs::write(
            &package_json_path,
            r#"{
                "name": "test-project",
                "version": "1.0.0"
            }"#,
        )
        .unwrap();

        let result = analyze_js_licenses(package_json_path.to_str().unwrap());
        assert!(result.is_empty());
    }

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
    fn test_license_compatibility_equality() {
        assert_eq!(
            LicenseCompatibility::Compatible,
            LicenseCompatibility::Compatible
        );
        assert_ne!(
            LicenseCompatibility::Compatible,
            LicenseCompatibility::Incompatible
        );
        assert_ne!(
            LicenseCompatibility::Unknown,
            LicenseCompatibility::Compatible
        );
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
    fn test_license_info_clone() {
        let info = LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
        };

        let cloned = info.clone();
        assert_eq!(info.name(), cloned.name());
        assert_eq!(info.version(), cloned.version());
        assert_eq!(info.get_license(), cloned.get_license());
    }

    #[test]
    fn test_license_info_debug() {
        let info = LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
        };

        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("test_package"));
        assert!(debug_str.contains("MIT"));
        assert!(debug_str.contains("Compatible"));
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
    fn test_is_license_compatible_apache_project() {
        assert_eq!(
            is_license_compatible("MIT", "Apache-2.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("BSD-2-Clause", "Apache-2.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("BSD-3-Clause", "Apache-2.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("Apache-2.0", "Apache-2.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("GPL-3.0", "Apache-2.0"),
            LicenseCompatibility::Incompatible
        );
        assert_eq!(
            is_license_compatible("LGPL-3.0", "Apache-2.0"),
            LicenseCompatibility::Incompatible
        );
    }

    #[test]
    fn test_is_license_compatible_gpl_project() {
        assert_eq!(
            is_license_compatible("MIT", "GPL-3.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("BSD-2-Clause", "GPL-3.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("BSD-3-Clause", "GPL-3.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("LGPL-3.0", "GPL-3.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("GPL-3.0", "GPL-3.0"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("Apache-2.0", "GPL-3.0"),
            LicenseCompatibility::Incompatible
        );
    }

    #[test]
    fn test_is_license_compatible_unknown_project() {
        assert_eq!(
            is_license_compatible("MIT", "UNKNOWN-LICENSE"),
            LicenseCompatibility::Unknown
        );
        assert_eq!(
            is_license_compatible("GPL-3.0", "CUSTOM-LICENSE"),
            LicenseCompatibility::Unknown
        );
    }

    #[test]
    fn test_is_license_compatible_case_insensitive() {
        assert_eq!(
            is_license_compatible("mit", "MIT"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("MIT", "mit"),
            LicenseCompatibility::Compatible
        );
        assert_eq!(
            is_license_compatible("apache-2.0", "MIT"),
            LicenseCompatibility::Compatible
        );
    }

    #[test]
    fn test_detect_project_license_mit_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
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
    fn test_detect_project_license_apache_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let license_path = temp_dir.path().join("LICENSE");

        std::fs::write(&license_path, "Apache License\nVersion 2.0, January 2004").unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("Apache-2.0".to_string()));
    }

    #[test]
    fn test_detect_project_license_gpl_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let license_path = temp_dir.path().join("LICENSE");

        std::fs::write(
            &license_path,
            "GNU GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007",
        )
        .unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("GPL-3.0".to_string()));
    }

    #[test]
    fn test_detect_project_license_bsd_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let license_path = temp_dir.path().join("LICENSE");

        std::fs::write(&license_path, "BSD 3-Clause License\n\nRedistribution and use in source and binary forms... Neither the name of the copyright holder...").unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("BSD-3-Clause".to_string()));
    }

    #[test]
    fn test_detect_project_license_lgpl_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let license_path = temp_dir.path().join("LICENSE");

        std::fs::write(
            &license_path,
            "GNU LESSER GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007",
        )
        .unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("LGPL-3.0".to_string()));
    }

    #[test]
    fn test_detect_project_license_mpl_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let license_path = temp_dir.path().join("LICENSE");

        std::fs::write(
            &license_path,
            "Mozilla Public License Version 2.0\n\n1. Definitions",
        )
        .unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("MPL-2.0".to_string()));
    }

    #[test]
    fn test_detect_project_license_package_json() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let package_json_path = temp_dir.path().join("package.json");

        std::fs::write(&package_json_path, r#"{"name": "test", "license": "MIT"}"#).unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("MIT".to_string()));
    }

    #[test]
    fn test_detect_project_license_package_json_invalid() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let package_json_path = temp_dir.path().join("package.json");

        std::fs::write(&package_json_path, "invalid json content").unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_project_license_cargo_toml() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let cargo_toml_path = temp_dir.path().join("Cargo.toml");

        std::fs::write(
            &cargo_toml_path,
            r#"[package]
name = "test"
version = "0.1.0"
license = "MIT"
"#,
        )
        .unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("MIT".to_string()));
    }

    #[test]
    fn test_detect_project_license_cargo_toml_invalid() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let cargo_toml_path = temp_dir.path().join("Cargo.toml");

        std::fs::write(&cargo_toml_path, "invalid toml [[[").unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_project_license_pyproject_toml() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let pyproject_toml_path = temp_dir.path().join("pyproject.toml");

        std::fs::write(
            &pyproject_toml_path,
            r#"[project]
name = "test"
version = "0.1.0"
license = "MIT"
"#,
        )
        .unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("MIT".to_string()));
    }

    #[test]
    fn test_detect_project_license_pyproject_toml_license_table() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let pyproject_toml_path = temp_dir.path().join("pyproject.toml");

        std::fs::write(
            &pyproject_toml_path,
            r#"[project]
name = "test"
version = "0.1.0"

[project.license]
text = "Apache-2.0"
"#,
        )
        .unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("Apache-2.0".to_string()));
    }

    #[test]
    fn test_detect_project_license_no_license() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_project_license_multiple_files_precedence() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Create multiple license sources
        std::fs::write(
            temp_dir.path().join("LICENSE"),
            "MIT License\n\nPermission is hereby granted...",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("package.json"),
            r#"{"license": "Apache-2.0"}"#,
        )
        .unwrap();

        // LICENSE file should take precedence
        let result = detect_project_license(temp_dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result, Some("MIT".to_string()));
    }

    #[test]
    fn test_package_json_get_all_dependencies() {
        let package_json = PackageJson {
            dependencies: Some({
                let mut deps = std::collections::HashMap::new();
                deps.insert("dep1".to_string(), "1.0.0".to_string());
                deps.insert("dep2".to_string(), "2.0.0".to_string());
                deps
            }),
            dev_dependencies: Some({
                let mut dev_deps = std::collections::HashMap::new();
                dev_deps.insert("dev_dep1".to_string(), "1.0.0".to_string());
                dev_deps
            }),
        };

        let all_deps = package_json.get_all_dependencies();
        assert_eq!(all_deps.len(), 3);
        assert!(all_deps.contains_key("dep1"));
        assert!(all_deps.contains_key("dep2"));
        assert!(all_deps.contains_key("dev_dep1"));
        assert_eq!(all_deps.get("dep1"), Some(&"1.0.0".to_string()));
        assert_eq!(all_deps.get("dev_dep1"), Some(&"1.0.0".to_string()));
    }

    #[test]
    fn test_package_json_only_dev_dependencies() {
        let package_json = PackageJson {
            dependencies: None,
            dev_dependencies: Some(
                [("dev_dep1".to_string(), "1.0.0".to_string())]
                    .iter()
                    .cloned()
                    .collect(),
            ),
        };

        let all_deps = package_json.get_all_dependencies();
        assert_eq!(all_deps.len(), 1);
        assert!(all_deps.contains_key("dev_dep1"));
    }

    #[test]
    fn test_go_packages_debug() {
        let go_package = GoPackages {
            name: "github.com/test/package".to_string(),
            version: "v1.0.0".to_string(),
        };

        let debug_str = format!("{:?}", go_package);
        assert!(debug_str.contains("github.com/test/package"));
        assert!(debug_str.contains("v1.0.0"));
    }

    #[test]
    fn test_analyze_rust_licenses_empty() {
        let packages = vec![];
        let result = analyze_rust_licenses(packages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_go_dependencies_single_require() {
        let content = "require github.com/test/pkg v1.0.0".to_string();
        let deps = get_go_dependencies(content);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "github.com/test/pkg");
        assert_eq!(deps[0].version, "v1.0.0");
    }

    #[test]
    fn test_get_go_dependencies_with_comments() {
        let content = r#"require (
    github.com/user/repo v1.0.0 // This is a comment
    github.com/another/pkg v2.0.0 # This is also a comment
)"#
        .to_string();

        let deps = get_go_dependencies(content);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "github.com/user/repo");
        assert_eq!(deps[1].name, "github.com/another/pkg");
    }

    #[test]
    fn test_get_go_dependencies_complex_versions() {
        let content = r#"require (
    github.com/user/repo v1.2.3-beta+build
    github.com/another/pkg v2.0.0-rc.1
    github.com/third/mod v0.1.0-alpha
)"#
        .to_string();

        let deps = get_go_dependencies(content);
        assert_eq!(deps.len(), 3);
        assert_eq!(deps[0].version, "v1.2.3-beta+build");
        assert_eq!(deps[1].version, "v2.0.0-rc.1");
        assert_eq!(deps[2].version, "v0.1.0-alpha");
    }

    #[test]
    fn test_get_go_dependencies_mixed_syntax() {
        let content = r#"require (
    github.com/user/repo v1.0.0
)

require github.com/single/pkg v2.0.0

require (github.com/another/pkg v3.0.0)
"#
        .to_string();

        let deps = get_go_dependencies(content);
        assert_eq!(deps.len(), 3);
        assert_eq!(deps[0].name, "github.com/user/repo");
        assert_eq!(deps[1].name, "github.com/single/pkg");
        assert_eq!(deps[2].name, "github.com/another/pkg");
    }

    #[test]
    fn test_get_go_dependencies_empty_content() {
        let content = "".to_string();
        let deps = get_go_dependencies(content);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_get_go_dependencies_no_require() {
        let content = r#"module test

go 1.19

// No require section
"#
        .to_string();

        let deps = get_go_dependencies(content);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_extract_license_from_html_apache() {
        let html_content = r#"
        <html>
            <body>
                <section class="License">
                    <h2 class="go-textTitle">
                        <div>Apache-2.0</div>
                    </h2>
                </section>
            </body>
        </html>
    "#;

        let license = extract_license_from_html(html_content);
        assert_eq!(license, Some("Apache-2.0".to_string()));
    }

    #[test]
    fn test_extract_license_from_html_malformed() {
        let html_content = "<invalid html>";
        let license = extract_license_from_html(html_content);
        assert_eq!(license, None);
    }

    #[test]
    fn test_analyze_python_licenses_empty_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let requirements_path = temp_dir.path().join("requirements.txt");

        std::fs::write(&requirements_path, "").unwrap();

        let result = analyze_python_licenses(requirements_path.to_str().unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_analyze_python_licenses_invalid_format() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let requirements_path = temp_dir.path().join("requirements.txt");

        std::fs::write(
            &requirements_path,
            "invalid-line-without-version\nanother-invalid",
        )
        .unwrap();

        let result = analyze_python_licenses(requirements_path.to_str().unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_analyze_js_licenses_missing_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let package_json_path = temp_dir.path().join("nonexistent.json");

        let result = analyze_js_licenses(package_json_path.to_str().unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_analyze_go_licenses_missing_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let go_mod_path = temp_dir.path().join("nonexistent.mod");

        let result = analyze_go_licenses(go_mod_path.to_str().unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_license_struct_serialization() {
        let license = License {
            title: "MIT License".to_string(),
            spdx_id: "MIT".to_string(),
            permissions: vec!["commercial-use".to_string(), "modifications".to_string()],
            conditions: vec!["include-copyright".to_string()],
            limitations: vec!["liability".to_string(), "warranty".to_string()],
        };

        // Test serialization
        let json = serde_json::to_string(&license).unwrap();
        assert!(json.contains("MIT License"));
        assert!(json.contains("commercial-use"));

        // Test deserialization
        let deserialized: License = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, "MIT License");
        assert_eq!(deserialized.spdx_id, "MIT");
        assert_eq!(deserialized.permissions.len(), 2);
    }

    #[test]
    fn test_fetch_license_for_python_dependency_error_handling() {
        // Test with a definitely non-existent package
        let result =
            fetch_license_for_python_dependency("definitely_nonexistent_package_12345", "1.0.0");
        assert!(result.contains("Unknown") || result.contains("nonexistent"));
    }

    #[test]
    fn test_fetch_license_for_go_dependency_error_handling() {
        // Test with invalid package name
        let result = fetch_license_for_go_dependency("invalid/package/name", "v1.0.0");
        assert_eq!(result, "Unknown");
    }

    #[test]
    fn test_license_compatibility_serde() {
        // Test serialization of LicenseCompatibility
        let compatible = LicenseCompatibility::Compatible;
        let json = serde_json::to_string(&compatible).unwrap();
        assert_eq!(json, "\"Compatible\"");

        let deserialized: LicenseCompatibility = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, LicenseCompatibility::Compatible);
    }

    #[test]
    fn test_is_license_restrictive_edge_cases() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();

            let known_licenses = std::collections::HashMap::new();

            assert!(!is_license_restrictive(&None, &known_licenses));

            assert!(!is_license_restrictive(
                &Some("".to_string()),
                &known_licenses
            ));

            assert!(!is_license_restrictive(
                &Some("   ".to_string()),
                &known_licenses
            ));

            assert!(is_license_restrictive(
                &Some("No License".to_string()),
                &known_licenses
            ));

            assert!(is_license_restrictive(
                &Some("GPL-3.0".to_string()),
                &known_licenses
            ));
            assert!(is_license_restrictive(
                &Some("AGPL-3.0".to_string()),
                &known_licenses
            ));
            assert!(is_license_restrictive(
                &Some("LGPL-3.0".to_string()),
                &known_licenses
            ));
            assert!(is_license_restrictive(
                &Some("MPL-2.0".to_string()),
                &known_licenses
            ));

            assert!(is_license_restrictive(
                &Some("GPL-3.0 License".to_string()),
                &known_licenses
            ));
            assert!(is_license_restrictive(
                &Some("Some GPL-3.0 variant".to_string()),
                &known_licenses
            ));

            assert!(!is_license_restrictive(
                &Some("MIT".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("Apache-2.0".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("BSD-3-Clause".to_string()),
                &known_licenses
            ));

            assert!(!is_license_restrictive(
                &Some("gpl-3.0".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("agpl-3.0".to_string()),
                &known_licenses
            ));

            // Test unknown license
            assert!(!is_license_restrictive(
                &Some("Unknown-License-123".to_string()),
                &known_licenses
            ));
        });
    }
}
