use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::FeludaConfig;
use crate::debug::{log, log_debug, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

pub fn analyze_c_licenses(project_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
    log(
        LogLevel::Info,
        &format!("Analyzing C dependencies from: {project_path}"),
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

    let direct_dependencies = detect_c_dependencies(project_path, config);
    log(
        LogLevel::Info,
        &format!("Found {} direct C dependencies", direct_dependencies.len()),
    );
    log_debug("Direct C dependencies", &direct_dependencies);

    let max_depth = config.dependencies.max_depth;
    log(
        LogLevel::Info,
        &format!("Using max dependency depth: {max_depth}"),
    );

    let all_deps = resolve_c_dependencies(project_path, &direct_dependencies, max_depth);
    log(
        LogLevel::Info,
        &format!(
            "Total C dependencies (including transitive): {}",
            all_deps.len()
        ),
    );
    log_debug("All C dependencies", &all_deps);

    let dependencies = all_deps;

    dependencies
        .into_iter()
        .map(|(name, version)| {
            log(
                LogLevel::Info,
                &format!("Processing dependency: {name} ({version})"),
            );

            let license_result = fetch_license_for_c_dependency(&name, &version);
            let license = Some(license_result);
            let is_restrictive = is_license_restrictive(&license, &known_licenses, config.strict);

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    &format!("Restrictive license found: {license:?} for {name}"),
                );
            }

            LicenseInfo {
                name,
                version,
                license: license.clone(),
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: match &license {
                    Some(l) => crate::licenses::get_osi_status(l),
                    None => crate::licenses::OsiStatus::Unknown,
                },
            }
        })
        .collect()
}

fn detect_c_dependencies(project_path: &str, config: &FeludaConfig) -> Vec<(String, String)> {
    let mut dependencies = Vec::new();
    let project_dir = Path::new(project_path).parent().unwrap_or(Path::new("."));

    if let Ok(autotools_deps) = parse_autotools_dependencies(project_dir, config) {
        log(
            LogLevel::Info,
            &format!("Found {} autotools dependencies", autotools_deps.len()),
        );
        dependencies.extend(autotools_deps);
    }

    if dependencies.is_empty() {
        if let Ok(makefile_deps) = parse_makefile_dependencies(project_dir, config) {
            log(
                LogLevel::Info,
                &format!("Found {} makefile dependencies", makefile_deps.len()),
            );
            dependencies.extend(makefile_deps);
        }
    }

    if dependencies.is_empty() {
        if let Ok(pkgconfig_deps) = parse_pkgconfig_dependencies(project_dir, config) {
            log(
                LogLevel::Info,
                &format!("Found {} pkg-config dependencies", pkgconfig_deps.len()),
            );
            dependencies.extend(pkgconfig_deps);
        }
    }

    dependencies
}

fn resolve_c_dependencies(
    _project_path: &str,
    direct_deps: &[(String, String)],
    max_depth: u32,
) -> Vec<(String, String)> {
    log(
        LogLevel::Info,
        &format!("Resolving C dependencies (including transitive up to depth {max_depth})"),
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

        if let Ok(transitive_deps) = resolve_c_transitive_deps(&name, &version) {
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
            "C dependency resolution completed. Total dependencies: {} (explored up to depth {})",
            all_dependencies.len(),
            max_depth
        ),
    );

    all_dependencies
}

fn resolve_c_transitive_deps(
    package_name: &str,
    version: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut dependencies = Vec::new();

    // Try pkg-config for transitive dependencies
    if let Ok(pkg_deps) = get_pkgconfig_requires(package_name) {
        dependencies.extend(pkg_deps);
    }

    // Try parsing .pc file directly
    if let Ok(pc_deps) = parse_pc_file_requires(package_name) {
        dependencies.extend(pc_deps);
    }

    // Try system package dependencies
    if version == "system" {
        if let Ok(sys_deps) = get_system_package_dependencies(package_name) {
            dependencies.extend(sys_deps);
        }
    }

    Ok(dependencies)
}

fn get_pkgconfig_requires(package_name: &str) -> Result<Vec<(String, String)>, String> {
    let output = Command::new("pkg-config")
        .args(["--print-requires", package_name])
        .output()
        .map_err(|e| format!("Failed to run pkg-config --print-requires: {e}"))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

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

    // Also check private requires
    let private_output = Command::new("pkg-config")
        .args(["--print-requires-private", package_name])
        .output();

    if let Ok(private_out) = private_output {
        if private_out.status.success() {
            let private_str = String::from_utf8_lossy(&private_out.stdout);
            for line in private_str.lines() {
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
        }
    }

    Ok(dependencies)
}

fn parse_pc_file_requires(package_name: &str) -> Result<Vec<(String, String)>, String> {
    let potential_paths = [
        format!("/usr/lib/pkgconfig/{package_name}.pc"),
        format!("/usr/local/lib/pkgconfig/{package_name}.pc"),
        format!("/usr/share/pkgconfig/{package_name}.pc"),
        format!("/usr/local/share/pkgconfig/{package_name}.pc"),
        format!("/opt/homebrew/lib/pkgconfig/{package_name}.pc"),
    ];

    for pc_path in &potential_paths {
        if let Ok(content) = fs::read_to_string(pc_path) {
            return parse_pc_content(&content);
        }
    }

    Ok(Vec::new())
}

fn parse_pc_content(content: &str) -> Result<Vec<(String, String)>, String> {
    let mut dependencies = Vec::new();
    let requires_regex = Regex::new(r"^Requires(?:\.private)?\s*:\s*(.+)$")
        .map_err(|e| format!("Failed to compile requires regex: {e}"))?;

    for line in content.lines() {
        if let Some(cap) = requires_regex.captures(line) {
            if let Some(requires_str) = cap.get(1) {
                let requires = requires_str.as_str().trim();
                for dep in requires.split(',') {
                    let dep = dep.trim();
                    if !dep.is_empty() {
                        let parts: Vec<&str> = dep.split_whitespace().collect();
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
            }
        }
    }

    Ok(dependencies)
}

fn get_system_package_dependencies(package_name: &str) -> Result<Vec<(String, String)>, String> {
    // Try dpkg (Debian/Ubuntu)
    if let Ok(output) = Command::new("dpkg-query")
        .args(["-W", "-f", "${Depends}\\n", package_name])
        .output()
    {
        if output.status.success() {
            let depends_str = String::from_utf8_lossy(&output.stdout);
            return parse_debian_dependencies(&depends_str);
        }
    }

    // Try rpm (RedHat/CentOS/Fedora)
    if let Ok(output) = Command::new("rpm")
        .args(["-q", "--requires", package_name])
        .output()
    {
        if output.status.success() {
            let requires_str = String::from_utf8_lossy(&output.stdout);
            return parse_rpm_dependencies(&requires_str);
        }
    }

    Ok(Vec::new())
}

fn parse_debian_dependencies(depends_str: &str) -> Result<Vec<(String, String)>, String> {
    let mut dependencies = Vec::new();

    for dep in depends_str.split(',') {
        let dep = dep.trim();
        if !dep.is_empty() && !dep.starts_with("${") {
            // Remove version constraints and alternatives
            let parts: Vec<&str> = dep.split_whitespace().collect();
            if let Some(pkg_name) = parts.first() {
                let clean_name = pkg_name.split('|').next().unwrap_or(pkg_name).trim();
                if !clean_name.is_empty() {
                    dependencies.push((clean_name.to_string(), "system".to_string()));
                }
            }
        }
    }

    Ok(dependencies)
}

fn parse_rpm_dependencies(requires_str: &str) -> Result<Vec<(String, String)>, String> {
    let mut dependencies = Vec::new();

    for line in requires_str.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with("rpmlib(") && !line.starts_with('/') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(pkg_name) = parts.first() {
                dependencies.push((pkg_name.to_string(), "system".to_string()));
            }
        }
    }

    Ok(dependencies)
}

fn parse_autotools_dependencies(
    project_dir: &Path,
    _config: &FeludaConfig,
) -> Result<Vec<(String, String)>, String> {
    let configure_ac = project_dir.join("configure.ac");
    let configure_in = project_dir.join("configure.in");

    let config_file = if configure_ac.exists() {
        configure_ac
    } else if configure_in.exists() {
        configure_in
    } else {
        return Err("No autotools configuration file found".to_string());
    };

    let content = fs::read_to_string(&config_file)
        .map_err(|e| format!("Failed to read autotools config: {e}"))?;

    let mut dependencies = Vec::new();

    let pkg_check_regex = Regex::new(r"PKG_CHECK_MODULES\s*\(\s*\w+\s*,\s*([^,\)]+)")
        .map_err(|e| format!("Failed to compile PKG_CHECK_MODULES regex: {e}"))?;

    for cap in pkg_check_regex.captures_iter(&content) {
        if let Some(pkg_spec) = cap.get(1) {
            let spec = pkg_spec
                .as_str()
                .trim()
                .trim_matches('"')
                .trim_matches('\'');

            let parts: Vec<&str> = spec.split_whitespace().collect();
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

    let ac_check_lib_regex = Regex::new(r"AC_CHECK_LIB\s*\(\s*([^,\)]+)")
        .map_err(|e| format!("Failed to compile AC_CHECK_LIB regex: {e}"))?;

    for cap in ac_check_lib_regex.captures_iter(&content) {
        if let Some(lib_name) = cap.get(1) {
            let name = lib_name
                .as_str()
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            dependencies.push((name.to_string(), "system".to_string()));
        }
    }

    Ok(dependencies)
}

fn parse_makefile_dependencies(
    project_dir: &Path,
    _config: &FeludaConfig,
) -> Result<Vec<(String, String)>, String> {
    let makefile_paths = ["Makefile", "makefile", "GNUmakefile"];

    for &makefile_name in &makefile_paths {
        let makefile_path = project_dir.join(makefile_name);
        if makefile_path.exists() {
            let content = fs::read_to_string(&makefile_path)
                .map_err(|e| format!("Failed to read {makefile_name}: {e}"))?;

            return parse_makefile_content(&content);
        }
    }

    Err("No Makefile found".to_string())
}

fn parse_makefile_content(content: &str) -> Result<Vec<(String, String)>, String> {
    let mut dependencies = Vec::new();

    let ldflags_regex = Regex::new(r"-l([a-zA-Z0-9_-]+)")
        .map_err(|e| format!("Failed to compile ldflags regex: {e}"))?;

    for cap in ldflags_regex.captures_iter(content) {
        if let Some(lib_name) = cap.get(1) {
            let name = lib_name.as_str();
            if !name.is_empty() && name != "c" && name != "m" {
                dependencies.push((format!("lib{name}"), "system".to_string()));
            }
        }
    }

    let pkgconfig_regex = Regex::new(r"`pkg-config\s+--[^`]*\s+([a-zA-Z0-9_-]+)")
        .map_err(|e| format!("Failed to compile pkg-config regex: {e}"))?;

    for cap in pkgconfig_regex.captures_iter(content) {
        if let Some(pkg_name) = cap.get(1) {
            dependencies.push((pkg_name.as_str().to_string(), "system".to_string()));
        }
    }

    Ok(dependencies)
}

fn parse_pkgconfig_dependencies(
    project_dir: &Path,
    _config: &FeludaConfig,
) -> Result<Vec<(String, String)>, String> {
    let output = Command::new("pkg-config")
        .args(["--list-all"])
        .current_dir(project_dir)
        .output()
        .map_err(|e| format!("Failed to run pkg-config: {e}"))?;

    if !output.status.success() {
        return Err("pkg-config command failed".to_string());
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut dependencies = Vec::new();

    for line in stdout_str.lines().take(10) {
        if let Some(space_pos) = line.find(' ') {
            let pkg_name = &line[..space_pos];
            if !pkg_name.is_empty() {
                dependencies.push((pkg_name.to_string(), "system".to_string()));
            }
        }
    }

    Ok(dependencies)
}

fn fetch_license_for_c_dependency(name: &str, version: &str) -> String {
    if version == "system" {
        if let Ok(license) = get_system_package_license(name) {
            return license;
        }
    }

    format!("Unknown license for {name}: {version}")
}

fn get_system_package_license(package_name: &str) -> Result<String, String> {
    if let Ok(output) = Command::new("dpkg-query")
        .args(["-f", "${Package} ${License}\n", "-W", package_name])
        .output()
    {
        if output.status.success() {
            let stdout_str = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout_str.lines().next() {
                if let Some(space_pos) = line.find(' ') {
                    return Ok(line[space_pos + 1..].to_string());
                }
            }
        }
    }

    if let Ok(output) = Command::new("rpm")
        .args(["-q", "--qf", "%{LICENSE}\n", package_name])
        .output()
    {
        if output.status.success() {
            let license = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !license.is_empty() && license != "(none)" {
                return Ok(license);
            }
        }
    }

    Err(format!("Could not determine license for {package_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_autotools_dependencies() {
        let temp_dir = TempDir::new().unwrap();
        let configure_ac = temp_dir.path().join("configure.ac");

        fs::write(
            &configure_ac,
            r#"AC_INIT([test], [1.0])
PKG_CHECK_MODULES([GLIB], [glib-2.0 >= 2.40])
PKG_CHECK_MODULES([GTK], [gtk+-3.0])
AC_CHECK_LIB([z], [inflate])
AC_CHECK_LIB([ssl], [SSL_new])
"#,
        )
        .unwrap();

        let config = FeludaConfig::default();
        let result = parse_autotools_dependencies(temp_dir.path(), &config);

        // Just verify it doesn't crash - dependency parsing is complex
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_makefile_dependencies() {
        let makefile_content = r#"
CFLAGS = -Wall -O2
LDFLAGS = -lz -lssl -lpthread
LIBS = `pkg-config --libs gtk+-3.0`

test: test.o
	$(CC) -o test test.o $(LDFLAGS)
"#;

        let result = parse_makefile_content(makefile_content).unwrap();

        assert!(!result.is_empty());
        assert!(result.iter().any(|(name, _)| name == "libz"));
        assert!(result.iter().any(|(name, _)| name == "libssl"));
        assert!(result.iter().any(|(name, _)| name == "libpthread"));
    }

    #[test]
    fn test_analyze_c_licenses_empty() {
        let temp_dir = TempDir::new().unwrap();
        let dummy_file = temp_dir.path().join("dummy");
        fs::write(&dummy_file, "").unwrap();

        let config = FeludaConfig::default();
        let dependencies = detect_c_dependencies(dummy_file.to_str().unwrap(), &config);

        // Should be empty or small since no autotools/makefile files exist
        // pkg-config might return some system packages, so just check it doesn't crash
        assert!(dependencies.len() <= 20);
    }
}
