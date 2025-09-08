use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

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

/// Go package information
#[derive(Debug)]
pub struct GoPackages {
    pub name: String,
    pub version: String,
}

/// Analyze the licenses of Go dependencies
pub fn analyze_go_licenses(go_mod_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
    log(
        LogLevel::Info,
        &format!("Analyzing Go dependencies from: {go_mod_path}"),
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

    let content = match fs::read_to_string(go_mod_path) {
        Ok(content) => content,
        Err(err) => {
            log_error(&format!("Failed to read go.mod file: {go_mod_path}"), &err);
            return Vec::new();
        }
    };

    let direct_dependencies = get_go_dependencies(content);
    log(
        LogLevel::Info,
        &format!("Found {} direct Go dependencies", direct_dependencies.len()),
    );
    log_debug("Direct Go dependencies", &direct_dependencies);

    // Try to resolve all dependencies using go mod graph
    let max_depth = config.dependencies.max_depth;
    log(
        LogLevel::Info,
        &format!("Using max dependency depth: {max_depth}"),
    );
    let all_deps = resolve_go_dependencies(go_mod_path, &direct_dependencies, max_depth);

    // Process all resolved dependencies
    let mut licenses = Vec::new();
    for (name, version) in all_deps {
        log(
            LogLevel::Info,
            &format!("Processing dependency: {name} ({version})"),
        );

        let license_result = fetch_license_for_go_dependency(name.as_str(), version.as_str());
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
        &format!("Found {} Go dependencies with licenses", licenses.len()),
    );
    licenses
}

/// Parse Go dependencies from go.mod content
pub fn get_go_dependencies(content_string: String) -> Vec<GoPackages> {
    log(LogLevel::Info, "Parsing Go dependencies");

    let re_comment = match Regex::new(r"(?m)^(.*?)\s*(//|#).*?$") {
        Ok(re) => re,
        Err(err) => {
            log_error("Failed to compile comment regex", &err);
            return Vec::new();
        }
    };

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
                &format!("Found Go dependency: {name} ({version})"),
            );

            dependency.push(GoPackages { name, version });
        }
    }

    log(
        LogLevel::Info,
        &format!("Parsed {} Go dependencies", dependency.len()),
    );
    dependency
}

/// Resolve all Go dependencies
fn resolve_go_dependencies(
    go_mod_path: &str,
    direct_deps: &[GoPackages],
    max_depth: u32,
) -> Vec<(String, String)> {
    log(
        LogLevel::Info,
        &format!("Resolving Go dependencies (including transitive up to depth {max_depth})"),
    );

    // go mod graph for complete dependency resolution
    if let Ok(go_deps) = resolve_with_go_mod_graph(go_mod_path, max_depth) {
        if !go_deps.is_empty() {
            log(
                LogLevel::Info,
                &format!(
                    "Resolved {} dependencies using go mod graph (depth {})",
                    go_deps.len(),
                    max_depth
                ),
            );
            return go_deps;
        }
    }

    // Direct dependencies in case go mod graph fails
    log(
        LogLevel::Info,
        "Falling back to direct dependencies only (go mod graph not available)",
    );
    direct_deps
        .iter()
        .map(|dep| (dep.name.clone(), dep.version.clone()))
        .collect()
}

/// Resolve dependencies using go mod graph with depth limit
fn resolve_with_go_mod_graph(
    go_mod_path: &str,
    max_depth: u32,
) -> Result<Vec<(String, String)>, String> {
    let project_dir = Path::new(go_mod_path)
        .parent()
        .ok_or("Cannot determine project directory")?;

    log(
        LogLevel::Info,
        &format!("Attempting to resolve dependencies with go mod graph (max depth: {max_depth})"),
    );

    // Run go mod graph to get dependency graph
    let output = Command::new("go")
        .args(["mod", "graph"])
        .current_dir(project_dir)
        .output()
        .map_err(|e| format!("Failed to run go mod graph: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("go mod graph failed: {stderr}"));
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let deps = parse_go_mod_graph_output(&stdout_str, max_depth);

    log(
        LogLevel::Info,
        &format!(
            "Resolved {} dependencies from go mod graph output",
            deps.len()
        ),
    );

    Ok(deps)
}

/// Parse go mod graph output to extract dependencies with depth awareness
fn parse_go_mod_graph_output(output: &str, max_depth: u32) -> Vec<(String, String)> {
    let mut all_deps = HashMap::new();
    let mut depth_map = HashMap::new();
    let mut edges = HashMap::new();

    log(
        LogLevel::Info,
        &format!("Parsing go mod graph with max depth {max_depth}"),
    );

    // Collect all edges and modules
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((from, to)) = line.split_once(' ') {
            let from = from.trim();
            let to = to.trim();

            // Parse module name and version
            if let Some((from_name, from_version)) = parse_go_module_version(from) {
                all_deps.insert(from_name.clone(), from_version);
            }

            if let Some((to_name, to_version)) = parse_go_module_version(to) {
                all_deps.insert(to_name.clone(), to_version);

                // Track edges for depth calculation
                edges
                    .entry(from.to_string())
                    .or_insert_with(Vec::new)
                    .push(to.to_string());
            }
        }
    }

    // Find root modules
    let mut roots = HashSet::new();
    let mut destinations = HashSet::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((from, to)) = line.split_once(' ') {
            let from = from.trim();
            let to = to.trim();
            roots.insert(from.to_string());
            destinations.insert(to.to_string());
        }
    }

    let root_modules: Vec<_> = roots.difference(&destinations).collect();

    // Calculate depths using BFS from root modules
    let mut queue = Vec::new();
    let mut visited = HashSet::new();

    for root in root_modules {
        queue.push((root.clone(), 0u32));
        depth_map.insert(root.clone(), 0);
    }

    let mut depth_stats = HashMap::new();

    while let Some((current, depth)) = queue.pop() {
        if visited.contains(&current) || depth >= max_depth {
            if depth >= max_depth {
                log(
                    LogLevel::Info,
                    &format!("Skipping {current} - exceeded max depth {max_depth}"),
                );
            }
            continue;
        }

        visited.insert(current.clone());
        depth_map.insert(current.clone(), depth);

        // Track depth statistics
        *depth_stats.entry(depth).or_insert(0) += 1;

        // Add children to queue
        if let Some(children) = edges.get(&current) {
            for child in children {
                if !visited.contains(child) && depth + 1 < max_depth {
                    queue.push((child.clone(), depth + 1));
                }
            }
        }
    }

    // Filter dependencies based on depth limit
    let filtered_deps: Vec<(String, String)> = all_deps
        .into_iter()
        .filter(|(name, _version)| {
            // Find the full module name in depth_map
            for (module_full, depth) in &depth_map {
                if let Some((module_name, _)) = parse_go_module_version(module_full) {
                    if module_name == *name && *depth < max_depth {
                        return true;
                    }
                }
            }
            false
        })
        .collect();

    // Log depth statistics
    for depth in 0..max_depth {
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
            "Go mod graph resolution completed. Total dependencies: {} (explored up to depth {})",
            filtered_deps.len(),
            max_depth
        ),
    );

    filtered_deps
}

/// Parse Go module string to extract name and version
fn parse_go_module_version(module_str: &str) -> Option<(String, String)> {
    // Handle formats like: github.com/user/repo@v1.2.3 or github.com/user/repo@v1.2.3-0.20210101000000-abcdef123456
    if let Some(at_pos) = module_str.rfind('@') {
        let name = module_str[..at_pos].to_string();
        let version = module_str[at_pos + 1..].to_string();
        Some((name, version))
    } else {
        // Handle cases without version
        Some((module_str.to_string(), "unknown".to_string()))
    }
}

/// Fetch the license for a Go dependency from the Go Package Index (pkg.go.dev)
pub fn fetch_license_for_go_dependency(
    name: impl Into<String>,
    _version: impl Into<String>,
) -> String {
    let name = name.into();
    let _version = _version.into();

    let api_url = format!("https://pkg.go.dev/{name}?tab=licenses");
    log(
        LogLevel::Info,
        &format!("Fetching license from Go Package Index: {api_url}"),
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
                    &format!("Go Package Index API response status: {status}"),
                );

                if status.as_u16() == 429 {
                    log(
                        LogLevel::Warn,
                        &format!(
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
                                    &format!("License found for {name}: {license}"),
                                );
                                return license;
                            } else {
                                log(
                                    LogLevel::Warn,
                                    &format!("No license found in HTML for {name}"),
                                );
                            }
                        }
                        Err(err) => {
                            log_error(&format!("Failed to extract HTML content for {name}"), &err);
                        }
                    }
                } else {
                    log(
                        LogLevel::Error,
                        &format!("Unexpected HTTP status: {status} for {name}"),
                    );
                }

                break;
            }
            Err(err) => {
                log_error(&format!("Failed to fetch metadata for {name}"), &err);
                break;
            }
        }
    }

    log(
        LogLevel::Warn,
        &format!("Unable to determine license for {name} after {attempts} attempts"),
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
                &format!("License found in HTML: {license_text}"),
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_get_go_dependencies_empty_content() {
        let content = "".to_string();
        let deps = get_go_dependencies(content);
        assert!(deps.is_empty());
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

    #[test]
    fn test_fetch_license_for_go_dependency_error_handling() {
        // Test with invalid package name
        let result = fetch_license_for_go_dependency("invalid/package/name", "v1.0.0");
        assert_eq!(result, "Unknown");
    }

    #[test]
    fn test_go_packages_debug() {
        let go_package = GoPackages {
            name: "github.com/test/package".to_string(),
            version: "v1.0.0".to_string(),
        };

        let debug_str = format!("{go_package:?}");
        assert!(debug_str.contains("github.com/test/package"));
        assert!(debug_str.contains("v1.0.0"));
    }

    #[test]
    fn test_parse_go_module_version() {
        // Test with version
        assert_eq!(
            parse_go_module_version("github.com/user/repo@v1.2.3"),
            Some(("github.com/user/repo".to_string(), "v1.2.3".to_string()))
        );

        // Test with complex version
        assert_eq!(
            parse_go_module_version("github.com/user/repo@v1.2.3-0.20210101000000-abcdef123456"),
            Some((
                "github.com/user/repo".to_string(),
                "v1.2.3-0.20210101000000-abcdef123456".to_string()
            ))
        );

        // Test without version
        assert_eq!(
            parse_go_module_version("github.com/user/repo"),
            Some(("github.com/user/repo".to_string(), "unknown".to_string()))
        );
    }

    #[test]
    fn test_parse_go_mod_graph_output() {
        let graph_output = r#"
github.com/myproject@v0.0.0 github.com/gin-gonic/gin@v1.9.1
github.com/myproject@v0.0.0 github.com/golang/protobuf@v1.5.3
github.com/gin-gonic/gin@v1.9.1 github.com/bytedance/sonic@v1.9.1
github.com/gin-gonic/gin@v1.9.1 github.com/chenzhuoyu/base64x@v0.0.0-20221115062448-fe3a3abad311
github.com/bytedance/sonic@v1.9.1 github.com/klauspost/cpuid/v2@v2.0.9
"#;

        let deps = parse_go_mod_graph_output(graph_output, 5);

        // Should include dependencies up to the specified depth
        assert!(!deps.is_empty());

        // Should include root dependencies
        let dep_names: Vec<String> = deps.iter().map(|(name, _)| name.clone()).collect();
        assert!(dep_names.contains(&"github.com/gin-gonic/gin".to_string()));
        assert!(dep_names.contains(&"github.com/golang/protobuf".to_string()));
    }

    #[test]
    fn test_parse_go_mod_graph_output_with_depth_limit() {
        let graph_output = r#"github.com/myproject@v0.0.0 github.com/level1@v1.0.0
github.com/level1@v1.0.0 github.com/level2@v1.0.0
github.com/level2@v1.0.0 github.com/level3@v1.0.0"#;

        // With depth limit 3, should include level1 and level2 but not level3
        let deps = parse_go_mod_graph_output(graph_output, 3);
        let dep_names: Vec<String> = deps.iter().map(|(name, _)| name.clone()).collect();

        // level1 is at depth 1, level2 is at depth 2 - both should be included with max_depth 3
        assert!(dep_names.contains(&"github.com/level1".to_string()));
        assert!(dep_names.contains(&"github.com/level2".to_string()));
        // level3 is at depth 3, should not be included with max_depth 3 (since we check depth >= max_depth)
        assert!(!dep_names.contains(&"github.com/level3".to_string()));
    }

    #[test]
    fn test_resolve_go_dependencies_fallback() {
        let direct_deps = vec![
            GoPackages {
                name: "github.com/test/pkg1".to_string(),
                version: "v1.0.0".to_string(),
            },
            GoPackages {
                name: "github.com/test/pkg2".to_string(),
                version: "v2.0.0".to_string(),
            },
        ];

        // This should fall back to direct dependencies when go mod graph fails
        let result = resolve_go_dependencies("/nonexistent/go.mod", &direct_deps, 5);

        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            ("github.com/test/pkg1".to_string(), "v1.0.0".to_string())
        );
        assert_eq!(
            result[1],
            ("github.com/test/pkg2".to_string(), "v2.0.0".to_string())
        );
    }
}
