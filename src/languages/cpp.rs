use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::FeludaConfig;
use crate::debug::{log, log_debug, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

#[derive(Debug, Clone)]
enum CppPackageManager {
    Vcpkg,
    Conan,
    CMake,
    Bazel,
    Unknown,
}

pub fn analyze_cpp_licenses(project_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
    log(
        LogLevel::Info,
        &format!("Analyzing C++ dependencies from: {project_path}"),
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

    let (direct_dependencies, package_manager) =
        detect_cpp_dependencies_with_type(project_path, config);
    log(
        LogLevel::Info,
        &format!(
            "Found {} direct C++ dependencies",
            direct_dependencies.len()
        ),
    );
    log_debug("Direct C++ dependencies", &direct_dependencies);

    let max_depth = config.dependencies.max_depth;
    log(
        LogLevel::Info,
        &format!("Using max dependency depth: {max_depth}"),
    );

    let all_deps = resolve_cpp_dependencies(
        project_path,
        &direct_dependencies,
        package_manager,
        max_depth,
    );
    log(
        LogLevel::Info,
        &format!(
            "Total C++ dependencies (including transitive): {}",
            all_deps.len()
        ),
    );
    log_debug("All C++ dependencies", &all_deps);

    let dependencies = all_deps;

    dependencies
        .into_iter()
        .map(|(name, version)| {
            log(
                LogLevel::Info,
                &format!("Processing dependency: {name} ({version})"),
            );

            let license_result = fetch_license_for_cpp_dependency(&name, &version);
            let license = Some(license_result);
            let is_restrictive = is_license_restrictive(&license, &known_licenses);

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    &format!("Restrictive license found: {license:?} for {name}"),
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

fn detect_cpp_dependencies_with_type(
    project_path: &str,
    config: &FeludaConfig,
) -> (Vec<(String, String)>, CppPackageManager) {
    let project_dir = Path::new(project_path).parent().unwrap_or(Path::new("."));

    if let Ok(vcpkg_deps) = parse_vcpkg_dependencies(project_dir, config) {
        log(
            LogLevel::Info,
            &format!("Found {} vcpkg dependencies", vcpkg_deps.len()),
        );
        return (vcpkg_deps, CppPackageManager::Vcpkg);
    }

    if let Ok(conan_deps) = parse_conan_dependencies(project_dir, config) {
        log(
            LogLevel::Info,
            &format!("Found {} conan dependencies", conan_deps.len()),
        );
        return (conan_deps, CppPackageManager::Conan);
    }

    if let Ok(cmake_deps) = parse_cmake_dependencies(project_dir, config) {
        log(
            LogLevel::Info,
            &format!("Found {} cmake dependencies", cmake_deps.len()),
        );
        return (cmake_deps, CppPackageManager::CMake);
    }

    if let Ok(bazel_deps) = parse_bazel_dependencies(project_dir, config) {
        log(
            LogLevel::Info,
            &format!("Found {} bazel dependencies", bazel_deps.len()),
        );
        return (bazel_deps, CppPackageManager::Bazel);
    }

    (Vec::new(), CppPackageManager::Unknown)
}

fn resolve_cpp_dependencies(
    _project_path: &str,
    direct_deps: &[(String, String)],
    package_manager: CppPackageManager,
    max_depth: u32,
) -> Vec<(String, String)> {
    log(
        LogLevel::Info,
        &format!("Resolving C++ dependencies (including transitive up to depth {max_depth})"),
    );

    let mut all_dependencies = Vec::new();
    let mut visited = HashSet::new();
    let mut depth_stats = HashMap::new();

    // Add direct dependencies first
    for (name, version) in direct_deps {
        all_dependencies.push((name.clone(), version.clone()));
        visited.insert(name.clone());
        *depth_stats.entry(0u32).or_insert(0) += 1;
    }

    // Queue for BFS: (package_name, version, depth)
    let mut to_process: Vec<(String, String, u32)> = direct_deps
        .iter()
        .map(|(name, version)| (name.clone(), version.clone(), 0))
        .collect();

    while let Some((name, version, depth)) = to_process.pop() {
        if depth >= max_depth {
            log(
                LogLevel::Trace,
                &format!("Skipping {name} - exceeded max depth {max_depth}"),
            );
            continue;
        }

        log(
            LogLevel::Trace,
            &format!("Resolving transitive dependencies for: {name} (depth {depth})"),
        );

        if let Ok(transitive_deps) = resolve_cpp_transitive_deps(&name, &version, &package_manager)
        {
            log(
                LogLevel::Trace,
                &format!(
                    "Found {} transitive dependencies for {} at depth {}",
                    transitive_deps.len(),
                    name,
                    depth
                ),
            );

            for (dep_name, dep_version) in transitive_deps {
                if !visited.contains(&dep_name) {
                    visited.insert(dep_name.clone());
                    all_dependencies.push((dep_name.clone(), dep_version.clone()));
                    to_process.push((dep_name, dep_version, depth + 1));
                    *depth_stats.entry(depth + 1).or_insert(0) += 1;
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
            "C++ dependency resolution completed. Total dependencies: {} (explored up to depth {})",
            all_dependencies.len(),
            max_depth
        ),
    );

    all_dependencies
}

fn resolve_cpp_transitive_deps(
    package_name: &str,
    version: &str,
    package_manager: &CppPackageManager,
) -> Result<Vec<(String, String)>, String> {
    match package_manager {
        CppPackageManager::Vcpkg => resolve_vcpkg_transitive(package_name, version),
        CppPackageManager::Conan => resolve_conan_transitive(package_name, version),
        CppPackageManager::CMake => resolve_cmake_transitive(package_name, version),
        CppPackageManager::Bazel => resolve_bazel_transitive(package_name, version),
        CppPackageManager::Unknown => Ok(Vec::new()),
    }
}

fn resolve_vcpkg_transitive(
    package_name: &str,
    _version: &str,
) -> Result<Vec<(String, String)>, String> {
    // Try to fetch dependencies from vcpkg registry
    let url = format!(
        "https://raw.githubusercontent.com/microsoft/vcpkg/master/ports/{package_name}/vcpkg.json"
    );

    if let Ok(response) = reqwest::blocking::get(&url) {
        if response.status().is_success() {
            if let Ok(json) = response.json::<Value>() {
                let mut dependencies = Vec::new();

                if let Some(deps) = json.get("dependencies").and_then(|d| d.as_array()) {
                    for dep in deps {
                        match dep {
                            Value::String(name) => {
                                dependencies.push((name.clone(), "latest".to_string()));
                            }
                            Value::Object(obj) => {
                                if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
                                    let version = obj
                                        .get("version")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("latest");
                                    dependencies.push((name.to_string(), version.to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                }

                return Ok(dependencies);
            }
        }
    }

    Ok(Vec::new())
}

fn resolve_conan_transitive(
    package_name: &str,
    version: &str,
) -> Result<Vec<(String, String)>, String> {
    // Try to fetch dependencies from Conan Center
    let url = format!("https://conan.io/center/api/packages/{package_name}/{version}");

    if let Ok(response) = reqwest::blocking::get(&url) {
        if response.status().is_success() {
            if let Ok(json) = response.json::<Value>() {
                let mut dependencies = Vec::new();

                if let Some(requires) = json.get("requires").and_then(|r| r.as_array()) {
                    for req in requires {
                        if let Some(req_str) = req.as_str() {
                            if let Some(slash_pos) = req_str.find('/') {
                                let name = &req_str[..slash_pos];
                                let version = &req_str[slash_pos + 1..];
                                let clean_version = version.split('@').next().unwrap_or(version);
                                dependencies.push((name.to_string(), clean_version.to_string()));
                            }
                        }
                    }
                }

                return Ok(dependencies);
            }
        }
    }

    Ok(Vec::new())
}

fn resolve_cmake_transitive(
    package_name: &str,
    _version: &str,
) -> Result<Vec<(String, String)>, String> {
    // For CMake, we could try to find installed CMake config files
    // This is complex as it depends on the system and CMake installation

    // Try pkg-config if the package has a .pc file
    if let Ok(output) = Command::new("pkg-config")
        .args(["--print-requires", package_name])
        .output()
    {
        if output.status.success() {
            let stdout_str = String::from_utf8_lossy(&output.stdout);
            let mut dependencies = Vec::new();

            for line in stdout_str.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if let Some(pkg_name) = parts.first() {
                        let version = if parts.len() > 2
                            && (parts[1] == ">=" || parts[1] == "=" || parts[1] == ">")
                        {
                            parts[2].to_string()
                        } else {
                            "system".to_string()
                        };
                        dependencies.push((pkg_name.to_string(), version));
                    }
                }
            }

            return Ok(dependencies);
        }
    }

    Ok(Vec::new())
}

fn resolve_bazel_transitive(
    package_name: &str,
    _version: &str,
) -> Result<Vec<(String, String)>, String> {
    // For Bazel, we could try to query the build graph
    // This would require being in a Bazel workspace

    // Try to run bazel query for dependencies
    if let Ok(output) = Command::new("bazel")
        .args(["query", &format!("deps(@{package_name}//...)")])
        .output()
    {
        if output.status.success() {
            let stdout_str = String::from_utf8_lossy(&output.stdout);
            let mut dependencies = Vec::new();

            for line in stdout_str.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('@') && trimmed.contains("//") {
                    if let Some(at_pos) = trimmed.find('@') {
                        if let Some(slash_pos) = trimmed.find("//") {
                            let dep_name = &trimmed[at_pos + 1..slash_pos];
                            if !dep_name.is_empty() && dep_name != package_name {
                                dependencies.push((dep_name.to_string(), "bazel".to_string()));
                            }
                        }
                    }
                }
            }

            return Ok(dependencies);
        }
    }

    Ok(Vec::new())
}

fn parse_vcpkg_dependencies(
    project_dir: &Path,
    _config: &FeludaConfig,
) -> Result<Vec<(String, String)>, String> {
    let vcpkg_json = project_dir.join("vcpkg.json");
    if !vcpkg_json.exists() {
        return Err("No vcpkg.json found".to_string());
    }

    let content =
        fs::read_to_string(&vcpkg_json).map_err(|e| format!("Failed to read vcpkg.json: {e}"))?;

    let json: Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse vcpkg.json: {e}"))?;

    let mut dependencies = Vec::new();

    if let Some(deps) = json.get("dependencies").and_then(|d| d.as_array()) {
        for dep in deps {
            match dep {
                Value::String(name) => {
                    dependencies.push((name.clone(), "latest".to_string()));
                }
                Value::Object(obj) => {
                    if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
                        let version = obj
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("latest");
                        dependencies.push((name.to_string(), version.to_string()));
                    }
                }
                _ => {}
            }
        }
    }

    Ok(dependencies)
}

fn parse_conan_dependencies(
    project_dir: &Path,
    _config: &FeludaConfig,
) -> Result<Vec<(String, String)>, String> {
    let conanfile_txt = project_dir.join("conanfile.txt");
    let conanfile_py = project_dir.join("conanfile.py");

    if conanfile_txt.exists() {
        parse_conanfile_txt(&conanfile_txt)
    } else if conanfile_py.exists() {
        parse_conanfile_py(&conanfile_py)
    } else {
        Err("No conanfile found".to_string())
    }
}

fn parse_conanfile_txt(conanfile_path: &Path) -> Result<Vec<(String, String)>, String> {
    let content = fs::read_to_string(conanfile_path)
        .map_err(|e| format!("Failed to read conanfile.txt: {e}"))?;

    let mut dependencies = Vec::new();
    let mut in_requires_section = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "[requires]" {
            in_requires_section = true;
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed != "[requires]" {
            in_requires_section = false;
            continue;
        }

        if in_requires_section && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(slash_pos) = trimmed.find('/') {
                let name = &trimmed[..slash_pos];
                let version = &trimmed[slash_pos + 1..];
                let clean_version = version.split('@').next().unwrap_or(version);
                dependencies.push((name.to_string(), clean_version.to_string()));
            }
        }
    }

    Ok(dependencies)
}

fn parse_conanfile_py(conanfile_path: &Path) -> Result<Vec<(String, String)>, String> {
    let content = fs::read_to_string(conanfile_path)
        .map_err(|e| format!("Failed to read conanfile.py: {e}"))?;

    let mut dependencies = Vec::new();

    let requires_regex = Regex::new(r#"requires\s*=\s*\[(.*?)\]"#)
        .map_err(|e| format!("Failed to compile requires regex: {e}"))?;

    if let Some(cap) = requires_regex.captures(&content) {
        if let Some(requires_content) = cap.get(1) {
            let req_str = requires_content.as_str();

            let dep_regex = Regex::new(r#""([^"]+)""#)
                .map_err(|e| format!("Failed to compile dependency regex: {e}"))?;

            for dep_cap in dep_regex.captures_iter(req_str) {
                if let Some(dep_str) = dep_cap.get(1) {
                    let dep = dep_str.as_str();
                    if let Some(slash_pos) = dep.find('/') {
                        let name = &dep[..slash_pos];
                        let version = &dep[slash_pos + 1..];
                        let clean_version = version.split('@').next().unwrap_or(version);
                        dependencies.push((name.to_string(), clean_version.to_string()));
                    }
                }
            }
        }
    }

    Ok(dependencies)
}

fn parse_cmake_dependencies(
    project_dir: &Path,
    _config: &FeludaConfig,
) -> Result<Vec<(String, String)>, String> {
    let cmake_file = project_dir.join("CMakeLists.txt");
    if !cmake_file.exists() {
        return Err("No CMakeLists.txt found".to_string());
    }

    let content = fs::read_to_string(&cmake_file)
        .map_err(|e| format!("Failed to read CMakeLists.txt: {e}"))?;

    let mut dependencies = Vec::new();

    let fetchcontent_regex = Regex::new(r"FetchContent_Declare\s*\(\s*(\w+)")
        .map_err(|e| format!("Failed to compile FetchContent regex: {e}"))?;

    for cap in fetchcontent_regex.captures_iter(&content) {
        if let Some(dep_name) = cap.get(1) {
            dependencies.push((dep_name.as_str().to_string(), "git".to_string()));
        }
    }

    let find_package_regex = Regex::new(r"find_package\s*\(\s*(\w+)(?:\s+([^)]+))?\)")
        .map_err(|e| format!("Failed to compile find_package regex: {e}"))?;

    for cap in find_package_regex.captures_iter(&content) {
        if let Some(pkg_name) = cap.get(1) {
            let version = cap
                .get(2)
                .map(|v| v.as_str().trim())
                .and_then(|v| {
                    if v.starts_with("REQUIRED") || v.starts_with("COMPONENTS") {
                        None
                    } else {
                        Some(v.split_whitespace().next().unwrap_or("system"))
                    }
                })
                .unwrap_or("system");

            dependencies.push((pkg_name.as_str().to_string(), version.to_string()));
        }
    }

    Ok(dependencies)
}

fn parse_bazel_dependencies(
    project_dir: &Path,
    _config: &FeludaConfig,
) -> Result<Vec<(String, String)>, String> {
    let module_bazel = project_dir.join("MODULE.bazel");
    let workspace = project_dir.join("WORKSPACE");

    if module_bazel.exists() {
        parse_module_bazel(&module_bazel)
    } else if workspace.exists() {
        parse_workspace_bazel(&workspace)
    } else {
        Err("No Bazel build files found".to_string())
    }
}

fn parse_module_bazel(module_path: &Path) -> Result<Vec<(String, String)>, String> {
    let content =
        fs::read_to_string(module_path).map_err(|e| format!("Failed to read MODULE.bazel: {e}"))?;

    let mut dependencies = Vec::new();

    let bazel_dep_regex =
        Regex::new(r#"bazel_dep\s*\(\s*name\s*=\s*"([^"]+)"\s*,\s*version\s*=\s*"([^"]+)""#)
            .map_err(|e| format!("Failed to compile bazel_dep regex: {e}"))?;

    for cap in bazel_dep_regex.captures_iter(&content) {
        if let (Some(name), Some(version)) = (cap.get(1), cap.get(2)) {
            dependencies.push((name.as_str().to_string(), version.as_str().to_string()));
        }
    }

    Ok(dependencies)
}

fn parse_workspace_bazel(workspace_path: &Path) -> Result<Vec<(String, String)>, String> {
    let content =
        fs::read_to_string(workspace_path).map_err(|e| format!("Failed to read WORKSPACE: {e}"))?;

    let mut dependencies = Vec::new();

    let http_archive_regex = Regex::new(r#"http_archive\s*\(\s*name\s*=\s*"([^"]+)""#)
        .map_err(|e| format!("Failed to compile http_archive regex: {e}"))?;

    for cap in http_archive_regex.captures_iter(&content) {
        if let Some(name) = cap.get(1) {
            dependencies.push((name.as_str().to_string(), "archive".to_string()));
        }
    }

    Ok(dependencies)
}

fn fetch_license_for_cpp_dependency(name: &str, version: &str) -> String {
    match version {
        "latest" | "git" => fetch_license_from_vcpkg_registry(name),
        v if v.chars().next().unwrap_or('0').is_ascii_digit() => {
            fetch_license_from_conan_center(name, version)
        }
        "system" => fetch_license_from_system_package(name),
        _ => format!("Unknown license for {name}: {version}"),
    }
}

fn fetch_license_from_vcpkg_registry(package_name: &str) -> String {
    let url = format!(
        "https://raw.githubusercontent.com/microsoft/vcpkg/master/ports/{package_name}/vcpkg.json"
    );

    if let Ok(response) = reqwest::blocking::get(&url) {
        if response.status().is_success() {
            if let Ok(json) = response.json::<Value>() {
                if let Some(license) = json.get("license").and_then(|l| l.as_str()) {
                    return license.to_string();
                }
            }
        }
    }

    format!("Unknown license (vcpkg: {package_name})")
}

fn fetch_license_from_conan_center(package_name: &str, version: &str) -> String {
    let url = format!("https://conan.io/center/api/packages/{package_name}/{version}");

    if let Ok(response) = reqwest::blocking::get(&url) {
        if response.status().is_success() {
            if let Ok(json) = response.json::<Value>() {
                if let Some(license) = json.get("license").and_then(|l| l.as_str()) {
                    return license.to_string();
                }
            }
        }
    }

    format!("Unknown license (conan: {package_name})")
}

fn fetch_license_from_system_package(package_name: &str) -> String {
    if let Ok(output) = Command::new("pkg-config")
        .args(["--variable=license", package_name])
        .output()
    {
        if output.status.success() {
            let license = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !license.is_empty() {
                return license;
            }
        }
    }

    format!("Unknown license (system: {package_name})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_vcpkg_dependencies() {
        let temp_dir = TempDir::new().unwrap();
        let vcpkg_json = temp_dir.path().join("vcpkg.json");

        fs::write(
            &vcpkg_json,
            r#"{
  "name": "test-project",
  "version": "1.0.0",
  "dependencies": [
    "boost",
    {
      "name": "opencv",
      "version": "4.5.0"
    }
  ]
}"#,
        )
        .unwrap();

        let config = FeludaConfig::default();
        let result = parse_vcpkg_dependencies(temp_dir.path(), &config).unwrap();

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|(name, _)| name == "boost"));
        assert!(result
            .iter()
            .any(|(name, version)| name == "opencv" && version == "4.5.0"));
    }

    #[test]
    fn test_parse_conanfile_txt() {
        let temp_dir = TempDir::new().unwrap();
        let conanfile = temp_dir.path().join("conanfile.txt");

        fs::write(
            &conanfile,
            r#"[requires]
boost/1.75.0
openssl/1.1.1k@
zlib/1.2.11

[generators]
cmake
"#,
        )
        .unwrap();

        let result = parse_conanfile_txt(&conanfile).unwrap();

        assert_eq!(result.len(), 3);
        assert!(result
            .iter()
            .any(|(name, version)| name == "boost" && version == "1.75.0"));
        assert!(result
            .iter()
            .any(|(name, version)| name == "openssl" && version == "1.1.1k"));
        assert!(result
            .iter()
            .any(|(name, version)| name == "zlib" && version == "1.2.11"));
    }

    #[test]
    fn test_parse_cmake_dependencies() {
        let temp_dir = TempDir::new().unwrap();
        let cmake_file = temp_dir.path().join("CMakeLists.txt");

        fs::write(
            &cmake_file,
            r#"cmake_minimum_required(VERSION 3.14)
project(TestProject)

include(FetchContent)
FetchContent_Declare(json
    URL https://github.com/nlohmann/json/releases/download/v3.10.5/json.tar.xz)
FetchContent_MakeAvailable(json)

find_package(Boost 1.70 REQUIRED COMPONENTS system filesystem)
find_package(OpenSSL REQUIRED)
"#,
        )
        .unwrap();

        let config = FeludaConfig::default();
        let result = parse_cmake_dependencies(temp_dir.path(), &config).unwrap();

        assert!(!result.is_empty());
        assert!(result.iter().any(|(name, _)| name == "json"));
        assert!(result
            .iter()
            .any(|(name, version)| name == "Boost" && version == "1.70"));
        assert!(result.iter().any(|(name, _)| name == "OpenSSL"));
    }

    #[test]
    fn test_analyze_cpp_licenses_empty() {
        let temp_dir = TempDir::new().unwrap();
        let dummy_file = temp_dir.path().join("dummy");
        fs::write(&dummy_file, "").unwrap();

        let config = FeludaConfig::default();
        let result = analyze_cpp_licenses(dummy_file.to_str().unwrap(), &config);

        // Should be empty since no build files exist
        assert!(result.is_empty());
    }
}
