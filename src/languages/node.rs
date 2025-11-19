use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::debug::{log, log_debug, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

/// Type alias for dependency detection
type DependencyDetector = fn(&Path) -> Result<HashMap<String, String>, String>;

/// Structure representing a package.json file
#[derive(Deserialize, Serialize, Debug)]
pub struct PackageJson {
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "peerDependencies")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "optionalDependencies")]
    pub optional_dependencies: Option<HashMap<String, String>>,
}

impl PackageJson {
    /// Get all dependencies from package.json (production + dev + peer + optional)
    #[allow(dead_code)]
    pub fn get_all_dependencies(&self) -> HashMap<String, String> {
        let mut all_dependencies: HashMap<String, String> = HashMap::new();

        if let Some(deps) = &self.dependencies {
            all_dependencies.extend(deps.clone());
        }
        if let Some(dev_deps) = &self.dev_dependencies {
            all_dependencies.extend(dev_deps.clone());
        }
        if let Some(peer_deps) = &self.peer_dependencies {
            all_dependencies.extend(peer_deps.clone());
        }
        if let Some(opt_deps) = &self.optional_dependencies {
            all_dependencies.extend(opt_deps.clone());
        }

        all_dependencies
    }
}

/// Recursive dependency resolver
struct DependencyResolver {
    resolved_cache: HashMap<String, PackageMetadata>,
    processing_stack: HashSet<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PackageMetadata {
    name: String,
    version: String,
    license: Option<String>,
    dependencies: HashMap<String, String>,
}

impl DependencyResolver {
    fn new() -> Self {
        Self {
            resolved_cache: HashMap::new(),
            processing_stack: HashSet::new(),
        }
    }

    fn resolve_recursive_dependencies(
        &mut self,
        package_json_path: &str,
        max_depth: usize,
    ) -> Result<HashMap<String, String>, String> {
        let root_package = self.parse_local_package_json(package_json_path)?;
        let mut all_dependencies = HashMap::new();

        let to_resolve = root_package.dependencies.clone();
        self.resolve_dependencies_recursive(to_resolve, &mut all_dependencies, 0, max_depth)?;

        Ok(all_dependencies)
    }

    fn resolve_dependencies_recursive(
        &mut self,
        dependencies: HashMap<String, String>,
        all_deps: &mut HashMap<String, String>,
        current_depth: usize,
        max_depth: usize,
    ) -> Result<(), String> {
        if current_depth >= max_depth {
            return Ok(());
        }

        for (name, version_spec) in dependencies {
            if all_deps.contains_key(&name) || self.processing_stack.contains(&name) {
                continue;
            }

            self.processing_stack.insert(name.clone());

            match self.resolve_package_metadata(&name, &version_spec) {
                Ok(metadata) => {
                    all_deps.insert(name.clone(), metadata.version.clone());
                    self.resolve_dependencies_recursive(
                        metadata.dependencies,
                        all_deps,
                        current_depth + 1,
                        max_depth,
                    )?;
                }
                Err(_) => {
                    all_deps.insert(name.clone(), clean_version_string(&version_spec));
                }
            }

            self.processing_stack.remove(&name);
        }

        Ok(())
    }

    fn resolve_package_metadata(
        &mut self,
        name: &str,
        version_spec: &str,
    ) -> Result<PackageMetadata, String> {
        let cache_key = format!("{name}@{version_spec}");

        if let Some(cached) = self.resolved_cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        let metadata = self.fetch_package_metadata_from_registry(name, version_spec)?;
        self.resolved_cache.insert(cache_key, metadata.clone());
        Ok(metadata)
    }

    fn fetch_package_metadata_from_registry(
        &self,
        name: &str,
        version_spec: &str,
    ) -> Result<PackageMetadata, String> {
        let clean_version = clean_version_string(version_spec);
        let url = if clean_version == "latest" || clean_version.is_empty() {
            format!("https://registry.npmjs.org/{name}")
        } else {
            format!("https://registry.npmjs.org/{name}/{clean_version}")
        };

        let response =
            reqwest::blocking::get(&url).map_err(|e| format!("Registry request failed: {e}"))?;

        if !response.status().is_success() {
            return Err(format!("Registry returned status: {}", response.status()));
        }

        let json: Value = response
            .json()
            .map_err(|e| format!("Failed to parse registry response: {e}"))?;

        self.parse_registry_metadata(&json, name, &clean_version)
    }

    fn parse_registry_metadata(
        &self,
        json: &Value,
        name: &str,
        requested_version: &str,
    ) -> Result<PackageMetadata, String> {
        let version_to_use = if requested_version == "latest" {
            json.get("dist-tags")
                .and_then(|tags| tags.get("latest"))
                .and_then(|v| v.as_str())
                .unwrap_or("latest")
        } else {
            requested_version
        };

        let version_data = if let Some(versions) = json.get("versions") {
            if let Some(specific_version) = versions.get(version_to_use) {
                specific_version
            } else {
                json
            }
        } else {
            json
        };

        let license = version_data
            .get("license")
            .and_then(|l| l.as_str())
            .or_else(|| {
                version_data
                    .get("licenses")
                    .and_then(|ls| ls.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|first| first.get("type"))
                    .and_then(|t| t.as_str())
            })
            .map(String::from);

        let dependencies = self.extract_dependencies_from_json(version_data, "dependencies");

        Ok(PackageMetadata {
            name: name.to_string(),
            version: version_to_use.to_string(),
            license,
            dependencies,
        })
    }

    fn extract_dependencies_from_json(
        &self,
        json: &Value,
        dep_type: &str,
    ) -> HashMap<String, String> {
        json.get(dep_type)
            .and_then(|deps| deps.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("*").to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn parse_local_package_json(&self, path: &str) -> Result<PackageMetadata, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read package.json: {e}"))?;

        let json: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse package.json: {e}"))?;

        let dependencies = self.extract_dependencies_from_json(&json, "dependencies");

        Ok(PackageMetadata {
            name: "root".to_string(),
            version: "0.0.0".to_string(),
            license: None,
            dependencies,
        })
    }
}

#[allow(dead_code)]
pub fn analyze_js_licenses(package_json_path: &str) -> Vec<LicenseInfo> {
    let config = crate::config::load_config().unwrap_or_default();
    analyze_js_licenses_with_config(package_json_path, &config, false)
}

pub fn analyze_js_licenses_with_no_local(
    package_json_path: &str,
    no_local: bool,
) -> Vec<LicenseInfo> {
    let config = crate::config::load_config().unwrap_or_default();
    analyze_js_licenses_with_config(package_json_path, &config, no_local)
}

pub fn analyze_js_licenses_with_config(
    package_json_path: &str,
    config: &crate::config::FeludaConfig,
    no_local: bool,
) -> Vec<LicenseInfo> {
    log(
        LogLevel::Info,
        &format!("Analyzing JavaScript dependencies from: {package_json_path}"),
    );

    let project_root = Path::new(package_json_path)
        .parent()
        .unwrap_or(Path::new("."));

    let all_dependencies = if project_root.join("pnpm-lock.yaml").exists() {
        log(
            LogLevel::Info,
            "Detected pnpm project - using specialized pnpm analysis",
        );
        analyze_pnpm_project_comprehensive(project_root, package_json_path)
    } else {
        log(LogLevel::Info, "Using general npm/yarn analysis");
        try_all_dependency_detection_methods(project_root, package_json_path)
    };

    if all_dependencies.is_empty() {
        log(LogLevel::Warn, "No dependencies found using any method");
        return Vec::new();
    }

    log(
        LogLevel::Info,
        &format!(
            "Successfully found {} total dependencies",
            all_dependencies.len()
        ),
    );
    log_debug(
        "All detected dependencies (first 20)",
        &all_dependencies.iter().take(20).collect::<HashMap<_, _>>(),
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

    // Process dependencies in parallel
    all_dependencies
        .par_iter()
        .map(|(name, version)| {
            let license = get_license_for_package(project_root, name, version, no_local);
            let is_restrictive =
                is_license_restrictive(&Some(license.clone()), &known_licenses, config.strict);

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    &format!("Restrictive license found: {license} for {name}"),
                );
            }

            LicenseInfo {
                name: name.to_string(),
                version: clean_version_string(version),
                license: Some(license.clone()),
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::get_osi_status(&license),
            }
        })
        .collect()
}

fn try_all_dependency_detection_methods(
    project_root: &Path,
    package_json_path: &str,
) -> HashMap<String, String> {
    let mut all_deps = HashMap::new();

    // pnpm
    if project_root.join("pnpm-lock.yaml").exists() {
        log(LogLevel::Info, "pnpm dependency detection...");

        for method in get_pnpm_methods() {
            if let Ok(deps) = method(project_root) {
                if !deps.is_empty() {
                    log(
                        LogLevel::Info,
                        &format!("pnpm method found {} dependencies", deps.len()),
                    );
                    all_deps.extend(deps);
                }
            }
        }
    }

    // yarn
    if all_deps.is_empty() && project_root.join("yarn.lock").exists() {
        log(LogLevel::Info, "yarn dependency detection...");

        for method in get_yarn_methods() {
            if let Ok(deps) = method(project_root) {
                if !deps.is_empty() {
                    log(
                        LogLevel::Info,
                        &format!("yarn method found {} dependencies", deps.len()),
                    );
                    all_deps.extend(deps);
                    break;
                }
            }
        }
    }

    // npm ls
    if all_deps.is_empty() {
        log(LogLevel::Info, "npm dependency detection...");

        for method in get_npm_methods() {
            if let Ok(deps) = method(project_root) {
                if !deps.is_empty() {
                    log(
                        LogLevel::Info,
                        &format!("npm method found {} dependencies", deps.len()),
                    );
                    all_deps.extend(deps);
                    break;
                }
            }
        }
    }

    // node_modules
    if all_deps.len() < 50 {
        log(LogLevel::Info, "node_modules scanning...");

        if let Ok(scanned_deps) = comprehensive_node_modules_scan(project_root) {
            log(
                LogLevel::Info,
                &format!(
                    "node_modules scan found {} additional dependencies",
                    scanned_deps.len()
                ),
            );
            all_deps.extend(scanned_deps);
        }
    }

    // Lockfile parsing
    if let Ok(lockfile_deps) = parse_lockfiles(project_root) {
        log(
            LogLevel::Info,
            &format!(
                "Lockfile parsing found {} additional dependencies",
                lockfile_deps.len()
            ),
        );
        all_deps.extend(lockfile_deps);
    }

    // Workspace detection
    if let Ok(workspace_deps) = detect_workspace_dependencies(project_root, package_json_path) {
        log(
            LogLevel::Info,
            &format!(
                "Workspace detection found {} additional dependencies",
                workspace_deps.len()
            ),
        );
        all_deps.extend(workspace_deps);
    }

    // recursive resolver
    if all_deps.len() < 20 {
        log(LogLevel::Info, "Using recursive resolver as final fallback");
        let mut resolver = DependencyResolver::new();
        if let Ok(recursive_deps) = resolver.resolve_recursive_dependencies(package_json_path, 15) {
            log(
                LogLevel::Info,
                &format!(
                    "Recursive resolver found {} dependencies",
                    recursive_deps.len()
                ),
            );
            all_deps.extend(recursive_deps);
        }
    }

    all_deps
}

/// Get all pnpm detection methods
fn get_pnpm_methods() -> Vec<DependencyDetector> {
    vec![
        pnpm_list_all_recursive,
        pnpm_list_json_depth_infinity,
        pnpm_list_prod_dev,
        pnpm_why_based_detection,
    ]
}

/// Get all yarn detection methods
fn get_yarn_methods() -> Vec<DependencyDetector> {
    vec![
        yarn_list_recursive,
        yarn_list_all_pattern,
        yarn_info_workspaces,
    ]
}

/// Get all npm detection methods
fn get_npm_methods() -> Vec<DependencyDetector> {
    vec![npm_ls_all_json, npm_ls_long_format, npm_list_global_style]
}

// =============================================================================
// PNPM DETECTION METHODS
// =============================================================================

fn pnpm_list_all_recursive(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(
        LogLevel::Info,
        "Trying: pnpm list --recursive --depth Infinity",
    );

    let output = Command::new("pnpm")
        .args([
            "list",
            "--recursive",
            "--depth",
            "Infinity",
            "--json",
            "--prod",
            "--dev",
        ])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("pnpm list recursive failed: {e}"))?;

    parse_pnpm_json_output(&output)
}

fn pnpm_list_json_depth_infinity(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: pnpm list --json --depth Infinity");

    let output = Command::new("pnpm")
        .args(["list", "--json", "--depth", "Infinity"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("pnpm list depth infinity failed: {e}"))?;

    parse_pnpm_json_output(&output)
}

fn pnpm_list_prod_dev(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: pnpm list --prod --dev --long");

    let output = Command::new("pnpm")
        .args(["list", "--prod", "--dev", "--long", "--depth", "999"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("pnpm list prod dev failed: {e}"))?;

    parse_pnpm_text_output(&output)
}

fn pnpm_why_based_detection(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: pnpm-based package discovery");

    let package_json_content = fs::read_to_string(project_root.join("package.json"))
        .map_err(|e| format!("Failed to read package.json: {e}"))?;

    let package_json: Value = serde_json::from_str(&package_json_content)
        .map_err(|e| format!("Failed to parse package.json: {e}"))?;

    let mut all_deps = HashMap::new();

    // Get direct dependencies
    if let Some(deps) = package_json.get("dependencies").and_then(|d| d.as_object()) {
        for (name, _) in deps {
            if let Ok(transitive) = get_pnpm_transitive_deps(project_root, name) {
                all_deps.extend(transitive);
            }
        }
    }

    Ok(all_deps)
}

fn get_pnpm_transitive_deps(
    project_root: &Path,
    package_name: &str,
) -> Result<HashMap<String, String>, String> {
    let output = Command::new("pnpm")
        .args(["why", package_name, "--json"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("pnpm why failed: {e}"))?;

    if !output.status.success() {
        return Ok(HashMap::new());
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut deps = HashMap::new();

    if let Ok(json) = serde_json::from_str::<Value>(&stdout_str) {
        extract_deps_from_pnpm_why(&json, &mut deps);
    }

    Ok(deps)
}

// =============================================================================
// YARN DETECTION METHODS
// =============================================================================

fn yarn_list_recursive(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: yarn list --recursive");

    let output = Command::new("yarn")
        .args(["list", "--recursive", "--json"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("yarn list recursive failed: {e}"))?;

    parse_yarn_json_output(&output)
}

fn yarn_list_all_pattern(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: yarn list --pattern '*'");

    let output = Command::new("yarn")
        .args(["list", "--pattern", "*", "--json"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("yarn list pattern failed: {e}"))?;

    parse_yarn_json_output(&output)
}

fn yarn_info_workspaces(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: yarn workspaces info");

    let output = Command::new("yarn")
        .args(["workspaces", "info", "--json"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("yarn workspaces info failed: {e}"))?;

    parse_yarn_workspaces_output(&output)
}

// =============================================================================
// NPM DETECTION METHODS
// =============================================================================

fn npm_ls_all_json(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: npm ls --all --json");

    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";

    let output = Command::new(NPM)
        .args(["ls", "--all", "--json", "--production", "--dev"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("npm ls all failed: {e}"))?;

    parse_npm_json_output(&output)
}

fn npm_ls_long_format(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: npm ls --long --parseable");

    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";

    let output = Command::new(NPM)
        .args(["ls", "--long", "--parseable", "--all"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("npm ls long failed: {e}"))?;

    parse_npm_parseable_output(&output)
}

fn npm_list_global_style(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Trying: npm list --global-style");

    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";

    let output = Command::new(NPM)
        .args(["list", "--global-style", "--depth", "999"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("npm list global-style failed: {e}"))?;

    parse_npm_tree_output(&output)
}

// =============================================================================
// NODE_MODULES SCANNING
// =============================================================================

fn comprehensive_node_modules_scan(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Starting comprehensive node_modules scan");

    let node_modules = project_root.join("node_modules");
    if !node_modules.exists() {
        return Ok(HashMap::new());
    }

    let mut all_packages = HashMap::new();
    let mut visited_paths = HashSet::new();

    scan_with_symlink_resolution(&node_modules, &mut all_packages, &mut visited_paths, 0)?;

    let pnpm_dir = node_modules.join(".pnpm");
    if pnpm_dir.exists() {
        log(
            LogLevel::Info,
            "Found .pnpm directory, scanning pnpm virtual store",
        );
        scan_pnpm_virtual_store(&pnpm_dir, &mut all_packages)?;
    }

    Ok(all_packages)
}

fn scan_with_symlink_resolution(
    dir: &Path,
    packages: &mut HashMap<String, String>,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > 25 || visited.contains(&dir.to_path_buf()) {
        return Ok(());
    }

    visited.insert(dir.to_path_buf());

    let entries =
        fs::read_dir(dir).map_err(|e| format!("Failed to read {}: {}", dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if name.starts_with('.') {
            continue;
        }

        if name.starts_with('@') {
            if let Ok(scoped_entries) = fs::read_dir(&path) {
                for scoped_entry in scoped_entries.flatten() {
                    let scoped_path = scoped_entry.path();
                    if scoped_path.is_dir() {
                        let scoped_name = scoped_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("");
                        let full_name = format!("{name}/{scoped_name}");

                        if let Some(version) = read_package_version_safe(&scoped_path) {
                            packages.insert(full_name, version);
                        }

                        let nested = scoped_path.join("node_modules");
                        if nested.exists() {
                            scan_with_symlink_resolution(&nested, packages, visited, depth + 1)?;
                        }
                    }
                }
            }
        } else if let Some(version) = read_package_version_safe(&path) {
            packages.insert(name.to_string(), version);

            let nested = path.join("node_modules");
            if nested.exists() {
                scan_with_symlink_resolution(&nested, packages, visited, depth + 1)?;
            }
        }
    }

    Ok(())
}

fn scan_pnpm_virtual_store(
    pnpm_dir: &Path,
    packages: &mut HashMap<String, String>,
) -> Result<(), String> {
    let entries = fs::read_dir(pnpm_dir).map_err(|e| format!("Failed to read .pnpm: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if let Some((pkg_with_version, _hash)) = dir_name.split_once('_') {
                if let Some((pkg_name, version)) = pkg_with_version.rsplit_once('@') {
                    let clean_name =
                        if pkg_name.starts_with('@') && pkg_name.matches('@').count() == 2 {
                            if let Some(at_pos) = pkg_name[1..].find('@') {
                                format!("@{}", &pkg_name[at_pos + 2..])
                            } else {
                                pkg_name.to_string()
                            }
                        } else {
                            pkg_name.to_string()
                        };

                    packages.insert(clean_name, version.to_string());

                    let inner_node_modules = path.join("node_modules");
                    if inner_node_modules.exists() {
                        let mut visited = HashSet::new();
                        let _ = scan_with_symlink_resolution(
                            &inner_node_modules,
                            packages,
                            &mut visited,
                            0,
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

// =============================================================================
// LOCKFILE PARSING
// =============================================================================

fn parse_lockfiles(project_root: &Path) -> Result<HashMap<String, String>, String> {
    let mut deps = HashMap::new();

    // Parse pnpm-lock.yaml
    if let Some(pnpm_deps) = parse_pnpm_lockfile(project_root) {
        deps.extend(pnpm_deps);
    }

    // Parse yarn.lock
    if let Some(yarn_deps) = parse_yarn_lockfile(project_root) {
        deps.extend(yarn_deps);
    }

    // Parse package-lock.json
    if let Some(npm_deps) = parse_npm_lockfile(project_root) {
        deps.extend(npm_deps);
    }

    Ok(deps)
}

fn parse_pnpm_lockfile(project_root: &Path) -> Option<HashMap<String, String>> {
    let lockfile_path = project_root.join("pnpm-lock.yaml");
    if !lockfile_path.exists() {
        return None;
    }

    log(LogLevel::Info, "Parsing pnpm-lock.yaml");

    if let Ok(content) = fs::read_to_string(&lockfile_path) {
        let mut deps = HashMap::new();

        for line in content.lines() {
            if line.trim().starts_with('/') && line.contains(':') {
                if let Some(pkg_info) = line.trim().strip_prefix('/') {
                    if let Some(colon_pos) = pkg_info.find(':') {
                        let pkg_with_version = &pkg_info[..colon_pos];
                        if let Some((pkg_name, version)) = pkg_with_version.rsplit_once('@') {
                            deps.insert(pkg_name.to_string(), version.to_string());
                        }
                    }
                }
            }
        }

        log(
            LogLevel::Info,
            &format!("Parsed {} dependencies from pnpm-lock.yaml", deps.len()),
        );
        Some(deps)
    } else {
        None
    }
}

fn parse_yarn_lockfile(project_root: &Path) -> Option<HashMap<String, String>> {
    let lockfile_path = project_root.join("yarn.lock");
    if !lockfile_path.exists() {
        return None;
    }

    log(LogLevel::Info, "Parsing yarn.lock");

    if let Ok(content) = fs::read_to_string(&lockfile_path) {
        let mut deps = HashMap::new();
        let mut current_package = None;

        for line in content.lines() {
            let trimmed = line.trim();

            if !trimmed.is_empty()
                && !trimmed.starts_with(' ')
                && trimmed.contains('@')
                && trimmed.ends_with(':')
            {
                let package_line = trimmed.trim_end_matches(':');
                if let Some((name, _range)) = package_line.split_once('@') {
                    current_package = Some(name.trim_matches('"').to_string());
                }
            }

            if let Some(version_line) = trimmed.strip_prefix("version ") {
                if let Some(ref pkg_name) = current_package {
                    let version = version_line.trim_matches('"');
                    deps.insert(pkg_name.clone(), version.to_string());
                    current_package = None;
                }
            }
        }

        log(
            LogLevel::Info,
            &format!("Parsed {} dependencies from yarn.lock", deps.len()),
        );
        Some(deps)
    } else {
        None
    }
}

fn parse_npm_lockfile(project_root: &Path) -> Option<HashMap<String, String>> {
    let lockfile_path = project_root.join("package-lock.json");
    if !lockfile_path.exists() {
        return None;
    }

    log(LogLevel::Info, "Parsing package-lock.json");

    if let Ok(content) = fs::read_to_string(&lockfile_path) {
        if let Ok(json) = serde_json::from_str::<Value>(&content) {
            let mut deps = HashMap::new();

            if let Some(packages) = json.get("packages").and_then(|p| p.as_object()) {
                for (path, info) in packages {
                    if !path.is_empty() && !path.starts_with("node_modules/") {
                        continue;
                    }

                    if let Some(name) = info.get("name").and_then(|n| n.as_str()) {
                        if let Some(version) = info.get("version").and_then(|v| v.as_str()) {
                            deps.insert(name.to_string(), version.to_string());
                        }
                    }
                }
            }

            log(
                LogLevel::Info,
                &format!("Parsed {} dependencies from package-lock.json", deps.len()),
            );
            Some(deps)
        } else {
            None
        }
    } else {
        None
    }
}

// =============================================================================
// WORKSPACE DETECTION
// =============================================================================

fn detect_workspace_dependencies(
    project_root: &Path,
    package_json_path: &str,
) -> Result<HashMap<String, String>, String> {
    let mut workspace_deps = HashMap::new();

    if let Ok(content) = fs::read_to_string(package_json_path) {
        if let Ok(json) = serde_json::from_str::<Value>(&content) {
            if let Some(workspaces) = json.get("workspaces") {
                log(LogLevel::Info, "Detected workspace configuration");

                let workspace_patterns = if let Some(array) = workspaces.as_array() {
                    array.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()
                } else if let Some(obj) = workspaces.as_object() {
                    if let Some(packages) = obj.get("packages").and_then(|p| p.as_array()) {
                        packages
                            .iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };

                for pattern in workspace_patterns {
                    if let Ok(workspace_deps_found) = scan_workspace_pattern(project_root, pattern)
                    {
                        workspace_deps.extend(workspace_deps_found);
                    }
                }
            }
        }
    }

    Ok(workspace_deps)
}

fn scan_workspace_pattern(
    project_root: &Path,
    pattern: &str,
) -> Result<HashMap<String, String>, String> {
    let mut deps = HashMap::new();

    let pattern_path = if let Some(stripped) = pattern.strip_suffix("/*") {
        project_root.join(stripped)
    } else {
        project_root.join(pattern)
    };

    if pattern_path.exists() && pattern_path.is_dir() {
        if pattern.ends_with("/*") {
            if let Ok(entries) = fs::read_dir(&pattern_path) {
                for entry in entries.flatten() {
                    let workspace_path = entry.path();
                    if workspace_path.is_dir() {
                        let workspace_package_json = workspace_path.join("package.json");
                        if workspace_package_json.exists() {
                            let workspace_deps_found = try_all_dependency_detection_methods(
                                &workspace_path,
                                workspace_package_json.to_str().unwrap_or(""),
                            );
                            deps.extend(workspace_deps_found);
                        }
                    }
                }
            }
        } else {
            let workspace_package_json = pattern_path.join("package.json");
            if workspace_package_json.exists() {
                let workspace_deps_found = try_all_dependency_detection_methods(
                    &pattern_path,
                    workspace_package_json.to_str().unwrap_or(""),
                );
                deps.extend(workspace_deps_found);
            }
        }
    }

    Ok(deps)
}

// =============================================================================
// PARSER HELPER FUNCTIONS
// =============================================================================

fn parse_pnpm_json_output(
    output: &std::process::Output,
) -> Result<HashMap<String, String>, String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("pnpm command failed: {stderr}"));
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    if let Ok(json) = serde_json::from_str::<Value>(&stdout_str) {
        extract_dependencies_from_json(&json, &mut dependencies);
    }

    Ok(dependencies)
}

fn parse_pnpm_text_output(
    output: &std::process::Output,
) -> Result<HashMap<String, String>, String> {
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    for line in stdout_str.lines() {
        if let Some((name, version)) = parse_dependency_line(line) {
            dependencies.insert(name, version);
        }
    }

    Ok(dependencies)
}

fn parse_yarn_json_output(
    output: &std::process::Output,
) -> Result<HashMap<String, String>, String> {
    if !output.status.success() {
        return Err("yarn command failed".to_string());
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    for line in stdout_str.lines() {
        if let Ok(json) = serde_json::from_str::<Value>(line) {
            if json.get("type").and_then(|t| t.as_str()) == Some("tree") {
                if let Some(data) = json.get("data") {
                    extract_dependencies_from_json(data, &mut dependencies);
                }
            }
        }
    }

    Ok(dependencies)
}

fn parse_yarn_workspaces_output(
    output: &std::process::Output,
) -> Result<HashMap<String, String>, String> {
    if !output.status.success() {
        return Err("yarn workspaces command failed".to_string());
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let dependencies = HashMap::new();

    if let Ok(json) = serde_json::from_str::<Value>(&stdout_str) {
        if let Some(workspaces) = json.as_object() {
            for (_workspace_name, workspace_info) in workspaces {
                if let Some(location) = workspace_info.get("location").and_then(|l| l.as_str()) {
                    log(LogLevel::Info, &format!("Found workspace at: {location}"));
                }
            }
        }
    }

    Ok(dependencies)
}

fn parse_npm_json_output(output: &std::process::Output) -> Result<HashMap<String, String>, String> {
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if !output.status.success() {
        log(
            LogLevel::Warn,
            &format!(
                "npm command had non-zero exit: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        );
    }

    let mut dependencies = HashMap::new();

    if let Ok(json) = serde_json::from_str::<Value>(&stdout_str) {
        extract_dependencies_from_json(&json, &mut dependencies);
    }

    Ok(dependencies)
}

fn parse_npm_parseable_output(
    output: &std::process::Output,
) -> Result<HashMap<String, String>, String> {
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    for line in stdout_str.lines() {
        if line.contains("node_modules") {
            if let Some(package_path) = line.split("node_modules/").last() {
                let parts: Vec<&str> = package_path.split('/').collect();
                let package_name = if parts[0].starts_with('@') && parts.len() > 1 {
                    format!("{}/{}", parts[0], parts[1])
                } else {
                    parts[0].to_string()
                };

                if let Some(version) = read_package_version_from_path(line) {
                    dependencies.insert(package_name, version);
                }
            }
        }
    }

    Ok(dependencies)
}

fn parse_npm_tree_output(output: &std::process::Output) -> Result<HashMap<String, String>, String> {
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    for line in stdout_str.lines() {
        if let Some((name, version)) = parse_dependency_line(line) {
            dependencies.insert(name, version);
        }
    }

    Ok(dependencies)
}

fn extract_dependencies_from_json(json: &Value, dependencies: &mut HashMap<String, String>) {
    if let Some(deps) = json.get("dependencies").and_then(|d| d.as_object()) {
        extract_deps_recursive(deps, dependencies);
    }

    // pnpm structure
    if let Some(projects) = json.as_array() {
        for project in projects {
            if let Some(deps) = project.get("dependencies").and_then(|d| d.as_object()) {
                extract_deps_recursive(deps, dependencies);
            }
            if let Some(dev_deps) = project.get("devDependencies").and_then(|d| d.as_object()) {
                extract_deps_recursive(dev_deps, dependencies);
            }
        }
    }

    // Yarn tree structure
    if let Some(trees) = json.get("trees").and_then(|t| t.as_array()) {
        for tree in trees {
            if let Some(name) = tree.get("name").and_then(|n| n.as_str()) {
                if let Some((pkg_name, version)) = name.rsplit_once('@') {
                    dependencies.insert(pkg_name.to_string(), version.to_string());
                }
            }
        }
    }
}

fn extract_deps_recursive(
    deps: &serde_json::Map<String, Value>,
    all_deps: &mut HashMap<String, String>,
) {
    for (name, dep_info) in deps {
        if let Some(version) = dep_info.get("version").and_then(|v| v.as_str()) {
            all_deps.insert(name.clone(), version.to_string());
        }

        // Recursive extract nested dependencies
        if let Some(nested_deps) = dep_info.get("dependencies").and_then(|d| d.as_object()) {
            extract_deps_recursive(nested_deps, all_deps);
        }
    }
}

fn extract_deps_from_pnpm_why(json: &Value, deps: &mut HashMap<String, String>) {
    if let Some(dependents) = json.get("dependents").and_then(|d| d.as_array()) {
        for dependent in dependents {
            if let Some(from) = dependent.get("from").and_then(|f| f.as_str()) {
                if let Some((name, version)) = from.rsplit_once('@') {
                    deps.insert(name.to_string(), version.to_string());
                }
            }
        }
    }
}

fn parse_dependency_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();

    // Handle tree output like "├── package@1.0.0" or "└── package@1.0.0"
    let clean_line = trimmed
        .trim_start_matches("├── ")
        .trim_start_matches("└── ")
        .trim_start_matches("│   ")
        .trim_start_matches("    ");

    if let Some(at_pos) = clean_line.rfind('@') {
        let name_part = &clean_line[..at_pos];
        let version_part = &clean_line[at_pos + 1..];

        if version_part
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
        {
            return Some((name_part.to_string(), version_part.to_string()));
        }
    }

    None
}

fn read_package_version_safe(package_dir: &Path) -> Option<String> {
    let package_json_path = package_dir.join("package.json");

    match fs::read_to_string(&package_json_path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(json) => json
                .get("version")
                .and_then(|v| v.as_str())
                .map(String::from),
            Err(_) => None,
        },
        Err(_) => None,
    }
}

fn read_package_version_from_path(path: &str) -> Option<String> {
    let path_buf = PathBuf::from(path);
    read_package_version_safe(&path_buf)
}

// =============================================================================
// LICENSE DETECTION
// =============================================================================

fn get_license_for_package(
    project_root: &Path,
    name: &str,
    version: &str,
    no_local: bool,
) -> String {
    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";

    let mut result = get_license_from_package_json(project_root, name, version);

    if result.is_none() && !no_local {
        result = get_license_from_local_license_file(project_root, name);
    }

    result
        .or_else(|| get_license_from_pnpm_metadata(project_root, name, version))
        .or_else(|| get_license_from_npm_view(NPM, name, version))
        .or_else(|| get_license_from_npm_registry_api(name, version))
        .unwrap_or_else(|| "Unknown (failed to retrieve)".to_string())
}

fn get_license_from_package_json(
    project_root: &Path,
    package_name: &str,
    _version: &str,
) -> Option<String> {
    let possible_paths = vec![
        if package_name.starts_with('@') {
            let parts: Vec<&str> = package_name.splitn(2, '/').collect();
            if parts.len() == 2 {
                Some(
                    project_root
                        .join("node_modules")
                        .join(parts[0])
                        .join(parts[1])
                        .join("package.json"),
                )
            } else {
                None
            }
        } else {
            Some(
                project_root
                    .join("node_modules")
                    .join(package_name)
                    .join("package.json"),
            )
        },
        if package_name.starts_with('@') {
            let parts: Vec<&str> = package_name.splitn(2, '/').collect();
            if parts.len() == 2 {
                Some(
                    project_root
                        .join("node_modules")
                        .join(".pnpm")
                        .join("node_modules")
                        .join(parts[0])
                        .join(parts[1])
                        .join("package.json"),
                )
            } else {
                None
            }
        } else {
            Some(
                project_root
                    .join("node_modules")
                    .join(".pnpm")
                    .join("node_modules")
                    .join(package_name)
                    .join("package.json"),
            )
        },
    ];

    for package_path in possible_paths.into_iter().flatten() {
        if let Ok(content) = fs::read_to_string(&package_path) {
            if let Ok(json) = serde_json::from_str::<Value>(&content) {
                if let Some(license) = json.get("license").and_then(|l| l.as_str()) {
                    if !license.is_empty() && license != "UNLICENSED" {
                        log(
                            LogLevel::Info,
                            &format!("Found license in package.json for {package_name}: {license}"),
                        );
                        return Some(license.to_string());
                    }
                }

                if let Some(licenses) = json.get("licenses").and_then(|l| l.as_array()) {
                    if let Some(first_license) = licenses.first() {
                        if let Some(license_type) =
                            first_license.get("type").and_then(|t| t.as_str())
                        {
                            log(
                                LogLevel::Info,
                                &format!(
                                    "Found license in licenses array for {package_name}: {license_type}"
                                ),
                            );
                            return Some(license_type.to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

fn get_license_from_npm_view(npm_cmd: &str, package_name: &str, version: &str) -> Option<String> {
    let clean_version = clean_version_string(version);
    let package_spec = if clean_version == "latest" || clean_version.is_empty() {
        package_name.to_string()
    } else {
        format!("{package_name}@{clean_version}")
    };

    log(
        LogLevel::Info,
        &format!("Trying npm view for: {package_spec}"),
    );

    let output = Command::new(npm_cmd)
        .arg("view")
        .arg(&package_spec)
        .arg("license")
        .arg("--json")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let output_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<Value>(&output_str) {
        if let Some(license) = json.as_str() {
            return Some(license.to_string());
        }
    }

    let license = output_str.trim().trim_matches('"');
    if !license.is_empty() && license != "undefined" {
        Some(license.to_string())
    } else {
        None
    }
}

fn get_license_from_npm_registry_api(package_name: &str, version: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Trying npm registry API for {package_name}"),
    );

    let versions_to_try = if version == "latest" || version.is_empty() {
        vec!["latest"]
    } else {
        vec![version, "latest"]
    };

    for ver in versions_to_try {
        let url = if ver == "latest" {
            format!("https://registry.npmjs.org/{package_name}")
        } else {
            format!("https://registry.npmjs.org/{package_name}/{ver}")
        };

        if let Ok(response) = reqwest::blocking::get(&url) {
            if response.status().is_success() {
                if let Ok(json) = response.json::<Value>() {
                    let license_paths = [
                        vec!["license"],
                        vec!["licenses", "0", "type"],
                        vec!["licenses", "0"],
                        vec!["latest", "license"],
                    ];

                    for path in &license_paths {
                        if let Some(license_value) = get_nested_json_value(&json, path) {
                            if let Some(license_str) = license_value.as_str() {
                                if !license_str.is_empty() && license_str != "UNLICENSED" {
                                    log(
                                        LogLevel::Info,
                                        &format!(
                                            "Found license via registry API for {package_name}: {license_str}"
                                        ),
                                    );
                                    return Some(license_str.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn get_license_from_pnpm_metadata(
    project_root: &Path,
    package_name: &str,
    version: &str,
) -> Option<String> {
    let pnpm_meta_path = project_root.join("node_modules").join(".pnpm");

    if pnpm_meta_path.exists() {
        let expected_dir_name = format!("{package_name}@{version}");

        if let Ok(entries) = fs::read_dir(&pnpm_meta_path) {
            for entry in entries.flatten() {
                let dir_name = entry.file_name();
                let dir_name_str = dir_name.to_string_lossy();

                if dir_name_str.starts_with(&expected_dir_name) {
                    let package_json_path = entry
                        .path()
                        .join("node_modules")
                        .join(package_name)
                        .join("package.json");
                    if let Ok(content) = fs::read_to_string(&package_json_path) {
                        if let Ok(json) = serde_json::from_str::<Value>(&content) {
                            if let Some(license) = json.get("license").and_then(|l| l.as_str()) {
                                if !license.is_empty() && license != "UNLICENSED" {
                                    return Some(license.to_string());
                                }
                            }
                        }
                    }
                    break;
                }
            }
        }
    }

    None
}

fn get_license_from_local_license_file(project_root: &Path, package_name: &str) -> Option<String> {
    let package_dirs = if package_name.starts_with('@') {
        let parts: Vec<&str> = package_name.splitn(2, '/').collect();
        if parts.len() == 2 {
            vec![
                project_root
                    .join("node_modules")
                    .join(parts[0])
                    .join(parts[1]),
                project_root
                    .join("node_modules")
                    .join(".pnpm")
                    .join("node_modules")
                    .join(parts[0])
                    .join(parts[1]),
            ]
        } else {
            return None;
        }
    } else {
        vec![
            project_root.join("node_modules").join(package_name),
            project_root
                .join("node_modules")
                .join(".pnpm")
                .join("node_modules")
                .join(package_name),
        ]
    };

    let license_filenames = [
        "LICENSE",
        "LICENSE.md",
        "LICENSE.txt",
        "COPYING",
        "COPYING.md",
    ];

    for dir in package_dirs {
        if !dir.exists() {
            continue;
        }

        for filename in &license_filenames {
            let license_path = dir.join(filename);
            if license_path.exists() {
                if let Ok(content) = fs::read_to_string(&license_path) {
                    if !content.trim().is_empty() {
                        log(
                            LogLevel::Info,
                            &format!("Found license file for {package_name}: {filename}"),
                        );
                        return detect_license_from_content(&content);
                    }
                }
            }
        }
    }

    None
}

fn detect_license_from_content(content: &str) -> Option<String> {
    let content_upper = content.to_uppercase();

    let patterns = vec![
        ("MIT", "MIT License"),
        ("APACHE", "Apache License"),
        ("GPL", "GPL"),
        ("BSD", "BSD"),
        ("ISC", "ISC License"),
        ("LGPL", "LGPL"),
        ("UNLICENSE", "Unlicense"),
        ("MPL", "Mozilla Public License"),
    ];

    for (pattern, label) in patterns {
        if content_upper.contains(pattern) {
            return Some(label.to_string());
        }
    }

    None
}

fn get_nested_json_value<'a>(json: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = json;
    for key in path {
        current = current.get(key)?;
    }
    Some(current)
}

fn clean_version_string(version: &str) -> String {
    version
        .trim_start_matches('^')
        .trim_start_matches('~')
        .trim_start_matches(">=")
        .trim_start_matches('>')
        .trim_start_matches("<=")
        .trim_start_matches('<')
        .trim_start_matches('=')
        .split_whitespace()
        .next()
        .unwrap_or(version)
        .to_string()
}

#[allow(dead_code)]
fn parse_package_json_dependencies(
    package_json_path: &str,
) -> Result<HashMap<String, String>, String> {
    log(
        LogLevel::Info,
        &format!("Parsing dependencies directly from: {package_json_path}"),
    );

    let content = std::fs::read_to_string(package_json_path)
        .map_err(|e| format!("Failed to read package.json: {e}"))?;

    let package_json: PackageJson =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse package.json: {e}"))?;

    let all_deps = package_json.get_all_dependencies();

    log(
        LogLevel::Info,
        &format!(
            "Found {} total dependencies in package.json",
            all_deps.len()
        ),
    );

    log_debug("Dependencies from package.json", &all_deps);

    Ok(all_deps)
}

fn analyze_pnpm_project_comprehensive(
    project_root: &Path,
    _package_json_path: &str,
) -> HashMap<String, String> {
    let mut all_deps = HashMap::new();

    log(
        LogLevel::Info,
        "Method 1: Comprehensive pnpm-lock.yaml parsing",
    );
    if let Ok(lockfile_deps) = parse_pnpm_lockfile_comprehensive(project_root) {
        log(
            LogLevel::Info,
            &format!(
                "pnpm-lock.yaml parsing found {} dependencies",
                lockfile_deps.len()
            ),
        );
        all_deps.extend(lockfile_deps);
    }

    log(LogLevel::Info, "Method 2: pnpm list commands");
    let before_pnpm_commands = all_deps.len();

    if let Ok(deps) = try_pnpm_list_all_dependencies(project_root) {
        if !deps.is_empty() {
            log(
                LogLevel::Info,
                &format!("pnpm list all found {} dependencies", deps.len()),
            );
            all_deps.extend(deps);
        }
    }

    if all_deps.len() == before_pnpm_commands {
        for pnpm_method in [
            try_pnpm_list_comprehensive,
            try_pnpm_list_flat,
            try_pnpm_list_recursive_json,
        ] {
            if let Ok(deps) = pnpm_method(project_root) {
                if !deps.is_empty() {
                    log(
                        LogLevel::Info,
                        &format!("pnpm command found {} dependencies", deps.len()),
                    );
                    all_deps.extend(deps);
                    break;
                }
            }
        }
    }

    log(
        LogLevel::Info,
        &format!(
            "Total after pnpm commands: {} (added {})",
            all_deps.len(),
            all_deps.len().saturating_sub(before_pnpm_commands)
        ),
    );

    log(LogLevel::Info, "Method 3: Enhanced lockfile parsing");
    let before_enhanced_lockfile = all_deps.len();
    if let Ok(enhanced_deps) = parse_pnpm_lockfile_enhanced(project_root) {
        log(
            LogLevel::Info,
            &format!(
                "Enhanced lockfile parsing found {} dependencies",
                enhanced_deps.len()
            ),
        );
        all_deps.extend(enhanced_deps);
    }
    log(
        LogLevel::Info,
        &format!(
            "Total after enhanced lockfile: {} (added {})",
            all_deps.len(),
            all_deps.len().saturating_sub(before_enhanced_lockfile)
        ),
    );

    log(LogLevel::Info, "Method 4: .pnpm virtual store analysis");
    let before_virtual_store = all_deps.len();
    if let Ok(virtual_store_deps) = analyze_pnpm_virtual_store_comprehensive(project_root) {
        log(
            LogLevel::Info,
            &format!(
                "Virtual store analysis found {} dependencies",
                virtual_store_deps.len()
            ),
        );
        all_deps.extend(virtual_store_deps);
    }
    log(
        LogLevel::Info,
        &format!(
            "Total after virtual store: {} (added {})",
            all_deps.len(),
            all_deps.len().saturating_sub(before_virtual_store)
        ),
    );

    log(LogLevel::Info, "Method 5: node_modules symlink resolution");
    let before_symlinks = all_deps.len();
    if let Ok(symlink_deps) = resolve_pnpm_symlinks(project_root) {
        log(
            LogLevel::Info,
            &format!(
                "Symlink resolution found {} dependencies",
                symlink_deps.len()
            ),
        );
        all_deps.extend(symlink_deps);
    }
    log(
        LogLevel::Info,
        &format!(
            "Total after symlinks: {} (added {})",
            all_deps.len(),
            all_deps.len().saturating_sub(before_symlinks)
        ),
    );

    log(LogLevel::Info, "Method 6: Deep .pnpm directory scanning");
    let before_deep_scan = all_deps.len();
    if let Ok(deep_scan_deps) = deep_scan_pnpm_store(project_root) {
        log(
            LogLevel::Info,
            &format!(
                "Deep .pnpm scan found {} dependencies",
                deep_scan_deps.len()
            ),
        );
        all_deps.extend(deep_scan_deps);
    }
    log(
        LogLevel::Info,
        &format!(
            "Total after deep scan: {} (added {})",
            all_deps.len(),
            all_deps.len().saturating_sub(before_deep_scan)
        ),
    );

    if all_deps.len() < 200 {
        log(LogLevel::Info, "Method 7: node_modules scan");
        let before_fallback = all_deps.len();
        if let Ok(fallback_deps) = comprehensive_node_modules_scan(project_root) {
            log(
                LogLevel::Info,
                &format!(
                    "Comprehensive scan found {} dependencies",
                    fallback_deps.len()
                ),
            );
            all_deps.extend(fallback_deps);
        }
        log(
            LogLevel::Info,
            &format!(
                "Total after comprehensive scan: {} (added {})",
                all_deps.len(),
                all_deps.len().saturating_sub(before_fallback)
            ),
        );
    }

    all_deps
}

fn parse_pnpm_lockfile_comprehensive(
    project_root: &Path,
) -> Result<HashMap<String, String>, String> {
    let lockfile_path = project_root.join("pnpm-lock.yaml");
    if !lockfile_path.exists() {
        return Err("pnpm-lock.yaml not found".to_string());
    }

    log(LogLevel::Info, "Parsing pnpm-lock.yaml comprehensively");

    let content = fs::read_to_string(&lockfile_path)
        .map_err(|e| format!("Failed to read pnpm-lock.yaml: {e}"))?;

    let mut deps = HashMap::new();
    let mut in_packages_section = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "packages:" {
            in_packages_section = true;
            continue;
        }

        if !trimmed.is_empty()
            && !trimmed.starts_with(' ')
            && trimmed.ends_with(':')
            && in_packages_section
            && trimmed != "packages:"
        {
            in_packages_section = false;
            continue;
        }

        if in_packages_section && trimmed.starts_with('/') && trimmed.contains(':') {
            if let Some(pkg_info) = trimmed.strip_prefix('/').and_then(|s| s.strip_suffix(':')) {
                if let Some((pkg_name, version)) = parse_pnpm_package_entry(pkg_info) {
                    deps.insert(pkg_name, version);
                }
            }
        }

        if trimmed.contains('@') && trimmed.contains(':') && !trimmed.starts_with('#') {
            if let Some((pkg_name, version)) = extract_package_from_lockfile_line(trimmed) {
                deps.insert(pkg_name, version);
            }
        }
    }

    log(
        LogLevel::Info,
        &format!(
            "Comprehensive pnpm-lock.yaml parsing found {} dependencies",
            deps.len()
        ),
    );
    Ok(deps)
}

fn parse_pnpm_package_entry(pkg_info: &str) -> Option<(String, String)> {
    let clean_info = pkg_info.split('(').next().unwrap_or(pkg_info);
    let clean_info = clean_info.split('_').next().unwrap_or(clean_info);

    if let Some(at_pos) = clean_info.rfind('@') {
        let name_part = &clean_info[..at_pos];
        let version_part = &clean_info[at_pos + 1..];

        if version_part
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
        {
            return Some((name_part.to_string(), version_part.to_string()));
        }
    }

    None
}

fn extract_package_from_lockfile_line(line: &str) -> Option<(String, String)> {
    if line.contains("resolution:") {
        return None;
    }

    if let Some(colon_pos) = line.find(':') {
        let name_part = line[..colon_pos].trim();
        let version_part = line[colon_pos + 1..].trim();

        if name_part.is_empty() || version_part.is_empty() {
            return None;
        }

        if name_part.contains('/') && !name_part.starts_with('@') {
            return None;
        }

        if version_part
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
        {
            return Some((name_part.to_string(), version_part.to_string()));
        }
    }

    None
}

fn try_pnpm_list_comprehensive(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(
        LogLevel::Info,
        "Attempting: pnpm list --depth=Infinity --json --prod --dev",
    );

    let output = Command::new("pnpm")
        .args([
            "list",
            "--depth=Infinity",
            "--json",
            "--prod",
            "--dev",
            "--no-optional",
        ])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("pnpm list comprehensive failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("pnpm list failed: {stderr}"));
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    if let Ok(json) = serde_json::from_str::<Value>(&stdout_str) {
        if let Some(projects) = json.as_array() {
            for project in projects {
                extract_all_pnpm_dependencies(project, &mut dependencies);
            }
        } else {
            extract_all_pnpm_dependencies(&json, &mut dependencies);
        }
    }

    Ok(dependencies)
}

fn extract_all_pnpm_dependencies(project: &Value, deps: &mut HashMap<String, String>) {
    let dep_types = [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ];

    for dep_type in &dep_types {
        if let Some(dep_obj) = project.get(dep_type).and_then(|d| d.as_object()) {
            extract_deps_recursive_pnpm(dep_obj, deps);
        }
    }
}

fn extract_deps_recursive_pnpm(
    deps_obj: &serde_json::Map<String, Value>,
    all_deps: &mut HashMap<String, String>,
) {
    for (name, dep_info) in deps_obj {
        if let Some(version) = dep_info.get("version").and_then(|v| v.as_str()) {
            all_deps.insert(name.clone(), version.to_string());
        }

        if let Some(nested_deps) = dep_info.get("dependencies").and_then(|d| d.as_object()) {
            extract_deps_recursive_pnpm(nested_deps, all_deps);
        }
    }
}

fn try_pnpm_list_flat(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(
        LogLevel::Info,
        "Attempting: pnpm list --depth=Infinity (flat output)",
    );

    let output = Command::new("pnpm")
        .args(["list", "--depth=Infinity"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("pnpm list flat failed: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    for line in stdout_str.lines() {
        if let Some((name, version)) = parse_pnpm_tree_line(line) {
            dependencies.insert(name, version);
        }
    }

    Ok(dependencies)
}

fn parse_pnpm_tree_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();

    let clean_line = trimmed
        .trim_start_matches("├── ")
        .trim_start_matches("└── ")
        .trim_start_matches("│   ");

    let parts: Vec<&str> = clean_line.split_whitespace().collect();
    if parts.len() >= 2 {
        let name = parts[0];
        let version = parts[1];

        if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Some((name.to_string(), version.to_string()));
        }
    }

    None
}

fn try_pnpm_list_recursive_json(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(LogLevel::Info, "Attempting: pnpm list --recursive --json");

    let output = Command::new("pnpm")
        .args(["list", "--recursive", "--json"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("pnpm list recursive json failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("pnpm list recursive failed: {stderr}"));
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    if let Ok(json) = serde_json::from_str::<Value>(&stdout_str) {
        if let Some(projects) = json.as_array() {
            for project in projects {
                extract_all_pnpm_dependencies(project, &mut dependencies);
            }
        }
    }

    Ok(dependencies)
}

fn analyze_pnpm_virtual_store_comprehensive(
    project_root: &Path,
) -> Result<HashMap<String, String>, String> {
    let pnpm_dir = project_root.join("node_modules").join(".pnpm");
    if !pnpm_dir.exists() {
        return Ok(HashMap::new());
    }

    log(LogLevel::Info, "Analyzing .pnpm virtual store");
    let mut packages = HashMap::new();

    let entries =
        fs::read_dir(&pnpm_dir).map_err(|e| format!("Failed to read .pnpm directory: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if let Some((pkg_name, version)) = parse_pnpm_virtual_store_entry(dir_name) {
                packages.insert(pkg_name.clone(), version);

                let nested_modules = path.join("node_modules");
                if nested_modules.exists() {
                    if let Ok(nested_deps) = scan_nested_node_modules(&nested_modules, 0) {
                        packages.extend(nested_deps);
                    }
                }
            }
        }
    }

    log(
        LogLevel::Info,
        &format!("Virtual store analysis found {} packages", packages.len()),
    );
    Ok(packages)
}

fn deep_scan_pnpm_store(project_root: &Path) -> Result<HashMap<String, String>, String> {
    let pnpm_dir = project_root.join("node_modules").join(".pnpm");
    if !pnpm_dir.exists() {
        return Ok(HashMap::new());
    }

    log(LogLevel::Info, "Deep scanning .pnpm directory structure");
    let mut packages = HashMap::new();

    scan_pnpm_directory_recursive(&pnpm_dir, &mut packages, 0)?;

    log(
        LogLevel::Info,
        &format!("Deep .pnpm scan found {} packages", packages.len()),
    );
    Ok(packages)
}

fn scan_pnpm_directory_recursive(
    dir: &Path,
    packages: &mut HashMap<String, String>,
    depth: usize,
) -> Result<(), String> {
    if depth > 10 {
        return Ok(());
    }

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if path.is_dir() {
                if let Some((pkg_name, version)) = parse_any_pnpm_directory_name(name) {
                    packages.insert(pkg_name, version);
                }

                let node_modules_path = path.join("node_modules");
                if node_modules_path.exists() {
                    scan_all_packages_in_node_modules(&node_modules_path, packages)?;
                }

                scan_pnpm_directory_recursive(&path, packages, depth + 1)?;
            }
        }
    }

    Ok(())
}

fn parse_any_pnpm_directory_name(dir_name: &str) -> Option<(String, String)> {
    if let Some((pkg_with_version, _hash)) = dir_name.split_once('_') {
        let pkg_with_version = pkg_with_version.replace('+', "/");
        if let Some((pkg_name, version)) = pkg_with_version.rsplit_once('@') {
            if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some((pkg_name.to_string(), version.to_string()));
            }
        }
    }

    if let Some((pkg_name, version)) = dir_name.rsplit_once('@') {
        if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Some((pkg_name.replace('+', "/"), version.to_string()));
        }
    }

    None
}

fn scan_all_packages_in_node_modules(
    node_modules: &Path,
    packages: &mut HashMap<String, String>,
) -> Result<(), String> {
    if let Ok(entries) = fs::read_dir(node_modules) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if name.starts_with('.') {
                    continue;
                }

                if name.starts_with('@') {
                    if let Ok(scoped_entries) = fs::read_dir(&path) {
                        for scoped_entry in scoped_entries.flatten() {
                            let scoped_path = scoped_entry.path();
                            if scoped_path.is_dir() {
                                let scoped_name = scoped_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("");
                                let full_name = format!("{name}/{scoped_name}");

                                if let Some(version) = read_package_version_safe(&scoped_path) {
                                    packages.insert(full_name, version);
                                }
                            }
                        }
                    }
                } else if let Some(version) = read_package_version_safe(&path) {
                    packages.insert(name.to_string(), version);
                }
            }
        }
    }
    Ok(())
}

fn try_pnpm_list_all_dependencies(project_root: &Path) -> Result<HashMap<String, String>, String> {
    log(
        LogLevel::Info,
        "Attempting: pnpm list --all --depth=Infinity --json",
    );

    let output = Command::new("pnpm")
        .args([
            "list",
            "--all",
            "--depth=Infinity",
            "--json",
            "--prod",
            "--dev",
            "--optional",
        ])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("pnpm list all failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log(LogLevel::Warn, &format!("pnpm list --all failed: {stderr}"));
        return Err(format!("pnpm list all failed: {stderr}"));
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = HashMap::new();

    if let Ok(json) = serde_json::from_str::<Value>(&stdout_str) {
        extract_all_pnpm_dependencies(&json, &mut dependencies);
    }

    Ok(dependencies)
}

fn parse_pnpm_lockfile_enhanced(project_root: &Path) -> Result<HashMap<String, String>, String> {
    let lockfile_path = project_root.join("pnpm-lock.yaml");
    if !lockfile_path.exists() {
        return Err("pnpm-lock.yaml not found".to_string());
    }

    log(LogLevel::Info, "Enhanced parsing of pnpm-lock.yaml");

    let content = fs::read_to_string(&lockfile_path)
        .map_err(|e| format!("Failed to read pnpm-lock.yaml: {e}"))?;

    let mut deps = HashMap::new();
    let mut current_section = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.ends_with(':') && !trimmed.starts_with(' ') {
            current_section = Some(trimmed.trim_end_matches(':').to_string());
            continue;
        }

        match current_section.as_deref() {
            Some("packages") => {
                if trimmed.starts_with('/') && trimmed.contains(':') {
                    if let Some(pkg_info) =
                        trimmed.strip_prefix('/').and_then(|s| s.strip_suffix(':'))
                    {
                        if let Some((pkg_name, version)) = parse_pnpm_package_entry(pkg_info) {
                            deps.insert(pkg_name, version);
                        }
                    }
                }
            }
            Some("dependencies") | Some("devDependencies") | Some("optionalDependencies") => {
                if let Some(colon_pos) = trimmed.find(':') {
                    let name = trimmed[..colon_pos].trim();
                    let version_spec = trimmed[colon_pos + 1..].trim();

                    if !name.is_empty() && !version_spec.is_empty() {
                        let clean_version = clean_version_string(version_spec);
                        deps.insert(name.to_string(), clean_version);
                    }
                }
            }
            _ => {
                if trimmed.contains('@') && trimmed.contains(':') && !trimmed.starts_with('#') {
                    if let Some((potential_pkg, _)) = trimmed.split_once(':') {
                        if let Some((pkg_name, version)) = potential_pkg.trim().rsplit_once('@') {
                            if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                                deps.insert(pkg_name.to_string(), version.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    log(
        LogLevel::Info,
        &format!(
            "Enhanced lockfile parsing found {} dependencies",
            deps.len()
        ),
    );
    Ok(deps)
}

fn parse_pnpm_virtual_store_entry(dir_name: &str) -> Option<(String, String)> {
    if let Some((pkg_with_version, _hash)) = dir_name.split_once('_') {
        let pkg_with_version = pkg_with_version.replace('+', "/");

        if let Some((pkg_name, version)) = pkg_with_version.rsplit_once('@') {
            if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some((pkg_name.to_string(), version.to_string()));
            }
        }
    }

    None
}

fn resolve_pnpm_symlinks(project_root: &Path) -> Result<HashMap<String, String>, String> {
    let node_modules = project_root.join("node_modules");
    if !node_modules.exists() {
        return Ok(HashMap::new());
    }

    log(LogLevel::Info, "Resolving pnpm symlinks");
    let mut packages = HashMap::new();
    let mut visited = HashSet::new();

    scan_pnpm_symlinks_recursive(&node_modules, &mut packages, &mut visited, 0)?;

    log(
        LogLevel::Info,
        &format!("Symlink resolution found {} packages", packages.len()),
    );
    Ok(packages)
}

fn scan_pnpm_symlinks_recursive(
    dir: &Path,
    packages: &mut HashMap<String, String>,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > 30 || visited.contains(&dir.to_path_buf()) {
        return Ok(());
    }

    visited.insert(dir.to_path_buf());

    let entries = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if name.starts_with('.') {
            continue;
        }

        if name.starts_with('@') {
            if let Ok(scoped_entries) = fs::read_dir(&path) {
                for scoped_entry in scoped_entries.flatten() {
                    let scoped_path = scoped_entry.path();
                    if scoped_path.is_dir() {
                        let scoped_name = scoped_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("");
                        let full_name = format!("{name}/{scoped_name}");

                        if let Some(version) = read_package_version_safe(&scoped_path) {
                            packages.insert(full_name, version);
                        }

                        let nested = scoped_path.join("node_modules");
                        if nested.exists() {
                            scan_pnpm_symlinks_recursive(&nested, packages, visited, depth + 1)?;
                        }
                    }
                }
            }
        } else if let Some(version) = read_package_version_safe(&path) {
            packages.insert(name.to_string(), version);

            let nested = path.join("node_modules");
            if nested.exists() {
                scan_pnpm_symlinks_recursive(&nested, packages, visited, depth + 1)?;
            }
        }
    }

    Ok(())
}

fn scan_nested_node_modules(
    node_modules_path: &Path,
    depth: usize,
) -> Result<HashMap<String, String>, String> {
    if depth > 15 {
        return Ok(HashMap::new());
    }

    let mut packages = HashMap::new();

    if let Ok(entries) = fs::read_dir(node_modules_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if name.starts_with('.') {
                    continue;
                }

                if name.starts_with('@') {
                    if let Ok(scoped_entries) = fs::read_dir(&path) {
                        for scoped_entry in scoped_entries.flatten() {
                            let scoped_path = scoped_entry.path();
                            if scoped_path.is_dir() {
                                let scoped_name = scoped_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("");
                                let full_name = format!("{name}/{scoped_name}");

                                if let Some(version) = read_package_version_safe(&scoped_path) {
                                    packages.insert(full_name, version);
                                }
                            }
                        }
                    }
                } else if let Some(version) = read_package_version_safe(&path) {
                    packages.insert(name.to_string(), version);
                }
            }
        }
    }

    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_detect_license_from_content_mit() {
        let mit_content = "MIT License\n\nCopyright (c) 2024";
        assert_eq!(
            detect_license_from_content(mit_content),
            Some("MIT License".to_string())
        );
    }

    #[test]
    fn test_detect_license_from_content_apache() {
        let apache_content = "Apache License\nVersion 2.0";
        assert_eq!(
            detect_license_from_content(apache_content),
            Some("Apache License".to_string())
        );
    }

    #[test]
    fn test_detect_license_from_content_gpl() {
        let gpl_content = "GNU GPL License\n\nVersion 2";
        assert_eq!(
            detect_license_from_content(gpl_content),
            Some("GPL".to_string())
        );
    }

    #[test]
    fn test_detect_license_from_content_no_match() {
        let unknown = "Some random content";
        assert_eq!(detect_license_from_content(unknown), None);
    }

    #[test]
    fn test_get_license_from_local_license_file_mit() {
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("node_modules").join("test-pkg");
        fs::create_dir_all(&package_dir).unwrap();

        let license_path = package_dir.join("LICENSE");
        fs::write(&license_path, "MIT License\n\nCopyright (c) 2024").unwrap();

        let result = get_license_from_local_license_file(temp_dir.path(), "test-pkg");
        assert_eq!(result, Some("MIT License".to_string()));
    }

    #[test]
    fn test_get_license_from_local_license_file_scoped() {
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir
            .path()
            .join("node_modules")
            .join("@scope")
            .join("package");
        fs::create_dir_all(&package_dir).unwrap();

        let license_path = package_dir.join("LICENSE.md");
        fs::write(&license_path, "Apache License").unwrap();

        let result = get_license_from_local_license_file(temp_dir.path(), "@scope/package");
        assert_eq!(result, Some("Apache License".to_string()));
    }

    #[test]
    fn test_get_license_from_local_license_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let result = get_license_from_local_license_file(temp_dir.path(), "nonexistent");
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_license_from_local_license_file_pnpm() {
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir
            .path()
            .join("node_modules")
            .join(".pnpm")
            .join("node_modules")
            .join("test-pkg");
        fs::create_dir_all(&package_dir).unwrap();

        let license_path = package_dir.join("LICENSE.txt");
        fs::write(&license_path, "BSD License").unwrap();

        let result = get_license_from_local_license_file(temp_dir.path(), "test-pkg");
        assert_eq!(result, Some("BSD".to_string()));
    }
}
