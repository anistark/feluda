use cargo_metadata::Package;
use rayon::prelude::*;
use std::collections::HashMap;

use crate::debug::{log, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

/// Analyze the licenses of Rust dependencies from Cargo packages
#[allow(dead_code)]
pub fn analyze_rust_licenses(packages: Vec<Package>) -> Vec<LicenseInfo> {
    let config = crate::config::load_config().unwrap_or_default();
    analyze_rust_licenses_with_config(packages, &config, false)
}

pub fn analyze_rust_licenses_with_no_local(
    packages: Vec<Package>,
    no_local: bool,
) -> Vec<LicenseInfo> {
    let config = crate::config::load_config().unwrap_or_default();
    analyze_rust_licenses_with_config(packages, &config, no_local)
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

    match fs::read_to_string(manifest_path) {
        Ok(content) => match toml::from_str::<Value>(&content) {
            Ok(manifest) => manifest
                .get("package")
                .and_then(|pkg| pkg.get("license"))
                .and_then(|license| license.as_str())
                .map(|s| {
                    log(
                        crate::debug::LogLevel::Info,
                        &format!("Found license in manifest: {s}"),
                    );
                    s.to_string()
                }),
            Err(_) => None,
        },
        Err(_) => None,
    }
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
}
