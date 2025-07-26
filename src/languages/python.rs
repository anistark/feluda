use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use toml::Value as TomlValue;

use crate::debug::{log, log_debug, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

/// License Info
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct License {
    title: String,            // The full name of the license
    spdx_id: String,          // The SPDX identifier for the license
    permissions: Vec<String>, // A list of permissions granted by the license
    conditions: Vec<String>,  // A list of conditions that must be met under the license
    limitations: Vec<String>, // A list of limitations imposed by the license
}

/// Analyze the licenses of Python dependencies
pub fn analyze_python_licenses(package_file_path: &str) -> Vec<LicenseInfo> {
    let mut licenses = Vec::new();
    log(
        LogLevel::Info,
        &format!("Analyzing Python dependencies from: {package_file_path}"),
    );

    let known_licenses = match fetch_licenses_from_github() {
        Ok(licenses) => {
            log(
                LogLevel::Info,
                &format!("Fetched {} known licenses from GitHub", licenses.len()),
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
            Ok(content) => match toml::from_str::<TomlValue>(&content) {
                Ok(config) => {
                    if let Some(project) = config.as_table().and_then(|t| t.get("project")) {
                        if let Some(deps) = project
                            .as_table()
                            .and_then(|t| t.get("dependencies"))
                            .and_then(|d| d.as_array())
                        {
                            log(
                                LogLevel::Info,
                                &format!("Found {} Python dependencies", deps.len()),
                            );
                            log_debug("Dependencies", deps);

                            for dep in deps {
                                if let Some(dep_str) = dep.as_str() {
                                    log(
                                        LogLevel::Info,
                                        &format!("Processing dependency: {dep_str}"),
                                    );

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

                                    let version_clean =
                                        version.trim_matches('"').replace("^", "").replace("~", "");

                                    log(
                                        LogLevel::Info,
                                        &format!(
                                            "Fetching license for Python dependency: {name} ({version_clean})"
                                        ),
                                    );

                                    let license_result =
                                        fetch_license_for_python_dependency(name, &version_clean);
                                    let license = Some(license_result);
                                    let is_restrictive =
                                        is_license_restrictive(&license, &known_licenses);

                                    if is_restrictive {
                                        log(
                                            LogLevel::Warn,
                                            &format!(
                                                "Restrictive license found: {license:?} for {name}"
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
            },
            Err(err) => {
                log_error("Failed to read pyproject.toml file", &err);
            }
        }
    } else {
        log(LogLevel::Info, "Processing requirements.txt format");

        match File::open(package_file_path) {
            Ok(file) => {
                let reader = BufReader::new(file);
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
                                    &format!("Processing requirement: {name} {version}"),
                                );

                                let license_result =
                                    fetch_license_for_python_dependency(&name, &version);
                                let license = Some(license_result);
                                let is_restrictive =
                                    is_license_restrictive(&license, &known_licenses);

                                if is_restrictive {
                                    log(
                                        LogLevel::Warn,
                                        &format!(
                                            "Restrictive license found: {license:?} for {name}"
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
                                    &format!("Invalid requirement line: {line}"),
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
                    &format!("Processed {dep_count} requirements from requirements.txt"),
                );
            }
            Err(err) => {
                log_error("Failed to open requirements.txt file", &err);
            }
        }
    }

    log(
        LogLevel::Info,
        &format!("Found {} Python dependencies with licenses", licenses.len()),
    );
    licenses
}

/// Fetch the license for a Python dependency from the Python Package Index (PyPI)
pub fn fetch_license_for_python_dependency(name: &str, version: &str) -> String {
    let api_url = format!("https://pypi.org/pypi/{name}/{version}/json");
    log(
        LogLevel::Info,
        &format!("Fetching license from PyPI: {api_url}"),
    );

    match reqwest::blocking::get(&api_url) {
        Ok(response) => {
            let status = response.status();
            log(
                LogLevel::Info,
                &format!("PyPI API response status: {status}"),
            );

            if status.is_success() {
                match response.json::<Value>() {
                    Ok(json) => match json["info"]["license"].as_str() {
                        Some(license_str) if !license_str.is_empty() => {
                            log(
                                LogLevel::Info,
                                &format!("License found for {name}: {license_str}"),
                            );
                            license_str.to_string()
                        }
                        _ => {
                            log(
                                LogLevel::Warn,
                                &format!("No license found for {name} ({version})"),
                            );
                            format!("Unknown license for {name}: {version}")
                        }
                    },
                    Err(err) => {
                        log_error(
                            &format!("Failed to parse JSON for {name}: {version}"),
                            &err,
                        );
                        String::from("Unknown")
                    }
                }
            } else {
                log(
                    LogLevel::Error,
                    &format!("Failed to fetch metadata for {name}: HTTP {status}"),
                );
                String::from("Unknown")
            }
        }
        Err(err) => {
            log_error(&format!("Failed to fetch metadata for {name}"), &err);
            String::from("Unknown")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_analyze_python_licenses_pyproject_toml() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_toml_path = temp_dir.path().join("pyproject.toml");

        std::fs::write(
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
    fn test_analyze_python_licenses_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let requirements_path = temp_dir.path().join("requirements.txt");

        std::fs::write(&requirements_path, "").unwrap();

        let result = analyze_python_licenses(requirements_path.to_str().unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_analyze_python_licenses_invalid_format() {
        let temp_dir = TempDir::new().unwrap();
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
    fn test_fetch_license_for_python_dependency_error_handling() {
        // Test with a definitely non-existent package
        let result =
            fetch_license_for_python_dependency("definitely_nonexistent_package_12345", "1.0.0");
        assert!(result.contains("Unknown") || result.contains("nonexistent"));
    }
}
