use cargo_metadata::{Metadata, Package, PackageId};
use rayon::prelude::*;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::debug::{log, log_debug, log_error, LogLevel};
use crate::licenses::{
    detect_license_from_content, detect_license_in_dir, fetch_licenses_from_github,
    is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

/// Analyze the licenses of Rust dependencies from Cargo packages
#[allow(dead_code)]
pub fn analyze_rust_licenses(packages: Vec<Package>) -> Vec<LicenseInfo> {
    let config = crate::config::load_config().unwrap_or_default();
    analyze_rust_licenses_with_config(packages, &config, false)
}

/// Analyze Rust deps with full Metadata so workspace members can be attributed.
///
/// In a multi-member Cargo workspace, every dependency is tagged with the workspace
/// member(s) that pull it in, and workspace members themselves are excluded from the
/// dep report. Single-crate projects fall through to the existing behavior.
pub fn analyze_rust_licenses_with_metadata(
    metadata: Metadata,
    config: &crate::config::FeludaConfig,
    no_local: bool,
) -> Vec<LicenseInfo> {
    let workspace_members: HashSet<PackageId> =
        metadata.workspace_members.iter().cloned().collect();
    let is_workspace = workspace_members.len() > 1;

    log(
        LogLevel::Info,
        &format!(
            "Cargo metadata: {} workspace members, {} total packages",
            workspace_members.len(),
            metadata.packages.len()
        ),
    );

    if !is_workspace {
        log(
            LogLevel::Info,
            "Single-crate project; no workspace attribution",
        );
        return analyze_rust_licenses_with_config(metadata.packages, config, no_local);
    }

    let attribution = build_workspace_attribution(&metadata, &workspace_members);
    log_debug("Workspace attribution map", &attribution);

    let dep_packages: Vec<Package> = metadata
        .packages
        .into_iter()
        .filter(|p| !workspace_members.contains(&p.id))
        .collect();

    log(
        LogLevel::Info,
        &format!(
            "Analyzing {} non-workspace deps across workspace",
            dep_packages.len()
        ),
    );

    let mut infos = analyze_rust_licenses_with_config(dep_packages, config, no_local);
    for info in &mut infos {
        if let Some(member_names) = attribution.get(&(info.name.clone(), info.version.clone())) {
            if !member_names.is_empty() {
                info.sub_project =
                    Some(member_names.iter().cloned().collect::<Vec<_>>().join(", "));
            }
        }
    }
    infos
}

/// Build a map from (dep name, version) -> set of workspace member names that depend on it.
fn build_workspace_attribution(
    metadata: &Metadata,
    workspace_members: &HashSet<PackageId>,
) -> HashMap<(String, String), BTreeSet<String>> {
    let mut attribution: HashMap<(String, String), BTreeSet<String>> = HashMap::new();

    let resolve = match &metadata.resolve {
        Some(r) => r,
        None => {
            log(LogLevel::Warn, "No resolve graph in cargo metadata");
            return attribution;
        }
    };

    let nodes_by_id: HashMap<&PackageId, &cargo_metadata::Node> =
        resolve.nodes.iter().map(|n| (&n.id, n)).collect();
    let pkg_by_id: HashMap<&PackageId, &Package> =
        metadata.packages.iter().map(|p| (&p.id, p)).collect();

    for member_id in workspace_members {
        let member_name = match pkg_by_id.get(member_id) {
            Some(p) => p.name.to_string(),
            None => continue,
        };

        let mut visited: HashSet<&PackageId> = HashSet::new();
        let mut queue: VecDeque<&PackageId> = VecDeque::new();
        queue.push_back(member_id);
        visited.insert(member_id);

        while let Some(id) = queue.pop_front() {
            let node = match nodes_by_id.get(id) {
                Some(n) => *n,
                None => continue,
            };
            for dep_id in &node.dependencies {
                if !visited.insert(dep_id) {
                    continue;
                }
                queue.push_back(dep_id);
                if workspace_members.contains(dep_id) {
                    continue;
                }
                if let Some(pkg) = pkg_by_id.get(dep_id) {
                    attribution
                        .entry((pkg.name.to_string(), pkg.version.to_string()))
                        .or_default()
                        .insert(member_name.clone());
                }
            }
        }
    }

    attribution
}

pub fn analyze_rust_licenses_with_config(
    packages: Vec<Package>,
    config: &crate::config::FeludaConfig,
    no_local: bool,
) -> Vec<LicenseInfo> {
    if packages.is_empty() {
        log(
            LogLevel::Warn,
            "No Rust packages found for license analysis",
        );
        return vec![];
    }

    log(
        LogLevel::Info,
        &format!("Analyzing licenses for {} Rust packages", packages.len()),
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

    packages
        .par_iter()
        .map(|package| {
            log(
                LogLevel::Info,
                &format!("Analyzing package: {} ({})", package.name, package.version),
            );

            let license = package.license.clone().or_else(|| {
                if no_local {
                    None
                } else {
                    get_license_from_manifest(&package.manifest_path)
                }
            });

            let is_restrictive = is_license_restrictive(&license, &known_licenses, config.strict);

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    &format!(
                        "Restrictive license found: {:?} for {}",
                        license, package.name
                    ),
                );
            }

            LicenseInfo {
                name: package.name.to_string(),
                version: package.version.to_string(),
                license,
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: match &package.license {
                    Some(license) => crate::licenses::get_osi_status(license),
                    None => crate::licenses::OsiStatus::Unknown,
                },
                sub_project: None,
            }
        })
        .collect()
}

fn get_license_from_manifest<P: AsRef<std::path::Path>>(manifest_path: P) -> Option<String> {
    use std::fs;
    use toml::Value;

    let manifest_path = manifest_path.as_ref();

    log(
        crate::debug::LogLevel::Info,
        &format!("Checking manifest for license: {}", manifest_path.display()),
    );

    if !manifest_path.exists() {
        return None;
    }

    let content = fs::read_to_string(manifest_path).ok()?;
    let manifest = toml::from_str::<Value>(&content).ok()?;
    let package = manifest.get("package");

    // 1. Explicit SPDX expression in the `license` field.
    if let Some(license) = package
        .and_then(|pkg| pkg.get("license"))
        .and_then(|license| license.as_str())
    {
        log(
            crate::debug::LogLevel::Info,
            &format!("Found license in manifest: {license}"),
        );
        return Some(license.to_string());
    }

    let crate_dir = manifest_path.parent();

    // 2. `license-file` field: a relative path to a bundled license text, which may use a
    //    non-standard filename (e.g. `LICENSE-MIT`), so read it directly and content-detect.
    if let (Some(dir), Some(rel)) = (
        crate_dir,
        package
            .and_then(|pkg| pkg.get("license-file"))
            .and_then(|license_file| license_file.as_str()),
    ) {
        if let Ok(text) = fs::read_to_string(dir.join(rel)) {
            if let Some(spdx) = detect_license_from_content(&text) {
                log(
                    crate::debug::LogLevel::Info,
                    &format!("Detected {spdx} license from license-file: {rel}"),
                );
                return Some(spdx);
            }
        }
    }

    // 3. Probe conventional license files (LICENSE, COPYING, …) in the crate root.
    if let Some(spdx) = crate_dir.and_then(detect_license_in_dir) {
        log(
            crate::debug::LogLevel::Info,
            &format!("Detected {spdx} license from crate license file"),
        );
        return Some(spdx);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_analyze_rust_licenses_empty() {
        let packages = vec![];
        let result = analyze_rust_licenses(packages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_license_restrictive_with_default_config() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("GPL-3.0".to_string()),
                &known_licenses,
                false
            ));
            assert!(!is_license_restrictive(
                &Some("MIT".to_string()),
                &known_licenses,
                false
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
                &known_licenses,
                false
            ));
        });
    }

    #[test]
    fn test_get_license_from_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("Cargo.toml");

        let manifest_content = r#"[package]
name = "test-crate"
version = "0.1.0"
license = "MIT"
"#;

        std::fs::write(&manifest_path, manifest_content).unwrap();

        let result = get_license_from_manifest(&manifest_path);
        assert_eq!(result, Some("MIT".to_string()));
    }

    #[test]
    fn test_get_license_from_manifest_apache() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("Cargo.toml");

        let manifest_content = r#"[package]
name = "test-crate"
version = "0.1.0"
license = "Apache-2.0"
"#;

        std::fs::write(&manifest_path, manifest_content).unwrap();

        let result = get_license_from_manifest(&manifest_path);
        assert_eq!(result, Some("Apache-2.0".to_string()));
    }

    #[test]
    fn test_get_license_from_manifest_missing() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("Cargo.toml");

        let manifest_content = r#"[package]
name = "test-crate"
version = "0.1.0"
"#;

        std::fs::write(&manifest_path, manifest_content).unwrap();

        let result = get_license_from_manifest(&manifest_path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_license_from_manifest_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("nonexistent.toml");

        let result = get_license_from_manifest(&manifest_path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_license_from_manifest_license_file_field() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("Cargo.toml");

        // A crate with no `license` field, only a `license-file` pointing at a
        // non-standard filename — previously this resolved to "No License".
        let manifest_content = r#"[package]
name = "test-crate"
version = "0.1.0"
license-file = "LICENSE-MIT"
"#;
        std::fs::write(&manifest_path, manifest_content).unwrap();
        std::fs::write(
            temp_dir.path().join("LICENSE-MIT"),
            "MIT License\n\nPermission is hereby granted, free of charge, to any person",
        )
        .unwrap();

        let result = get_license_from_manifest(&manifest_path);
        assert_eq!(result, Some("MIT".to_string()));
    }

    #[test]
    fn test_get_license_from_manifest_crate_dir_fallback() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("Cargo.toml");

        // No `license` and no `license-file` field, but a conventional LICENSE file
        // ships in the crate root.
        let manifest_content = r#"[package]
name = "test-crate"
version = "0.1.0"
"#;
        std::fs::write(&manifest_path, manifest_content).unwrap();
        std::fs::write(
            temp_dir.path().join("LICENSE"),
            "Apache License\nVersion 2.0, January 2004",
        )
        .unwrap();

        let result = get_license_from_manifest(&manifest_path);
        assert_eq!(result, Some("Apache-2.0".to_string()));
    }

    #[test]
    fn test_get_license_from_manifest_license_field_wins_over_file() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("Cargo.toml");

        // When both are present the explicit SPDX expression takes precedence.
        let manifest_content = r#"[package]
name = "test-crate"
version = "0.1.0"
license = "MIT"
license-file = "LICENSE"
"#;
        std::fs::write(&manifest_path, manifest_content).unwrap();
        std::fs::write(
            temp_dir.path().join("LICENSE"),
            "Apache License\nVersion 2.0, January 2004",
        )
        .unwrap();

        let result = get_license_from_manifest(&manifest_path);
        assert_eq!(result, Some("MIT".to_string()));
    }
}
