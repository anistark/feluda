use serde_json::Value;
use std::collections::HashMap;
use std::fs;

use crate::config::FeludaConfig;
use crate::debug::{log, log_debug, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, License, LicenseCompatibility, LicenseInfo,
};

pub fn analyze_r_licenses(package_file_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
    let mut licenses = Vec::new();
    log(
        LogLevel::Info,
        &format!("Analyzing R dependencies from: {package_file_path}"),
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

    if package_file_path.ends_with("renv.lock") {
        licenses.extend(parse_renv_lock(package_file_path, &known_licenses, config));
    } else if package_file_path.ends_with("DESCRIPTION") {
        let max_depth = config.dependencies.max_depth;
        licenses.extend(parse_description_file(
            package_file_path,
            max_depth,
            &known_licenses,
            config,
        ));
    } else {
        log(
            LogLevel::Warn,
            &format!("Unsupported R dependency file: {package_file_path}"),
        );
    }

    log(
        LogLevel::Info,
        &format!("Found {} R dependencies with licenses", licenses.len()),
    );
    licenses
}

fn parse_renv_lock(
    lock_file_path: &str,
    known_licenses: &HashMap<String, License>,
    config: &FeludaConfig,
) -> Vec<LicenseInfo> {
    let mut licenses = Vec::new();

    match fs::read_to_string(lock_file_path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(json) => {
                if let Some(packages) = json["Packages"].as_object() {
                    log(
                        LogLevel::Info,
                        &format!("Found {} packages in renv.lock", packages.len()),
                    );
                    log_debug("Packages", packages);

                    for (name, pkg_info) in packages {
                        let version = pkg_info["Version"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_string();

                        log(
                            LogLevel::Info,
                            &format!("Processing R package: {name} ({version})"),
                        );

                        let license_result = fetch_license_for_r_dependency(name, &version);
                        let license = Some(license_result);
                        let is_restrictive =
                            is_license_restrictive(&license, known_licenses, config.strict);

                        if is_restrictive {
                            log(
                                LogLevel::Warn,
                                &format!("Restrictive license found: {license:?} for {name}"),
                            );
                        }

                        licenses.push(LicenseInfo {
                            name: name.clone(),
                            version,
                            license: license.clone(),
                            is_restrictive,
                            compatibility: LicenseCompatibility::Unknown,
                            osi_status: match &license {
                                Some(l) => crate::licenses::get_osi_status(l),
                                None => crate::licenses::OsiStatus::Unknown,
                            },
                        });
                    }
                } else {
                    log(LogLevel::Warn, "No 'Packages' section found in renv.lock");
                }
            }
            Err(err) => {
                log_error("Failed to parse renv.lock JSON", &err);
            }
        },
        Err(err) => {
            log_error("Failed to read renv.lock file", &err);
        }
    }

    licenses
}

fn parse_description_file(
    desc_file_path: &str,
    _max_depth: u32,
    known_licenses: &HashMap<String, License>,
    config: &FeludaConfig,
) -> Vec<LicenseInfo> {
    let mut licenses = Vec::new();

    match fs::read_to_string(desc_file_path) {
        Ok(content) => {
            let direct_deps = parse_dcf_dependencies(&content);

            if direct_deps.is_empty() {
                log(LogLevel::Warn, "No dependencies found in DESCRIPTION file");
                return licenses;
            }

            log(
                LogLevel::Info,
                &format!(
                    "Found {} dependencies in DESCRIPTION file (direct dependencies only - use renv.lock for transitive dependencies)",
                    direct_deps.len()
                ),
            );

            let all_deps = direct_deps;

            for (name, version) in all_deps {
                log(
                    LogLevel::Info,
                    &format!("Processing R package: {name} ({version})"),
                );

                let license_result = fetch_license_for_r_dependency(&name, &version);
                let license = Some(license_result);
                let is_restrictive =
                    is_license_restrictive(&license, known_licenses, config.strict);

                if is_restrictive {
                    log(
                        LogLevel::Warn,
                        &format!("Restrictive license found: {license:?} for {name}"),
                    );
                }

                licenses.push(LicenseInfo {
                    name,
                    version,
                    license: license.clone(),
                    is_restrictive,
                    compatibility: LicenseCompatibility::Unknown,
                    osi_status: match &license {
                        Some(l) => crate::licenses::get_osi_status(l),
                        None => crate::licenses::OsiStatus::Unknown,
                    },
                });
            }
        }
        Err(err) => {
            log_error("Failed to read DESCRIPTION file", &err);
        }
    }

    licenses
}

fn parse_dcf_dependencies(content: &str) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    let mut current_field = String::new();
    let mut current_value = String::new();

    for line in content.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            current_value.push(' ');
            current_value.push_str(line.trim());
        } else if let Some((field, value)) = line.split_once(':') {
            if !current_field.is_empty() {
                process_dependency_field(&current_field, &current_value, &mut deps);
            }
            current_field = field.trim().to_string();
            current_value = value.trim().to_string();
        }
    }

    if !current_field.is_empty() {
        process_dependency_field(&current_field, &current_value, &mut deps);
    }

    deps
}

fn process_dependency_field(field: &str, value: &str, deps: &mut Vec<(String, String)>) {
    let dependency_fields = ["Imports", "Depends", "Suggests", "LinkingTo"];

    if !dependency_fields.contains(&field) {
        return;
    }

    for dep_part in value.split(',') {
        let dep_part = dep_part.trim();
        if dep_part.is_empty() || dep_part.starts_with("R (") {
            continue;
        }

        let (name, version) = if let Some((pkg, ver_spec)) = dep_part.split_once('(') {
            let pkg = pkg.trim();
            let ver = ver_spec
                .trim_end_matches(')')
                .trim()
                .replace(">=", "")
                .replace("<=", "")
                .replace(">", "")
                .replace("<", "")
                .replace("==", "")
                .trim()
                .to_string();
            (pkg.to_string(), ver)
        } else {
            (dep_part.to_string(), "latest".to_string())
        };

        deps.push((name, version));
    }
}

pub fn fetch_license_for_r_dependency(name: &str, version: &str) -> String {
    let search_url = format!("https://r-universe.dev/api/search?q={name}&limit=1");
    log(
        LogLevel::Info,
        &format!("Fetching license from R-universe: {search_url}"),
    );

    match reqwest::blocking::get(&search_url) {
        Ok(response) => {
            let status = response.status();
            log(
                LogLevel::Info,
                &format!("R-universe API response status: {status}"),
            );

            if status.is_success() {
                match response.json::<Value>() {
                    Ok(json) => {
                        if let Some(results) = json["results"].as_array() {
                            if let Some(first_result) = results.first() {
                                if let Some(user) = first_result["_user"].as_str() {
                                    let package_url = format!(
                                        "https://{user}.r-universe.dev/api/packages/{name}"
                                    );
                                    log(
                                        LogLevel::Info,
                                        &format!("Fetching package details from: {package_url}"),
                                    );

                                    if let Ok(pkg_response) = reqwest::blocking::get(&package_url) {
                                        if let Ok(pkg_json) = pkg_response.json::<Value>() {
                                            if let Some(license) = pkg_json["License"].as_str() {
                                                if !license.is_empty() {
                                                    log(
                                                        LogLevel::Info,
                                                        &format!(
                                                            "License found for {name}: {license}"
                                                        ),
                                                    );
                                                    return license.to_string();
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        log(
                            LogLevel::Warn,
                            &format!("No license found for {name} ({version})"),
                        );
                        format!("Unknown license for {name}: {version}")
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_dcf_dependencies() {
        let content = r#"Package: testpkg
Version: 1.0.0
Imports:
    dplyr (>= 1.0.0),
    ggplot2,
    tidyr (>= 1.2.0)
Suggests:
    testthat,
    knitr
"#;
        let deps = parse_dcf_dependencies(content);
        assert_eq!(deps.len(), 5);
        assert!(deps.iter().any(|(name, _)| name == "dplyr"));
        assert!(deps.iter().any(|(name, _)| name == "ggplot2"));
        assert!(deps.iter().any(|(name, _)| name == "tidyr"));
        assert!(deps.iter().any(|(name, _)| name == "testthat"));
        assert!(deps.iter().any(|(name, _)| name == "knitr"));
    }

    #[test]
    fn test_parse_dcf_dependencies_with_versions() {
        let content = r#"Imports: dplyr (>= 1.0.0), ggplot2 (>= 3.3.0)"#;
        let deps = parse_dcf_dependencies(content);
        assert_eq!(deps.len(), 2);

        let dplyr_dep = deps.iter().find(|(name, _)| name == "dplyr").unwrap();
        assert_eq!(dplyr_dep.1, "1.0.0");

        let ggplot2_dep = deps.iter().find(|(name, _)| name == "ggplot2").unwrap();
        assert_eq!(ggplot2_dep.1, "3.3.0");
    }

    #[test]
    fn test_parse_dcf_dependencies_ignores_r_version() {
        let content = r#"Depends: R (>= 4.0.0), dplyr"#;
        let deps = parse_dcf_dependencies(content);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].0, "dplyr");
    }

    #[test]
    fn test_parse_renv_lock() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("renv.lock");

        let lock_content = r#"{
  "R": {
    "Version": "4.2.0",
    "Repositories": []
  },
  "Packages": {
    "dplyr": {
      "Package": "dplyr",
      "Version": "1.0.9",
      "Source": "Repository",
      "Repository": "CRAN"
    },
    "ggplot2": {
      "Package": "ggplot2",
      "Version": "3.3.6",
      "Source": "Repository",
      "Repository": "CRAN"
    }
  }
}"#;

        fs::write(&lock_path, lock_content).unwrap();

        let known_licenses = HashMap::new();
        let config = FeludaConfig::default();
        let result = parse_renv_lock(lock_path.to_str().unwrap(), &known_licenses, &config);

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|info| info.name == "dplyr"));
        assert!(result.iter().any(|info| info.name == "ggplot2"));
    }

    #[test]
    fn test_analyze_r_licenses_description() {
        let temp_dir = TempDir::new().unwrap();
        let desc_path = temp_dir.path().join("DESCRIPTION");

        let desc_content = r#"Package: testpkg
Version: 1.0.0
Imports:
    dplyr (>= 1.0.0),
    ggplot2
"#;

        fs::write(&desc_path, desc_content).unwrap();

        let config = FeludaConfig::default();
        let result = analyze_r_licenses(desc_path.to_str().unwrap(), &config);

        assert!(!result.is_empty());
        assert!(result.iter().any(|info| info.name == "dplyr"));
        assert!(result.iter().any(|info| info.name == "ggplot2"));
    }

    #[test]
    fn test_analyze_r_licenses_empty_description() {
        let temp_dir = TempDir::new().unwrap();
        let desc_path = temp_dir.path().join("DESCRIPTION");

        fs::write(&desc_path, "Package: testpkg\nVersion: 1.0.0\n").unwrap();

        let config = FeludaConfig::default();
        let result = analyze_r_licenses(desc_path.to_str().unwrap(), &config);
        assert!(result.is_empty());
    }
}
