use cargo_metadata::Package;
use rayon::prelude::*;
use std::collections::HashMap;

use crate::debug::{log, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

/// Analyze the licenses of Rust dependencies from Cargo packages
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

            let is_restrictive = is_license_restrictive(&package.license, &known_licenses);

            if is_restrictive {
                log(
                    LogLevel::Warn,
                    &format!(
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
                compatibility: LicenseCompatibility::Unknown,
                osi_status: match &package.license {
                    Some(license) => crate::licenses::get_osi_status(license),
                    None => crate::licenses::OsiStatus::Unknown,
                },
            }
        })
        .collect()
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
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("MIT".to_string()),
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
}
