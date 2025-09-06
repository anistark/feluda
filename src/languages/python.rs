use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Command;
use toml::Value as TomlValue;

use crate::config::FeludaConfig;
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

/// Analyze the licenses of Python dependencies with transitive resolution
pub fn analyze_python_licenses(package_file_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
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
                Ok(toml_config) => {
                    if let Some(project) = toml_config.as_table().and_then(|t| t.get("project")) {
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

                            // First collect direct dependencies
                            let mut direct_deps = Vec::new();
                            for dep in deps {
                                if let Some(dep_str) = dep.as_str() {
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

                                    direct_deps.push((name.to_string(), version.to_string()));
                                }
                            }

                            // Try to resolve all dependencies (direct + transitive) using uv or fallback to PyPI
                            let max_depth = config.dependencies.max_depth;
                            log(
                                LogLevel::Info,
                                &format!("Using max dependency depth: {max_depth}"),
                            );
                            let all_deps = resolve_python_dependencies(
                                &direct_deps,
                                package_file_path,
                                max_depth,
                            );

                            // Process all resolved dependencies
                            for (name, version) in all_deps {
                                log(
                                    LogLevel::Info,
                                    &format!("Processing dependency: {name} ({version})"),
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
                let mut direct_deps = Vec::new();

                // Direct dependencies
                for line_result in reader.lines() {
                    match line_result {
                        Ok(line) => {
                            let line = line.trim();
                            if line.is_empty() || line.starts_with('#') {
                                continue;
                            }

                            // Parse requirement line (supporting various formats)
                            if let Some((name, version)) = parse_requirement_line(line) {
                                direct_deps.push((name, version));
                            } else {
                                log(LogLevel::Warn, &format!("Invalid requirement line: {line}"));
                            }
                        }
                        Err(err) => {
                            log_error("Failed to read line from requirements.txt", &err);
                        }
                    }
                }

                log(
                    LogLevel::Info,
                    &format!(
                        "Found {} direct requirements in requirements.txt",
                        direct_deps.len()
                    ),
                );

                // Try to resolve all dependencies (direct + transitive)
                let max_depth = config.dependencies.max_depth;
                log(
                    LogLevel::Info,
                    &format!("Using max dependency depth: {max_depth}"),
                );
                let all_deps =
                    resolve_python_dependencies(&direct_deps, package_file_path, max_depth);

                // Process all resolved dependencies
                for (name, version) in all_deps {
                    log(
                        LogLevel::Info,
                        &format!("Processing dependency: {name} ({version})"),
                    );

                    let license_result = fetch_license_for_python_dependency(&name, &version);
                    let license = Some(license_result);
                    let is_restrictive = is_license_restrictive(&license, &known_licenses);

                    if is_restrictive {
                        log(
                            LogLevel::Warn,
                            &format!("Restrictive license found: {license:?} for {name}"),
                        );
                    }

                    licenses.push(LicenseInfo {
                        name,
                        version,
                        license,
                        is_restrictive,
                        compatibility: LicenseCompatibility::Unknown,
                    });
                }

                log(
                    LogLevel::Info,
                    &format!(
                        "Processed {} total dependencies (including transitive)",
                        licenses.len()
                    ),
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
                        log_error(&format!("Failed to parse JSON for {name}: {version}"), &err);
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

/// Parse a requirement line from requirements.txt supporting various formats
fn parse_requirement_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();

    // Handle various requirement formats
    if let Some((name, version)) = line
        .split_once("==")
        .or_else(|| line.split_once(">="))
        .or_else(|| line.split_once(">"))
        .or_else(|| line.split_once("~="))
        .or_else(|| line.split_once("<="))
        .or_else(|| line.split_once("<"))
    {
        let name = name.trim();
        let version = version
            .trim()
            .trim_matches('"')
            .replace("^", "")
            .replace("~", "");
        Some((name.to_string(), version))
    } else {
        // Package name without version
        Some((line.to_string(), "latest".to_string()))
    }
}

/// Resolve all Python dependencies (direct + transitive) with configurable depth
fn resolve_python_dependencies(
    direct_deps: &[(String, String)],
    package_file_path: &str,
    max_depth: u32,
) -> Vec<(String, String)> {
    log(
        LogLevel::Info,
        &format!("Resolving Python dependencies (including transitive up to depth {max_depth})"),
    );

    // First, try using uv for complete dependency resolution
    if let Ok(uv_deps) = resolve_with_uv(package_file_path, max_depth) {
        if !uv_deps.is_empty() {
            log(
                LogLevel::Info,
                &format!(
                    "Resolved {} dependencies using uv (depth {})",
                    uv_deps.len(),
                    max_depth
                ),
            );
            return uv_deps;
        }
    }

    // Fallback to PyPI-based transitive resolution
    log(
        LogLevel::Info,
        "Falling back to PyPI-based transitive dependency resolution",
    );
    resolve_with_pypi(direct_deps, max_depth)
}

/// Try to resolve dependencies using uv tool with depth limit
fn resolve_with_uv(
    package_file_path: &str,
    max_depth: u32,
) -> Result<Vec<(String, String)>, String> {
    let project_dir = Path::new(package_file_path)
        .parent()
        .ok_or("Cannot determine project directory")?;

    log(
        LogLevel::Info,
        &format!("Attempting to resolve dependencies with uv (max depth: {max_depth})"),
    );

    // Try uv lock command first (for uv-managed projects)
    if let Ok(output) = Command::new("uv")
        .args(["lock", "--dry-run"])
        .current_dir(project_dir)
        .output()
    {
        if output.status.success() {
            // Parse uv.lock file if it exists
            let lock_file = project_dir.join("uv.lock");
            if lock_file.exists() {
                if let Ok(deps) = parse_uv_lock(&lock_file, max_depth) {
                    log(
                        LogLevel::Info,
                        &format!("Resolved {} dependencies from uv.lock", deps.len()),
                    );
                    return Ok(deps);
                }
            }
        }
    }

    // Try pip-compile style resolution using uv
    if let Ok(output) = Command::new("uv")
        .args(["pip", "compile", "--dry-run", package_file_path])
        .current_dir(project_dir)
        .output()
    {
        if output.status.success() {
            let stdout_str = String::from_utf8_lossy(&output.stdout);
            let deps = parse_pip_compile_output(&stdout_str);
            log(
                LogLevel::Info,
                &format!(
                    "Resolved {} dependencies from pip-compile output",
                    deps.len()
                ),
            );
            return Ok(deps);
        }
    }

    Err("uv resolution failed".to_string())
}

/// Parse uv.lock file to extract dependencies with depth awareness
fn parse_uv_lock(lock_file: &Path, max_depth: u32) -> Result<Vec<(String, String)>, String> {
    let content =
        fs::read_to_string(lock_file).map_err(|e| format!("Failed to read uv.lock: {e}"))?;

    // uv.lock is TOML format
    let lock_data: TomlValue =
        toml::from_str(&content).map_err(|e| format!("Failed to parse uv.lock: {e}"))?;

    let mut deps = Vec::new();

    log(
        LogLevel::Info,
        &format!("Parsing uv.lock with max depth {max_depth}"),
    );

    // Extract packages from uv.lock format
    if let Some(packages) = lock_data.get("package").and_then(|p| p.as_array()) {
        for package in packages {
            if let Some(package_table) = package.as_table() {
                if let (Some(name), Some(version)) = (
                    package_table.get("name").and_then(|n| n.as_str()),
                    package_table.get("version").and_then(|v| v.as_str()),
                ) {
                    deps.push((name.to_string(), version.to_string()));
                }
            }
        }

        log(
            LogLevel::Info,
            &format!(
                "Extracted {} dependencies from uv.lock (all depths included)",
                deps.len()
            ),
        );
    }

    Ok(deps)
}

/// Parse pip-compile style output to extract dependencies
fn parse_pip_compile_output(output: &str) -> Vec<(String, String)> {
    let mut deps = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((name, version)) = parse_requirement_line(line) {
            deps.push((name, version));
        }
    }

    deps
}

/// Resolve transitive dependencies using PyPI API with configurable depth limit
fn resolve_with_pypi(direct_deps: &[(String, String)], max_depth: u32) -> Vec<(String, String)> {
    let mut all_deps = HashMap::new();
    let mut processed = HashSet::new();
    let mut to_process: Vec<(String, String, u32)> = direct_deps
        .iter()
        .map(|(name, version)| (name.clone(), version.clone(), 0))
        .collect();

    log(
        LogLevel::Info,
        &format!(
            "Starting PyPI-based resolution with {} direct dependencies (max depth: {})",
            direct_deps.len(),
            max_depth
        ),
    );

    let mut depth_stats = HashMap::new();

    // Iteratively resolve dependencies with depth tracking
    while let Some((name, version, depth)) = to_process.pop() {
        let key = format!("{name}@{version}");
        if processed.contains(&key) {
            continue;
        }

        // Skip if we've exceeded max depth
        if depth >= max_depth {
            log(
                LogLevel::Info,
                &format!("Skipping {name}@{version} - exceeded max depth {max_depth}"),
            );
            continue;
        }

        processed.insert(key);
        all_deps.insert(name.clone(), version.clone());

        // Track depth statistics
        *depth_stats.entry(depth).or_insert(0) += 1;

        log(
            LogLevel::Info,
            &format!("Resolving dependencies for: {name}@{version} (depth {depth})"),
        );

        // Fetch dependencies for this package
        if let Ok(transitive_deps) = fetch_pypi_dependencies(&name, &version) {
            log(
                LogLevel::Info,
                &format!(
                    "Found {} transitive dependencies for {} at depth {}",
                    transitive_deps.len(),
                    name,
                    depth
                ),
            );

            for (dep_name, dep_version) in transitive_deps {
                let dep_key = format!("{dep_name}@{dep_version}");
                if !processed.contains(&dep_key) {
                    to_process.push((dep_name, dep_version, depth + 1));
                }
            }
        }
    }

    // Log depth statistics
    for depth in 0..=max_depth {
        if let Some(count) = depth_stats.get(&depth) {
            log(
                LogLevel::Info,
                &format!("Depth {depth}: {count} dependencies"),
            );
        }
    }

    log(
        LogLevel::Info,
        &format!(
            "PyPI resolution completed. Total dependencies: {} (explored up to depth {})",
            all_deps.len(),
            max_depth
        ),
    );

    all_deps.into_iter().collect()
}

/// Fetch dependencies from PyPI for a specific package
fn fetch_pypi_dependencies(name: &str, version: &str) -> Result<Vec<(String, String)>, String> {
    let api_url = format!("https://pypi.org/pypi/{name}/{version}/json");

    match reqwest::blocking::get(&api_url) {
        Ok(response) => {
            if response.status().is_success() {
                if let Ok(json) = response.json::<Value>() {
                    let mut deps = Vec::new();

                    // Extract requires_dist information
                    if let Some(requires_dist) = json["info"]["requires_dist"].as_array() {
                        for req in requires_dist {
                            if let Some(req_str) = req.as_str() {
                                if let Some((dep_name, dep_version)) =
                                    parse_pypi_requirement(req_str)
                                {
                                    deps.push((dep_name, dep_version));
                                }
                            }
                        }
                    }

                    return Ok(deps);
                }
            }
        }
        Err(err) => {
            log_error(&format!("Failed to fetch dependencies for {name}"), &err);
        }
    }

    Ok(Vec::new())
}

/// Parse a PyPI requires_dist requirement string
fn parse_pypi_requirement(req_str: &str) -> Option<(String, String)> {
    // Handle requirements like "requests>=2.20.0", "flask", "typing-extensions>=3.7.4; python_version < '3.8'"
    let req_str = req_str.trim();

    // TODO: Check requirements with environment markers
    let req_str = if let Some((base, _marker)) = req_str.split_once(';') {
        base.trim()
    } else {
        req_str
    };

    // Parse package name and version using regex
    let mut chars = req_str.chars().peekable();
    let mut name = String::new();

    // Extract package name
    while let Some(ch) = chars.peek() {
        if ">=<!~=()".contains(*ch) || ch.is_whitespace() {
            break;
        }
        if let Some(ch) = chars.next() {
            name.push(ch);
        }
    }

    let name = name.trim().to_string();
    if name.is_empty() {
        return None;
    }

    // Skip whitespace and parentheses
    while let Some(ch) = chars.peek() {
        if ch.is_whitespace() || *ch == '(' {
            chars.next();
        } else {
            break;
        }
    }

    // Extract version constraint
    let remaining: String = chars.collect();
    let remaining = remaining.trim_end_matches(')').trim();

    if remaining.is_empty() {
        return Some((name, "latest".to_string()));
    }

    // Parse version constraints
    let constraints: Vec<&str> = remaining.split(',').collect();
    let mut best_version = "latest";

    for constraint in &constraints {
        if let Some((_operator, version_part)) = parse_version_constraint(constraint.trim()) {
            if constraint.trim().starts_with(">=") || constraint.trim().starts_with("==") {
                best_version = version_part.trim();
                break;
            } else if best_version == "latest" {
                best_version = version_part.trim();
            }
        }
    }

    Some((name, best_version.to_string()))
}

/// Parse version constraint operators
fn parse_version_constraint(constraint: &str) -> Option<(&str, &str)> {
    let constraint = constraint.trim();

    if let Some(version) = constraint.strip_prefix(">=") {
        Some((">=", version.trim()))
    } else if let Some(version) = constraint.strip_prefix("<=") {
        Some(("<=", version.trim()))
    } else if let Some(version) = constraint.strip_prefix("==") {
        Some(("==", version.trim()))
    } else if let Some(version) = constraint.strip_prefix("~=") {
        Some(("~=", version.trim()))
    } else if let Some(version) = constraint.strip_prefix("!=") {
        Some(("!=", version.trim()))
    } else if let Some(version) = constraint.strip_prefix(">") {
        Some((">", version.trim()))
    } else if let Some(version) = constraint.strip_prefix("<") {
        Some(("<", version.trim()))
    } else {
        None
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

        let config = FeludaConfig::default();
        let result = analyze_python_licenses(pyproject_toml_path.to_str().unwrap(), &config);
        assert!(!result.is_empty());
        assert!(result.iter().any(|info| info.name == "requests"));
        assert!(result.iter().any(|info| info.name == "flask"));
    }

    #[test]
    fn test_analyze_python_licenses_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let requirements_path = temp_dir.path().join("requirements.txt");

        std::fs::write(&requirements_path, "").unwrap();

        let config = FeludaConfig::default();
        let result = analyze_python_licenses(requirements_path.to_str().unwrap(), &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_analyze_python_licenses_invalid_format() {
        let temp_dir = TempDir::new().unwrap();
        let requirements_path = temp_dir.path().join("requirements.txt");

        std::fs::write(&requirements_path, "# This is a comment\n\n").unwrap();

        let config = FeludaConfig::default();
        let result = analyze_python_licenses(requirements_path.to_str().unwrap(), &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_analyze_python_licenses_packages_without_versions() {
        let temp_dir = TempDir::new().unwrap();
        let requirements_path = temp_dir.path().join("requirements.txt");

        std::fs::write(
            &requirements_path,
            "requests\nflask\n# This is a comment\nnumpy",
        )
        .unwrap();

        let config = FeludaConfig::default();
        let result = analyze_python_licenses(requirements_path.to_str().unwrap(), &config);
        // Process packages without explicit versions using transitive resolution
        assert!(!result.is_empty());
        assert!(result.iter().any(|info| info.name == "requests"));
        assert!(result.iter().any(|info| info.name == "flask"));
        assert!(result.iter().any(|info| info.name == "numpy"));
    }

    #[test]
    fn test_fetch_license_for_python_dependency_error_handling() {
        // Test with a definitely non-existent package
        let result =
            fetch_license_for_python_dependency("definitely_nonexistent_package_12345", "1.0.0");
        assert!(result.contains("Unknown") || result.contains("nonexistent"));
    }

    #[test]
    fn test_parse_requirement_line() {
        // Test various requirement formats
        assert_eq!(
            parse_requirement_line("requests==2.31.0"),
            Some(("requests".to_string(), "2.31.0".to_string()))
        );
        assert_eq!(
            parse_requirement_line("flask>=2.0.0"),
            Some(("flask".to_string(), "2.0.0".to_string()))
        );
        assert_eq!(
            parse_requirement_line("django"),
            Some(("django".to_string(), "latest".to_string()))
        );
    }

    #[test]
    fn test_parse_pypi_requirement() {
        // Test PyPI requires_dist format parsing
        assert_eq!(
            parse_pypi_requirement("requests>=2.20.0"),
            Some(("requests".to_string(), "2.20.0".to_string()))
        );
        assert_eq!(
            parse_pypi_requirement("typing-extensions>=3.7.4; python_version < '3.8'"),
            Some(("typing-extensions".to_string(), "3.7.4".to_string()))
        );
        assert_eq!(
            parse_pypi_requirement("flask"),
            Some(("flask".to_string(), "latest".to_string()))
        );

        // Test complex version constraints
        assert_eq!(
            parse_pypi_requirement("urllib3 (<3,>=1.21.1)"),
            Some(("urllib3".to_string(), "1.21.1".to_string()))
        );
        assert_eq!(
            parse_pypi_requirement("chardet (<6,>=3.0.2)"),
            Some(("chardet".to_string(), "3.0.2".to_string()))
        );
        assert_eq!(
            parse_pypi_requirement("PySocks (!=1.5.7,>=1.5.6)"),
            Some(("PySocks".to_string(), "1.5.6".to_string()))
        );
    }
}
