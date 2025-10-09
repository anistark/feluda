//! Core parsing coordination and project discovery functionality

use crate::cli;
use crate::debug::{log, log_debug, FeludaResult, LogLevel};
use crate::languages::{Language, CPP_PATHS, C_PATHS, PYTHON_PATHS, R_PATHS};
use crate::licenses::{
    detect_project_license, is_license_compatible, LicenseCompatibility, LicenseInfo,
};
use cargo_metadata::MetadataCommand;
use ignore::Walk;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

/// Project root information
#[derive(Debug)]
struct ProjectRoot {
    pub path: PathBuf,
    pub project_type: Language,
}

/// Walk through a directory and find all project-related roots
fn find_project_roots(root_path: impl AsRef<Path>) -> FeludaResult<Vec<ProjectRoot>> {
    let mut project_roots = Vec::new();
    log(
        LogLevel::Info,
        &format!(
            "Scanning for project files in: {}",
            root_path.as_ref().display()
        ),
    );

    for entry in Walk::new(&root_path).filter_map(|e| match e {
        Ok(entry) => Some(entry),
        Err(err) => {
            log(
                LogLevel::Error,
                &format!("Error while walking directory: {err}"),
            );
            None
        }
    }) {
        if let Some(file_type) = entry.file_type() {
            if !file_type.is_file() {
                continue;
            }
        } else {
            continue;
        }

        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let parent_path = path.parent();

        if let Some(parent) = parent_path {
            if let Some(project_type) = Language::from_file_name(file_name) {
                log(
                    LogLevel::Info,
                    &format!(
                        "Found project file: {} ({:?})",
                        path.display(),
                        project_type
                    ),
                );
                project_roots.push(ProjectRoot {
                    path: parent.to_path_buf(),
                    project_type,
                });
            }
        }
    }

    log(
        LogLevel::Info,
        &format!("Found {} project roots", project_roots.len()),
    );
    log_debug("Project roots", &project_roots);

    Ok(project_roots)
}

/// Check which C project file exists in the given path
fn check_which_c_file_exists(project_path: impl AsRef<Path>) -> Option<String> {
    for &path in C_PATHS.iter() {
        let full_path = Path::new(project_path.as_ref()).join(path);
        if full_path.exists() {
            log(
                LogLevel::Info,
                &format!("Found C project file: {}", full_path.display()),
            );
            return Some(path.to_string());
        }
    }

    log(
        LogLevel::Warn,
        &format!(
            "No C project file found in: {}",
            project_path.as_ref().display()
        ),
    );
    None
}

/// Check which C++ project file exists in the given path
fn check_which_cpp_file_exists(project_path: impl AsRef<Path>) -> Option<String> {
    for &path in CPP_PATHS.iter() {
        let full_path = Path::new(project_path.as_ref()).join(path);
        if full_path.exists() {
            log(
                LogLevel::Info,
                &format!("Found C++ project file: {}", full_path.display()),
            );
            return Some(path.to_string());
        }
    }

    log(
        LogLevel::Warn,
        &format!(
            "No C++ project file found in: {}",
            project_path.as_ref().display()
        ),
    );
    None
}

/// Check which Python project file exists in the given path
fn check_which_python_file_exists(project_path: impl AsRef<Path>) -> Option<String> {
    for &path in PYTHON_PATHS.iter() {
        let full_path = Path::new(project_path.as_ref()).join(path);
        if full_path.exists() {
            log(
                LogLevel::Info,
                &format!("Found Python project file: {}", full_path.display()),
            );
            return Some(path.to_string());
        }
    }

    log(
        LogLevel::Warn,
        &format!(
            "No Python project file found in: {}",
            project_path.as_ref().display()
        ),
    );
    None
}

/// Check which R project file exists in the given path
fn check_which_r_file_exists(project_path: impl AsRef<Path>) -> Option<String> {
    for &path in R_PATHS.iter() {
        let full_path = Path::new(project_path.as_ref()).join(path);
        if full_path.exists() {
            log(
                LogLevel::Info,
                &format!("Found R project file: {}", full_path.display()),
            );
            return Some(path.to_string());
        }
    }

    log(
        LogLevel::Warn,
        &format!(
            "No R project file found in: {}",
            project_path.as_ref().display()
        ),
    );
    None
}

/// Main entry point for parsing project dependencies
pub fn parse_root(
    root_path: impl AsRef<Path>,
    language: Option<&str>,
) -> FeludaResult<Vec<LicenseInfo>> {
    let config = crate::config::load_config()?;
    parse_root_with_config(root_path, language, &config)
}

/// Main entry point for parsing project dependencies
pub fn parse_root_with_config(
    root_path: impl AsRef<Path>,
    language: Option<&str>,
    config: &crate::config::FeludaConfig,
) -> FeludaResult<Vec<LicenseInfo>> {
    log(
        LogLevel::Info,
        &format!("Parsing root path: {}", root_path.as_ref().display()),
    );
    if let Some(lang) = language {
        log(LogLevel::Info, &format!("Filtering by language: {lang}"));
    }

    let project_roots = find_project_roots(&root_path)?;

    if project_roots.is_empty() {
        log(
            LogLevel::Warn,
            "No project files found in the specified path",
        );
        println!(
            "❌ No supported project files found.\n\
            Feluda supports: C, C++, Rust, Node.js, Go, Python, R"
        );
        return Ok(Vec::new());
    }

    let licenses: Vec<LicenseInfo> = project_roots
        .into_par_iter()
        .filter_map(|root| {
            if let Some(language) = language {
                if !matches_language(root.project_type, language) {
                    log(
                        LogLevel::Info,
                        &format!(
                            "Skipping {:?} project (language filter: {})",
                            root.project_type, language
                        ),
                    );
                    return None;
                }
            }

            match parse_dependencies(&root, config) {
                Ok(deps) => {
                    log(
                        LogLevel::Info,
                        &format!(
                            "Found {} dependencies in {}",
                            deps.len(),
                            root.path.display()
                        ),
                    );
                    Some(deps)
                }
                Err(err) => {
                    log(
                        LogLevel::Error,
                        &format!(
                            "Error parsing dependencies in {}: {}",
                            root.path.display(),
                            err
                        ),
                    );
                    None
                }
            }
        })
        .flatten()
        .collect();

    log(
        LogLevel::Info,
        &format!("Total dependencies found: {}", licenses.len()),
    );

    // Set license compatibility based on project license
    let project_license =
        detect_project_license(root_path.as_ref().to_str().unwrap_or("")).unwrap_or(None);

    let mut licenses = licenses;
    set_license_compatibility(&mut licenses, &project_license);

    Ok(licenses)
}

/// Set license compatibility for all dependencies
fn set_license_compatibility(licenses: &mut [LicenseInfo], project_license: &Option<String>) {
    for license in licenses {
        license.compatibility = match (project_license, &license.license) {
            (Some(proj_license), Some(dep_license)) => {
                is_license_compatible(dep_license, proj_license)
            }
            _ => LicenseCompatibility::Unknown,
        };
    }
}

/// Check if a project type matches the given language filter
fn matches_language(project_type: Language, language: &str) -> bool {
    matches!(
        (project_type, language.to_lowercase().as_str()),
        (Language::C(_), "c")
            | (Language::Cpp(_), "cpp" | "c++")
            | (Language::Rust(_), "rust")
            | (Language::Node(_), "node")
            | (Language::Go(_), "go")
            | (Language::Python(_), "python")
            | (Language::R(_), "r")
    )
}

/// Parse dependencies based on the project type
fn parse_dependencies(
    root: &ProjectRoot,
    config: &crate::config::FeludaConfig,
) -> FeludaResult<Vec<LicenseInfo>> {
    let project_path = &root.path;
    let project_type = root.project_type;

    let licenses = cli::with_spinner(&format!("🔎: {}", project_path.display()), |indicator| {
        match project_type {
            Language::Rust(_) => {
                let project_path = Path::new(project_path).join("Cargo.toml");
                log(
                    LogLevel::Info,
                    &format!("Parsing Rust project: {}", project_path.display()),
                );

                indicator.update_progress("analyzing Cargo.toml");

                match MetadataCommand::new()
                    .manifest_path(Path::new(&project_path))
                    .exec()
                {
                    Ok(metadata) => {
                        log(
                            LogLevel::Info,
                            &format!("Found {} packages in Rust project", metadata.packages.len()),
                        );
                        indicator.update_progress(&format!(
                            "found {} packages",
                            metadata.packages.len()
                        ));

                        crate::languages::analyze_rust_licenses(metadata.packages)
                    }
                    Err(err) => {
                        log(
                            LogLevel::Error,
                            &format!("Failed to fetch cargo metadata: {err}"),
                        );
                        Vec::new()
                    }
                }
            }
            Language::Node(_) => {
                let project_path = Path::new(project_path).join("package.json");
                log(
                    LogLevel::Info,
                    &format!("Parsing Node.js project: {}", project_path.display()),
                );

                indicator.update_progress("analyzing package.json");

                match project_path.to_str() {
                    Some(path_str) => {
                        let deps = crate::languages::analyze_js_licenses(path_str);
                        indicator.update_progress(&format!("found {} dependencies", deps.len()));
                        deps
                    }
                    None => {
                        log(LogLevel::Error, "Failed to convert Node.js path to string");
                        Vec::new()
                    }
                }
            }
            Language::Go(_) => {
                let project_path = Path::new(project_path).join("go.mod");
                log(
                    LogLevel::Info,
                    &format!("Parsing Go project: {}", project_path.display()),
                );

                indicator.update_progress("analyzing go.mod");

                match project_path.to_str() {
                    Some(path_str) => {
                        let deps = crate::languages::analyze_go_licenses(path_str, config);
                        indicator.update_progress(&format!("found {} dependencies", deps.len()));
                        deps
                    }
                    None => {
                        log(LogLevel::Error, "Failed to convert Go path to string");
                        Vec::new()
                    }
                }
            }
            Language::Python(_) => match check_which_python_file_exists(project_path) {
                Some(python_package_file) => {
                    let project_path = Path::new(project_path).join(&python_package_file);
                    log(
                        LogLevel::Info,
                        &format!("Parsing Python project: {}", project_path.display()),
                    );

                    indicator.update_progress(&format!("analyzing {python_package_file}"));

                    match project_path.to_str() {
                        Some(path_str) => {
                            let deps = crate::languages::analyze_python_licenses(path_str, config);
                            indicator
                                .update_progress(&format!("found {} dependencies", deps.len()));
                            deps
                        }
                        None => {
                            log(LogLevel::Error, "Failed to convert Python path to string");
                            Vec::new()
                        }
                    }
                }
                None => {
                    log(LogLevel::Error, "Python package file not found");
                    Vec::new()
                }
            },
            Language::C(_) => match check_which_c_file_exists(project_path) {
                Some(c_build_file) => {
                    let project_path = Path::new(project_path).join(&c_build_file);
                    log(
                        LogLevel::Info,
                        &format!("Parsing C project: {}", project_path.display()),
                    );

                    indicator.update_progress(&format!("analyzing {c_build_file}"));

                    match project_path.to_str() {
                        Some(path_str) => {
                            let deps = crate::languages::analyze_c_licenses(path_str, config);
                            indicator
                                .update_progress(&format!("found {} dependencies", deps.len()));
                            deps
                        }
                        None => {
                            log(LogLevel::Error, "Failed to convert C path to string");
                            Vec::new()
                        }
                    }
                }
                None => {
                    log(LogLevel::Error, "C build file not found");
                    Vec::new()
                }
            },
            Language::Cpp(_) => match check_which_cpp_file_exists(project_path) {
                Some(cpp_build_file) => {
                    let project_path = Path::new(project_path).join(&cpp_build_file);
                    log(
                        LogLevel::Info,
                        &format!("Parsing C++ project: {}", project_path.display()),
                    );

                    indicator.update_progress(&format!("analyzing {cpp_build_file}"));

                    match project_path.to_str() {
                        Some(path_str) => {
                            let deps = crate::languages::analyze_cpp_licenses(path_str, config);
                            indicator
                                .update_progress(&format!("found {} dependencies", deps.len()));
                            deps
                        }
                        None => {
                            log(LogLevel::Error, "Failed to convert C++ path to string");
                            Vec::new()
                        }
                    }
                }
                None => {
                    log(LogLevel::Error, "C++ build file not found");
                    Vec::new()
                }
            },
            Language::R(_) => match check_which_r_file_exists(project_path) {
                Some(r_package_file) => {
                    let project_path = Path::new(project_path).join(&r_package_file);
                    log(
                        LogLevel::Info,
                        &format!("Parsing R project: {}", project_path.display()),
                    );

                    indicator.update_progress(&format!("analyzing {r_package_file}"));

                    match project_path.to_str() {
                        Some(path_str) => {
                            let deps = crate::languages::analyze_r_licenses(path_str, config);
                            indicator
                                .update_progress(&format!("found {} dependencies", deps.len()));
                            deps
                        }
                        None => {
                            log(LogLevel::Error, "Failed to convert R path to string");
                            Vec::new()
                        }
                    }
                }
                None => {
                    log(LogLevel::Error, "R package file not found");
                    Vec::new()
                }
            },
        }
    });

    Ok(licenses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_language() {
        assert!(matches_language(Language::C(&C_PATHS), "c"));
        assert!(matches_language(Language::C(&C_PATHS), "C"));

        assert!(matches_language(Language::Cpp(&CPP_PATHS), "cpp"));
        assert!(matches_language(Language::Cpp(&CPP_PATHS), "c++"));
        assert!(matches_language(Language::Cpp(&CPP_PATHS), "CPP"));

        assert!(matches_language(Language::Rust("Cargo.toml"), "rust"));
        assert!(matches_language(Language::Rust("Cargo.toml"), "RUST"));
        assert!(matches_language(Language::Rust("Cargo.toml"), "Rust"));

        assert!(matches_language(Language::Node("package.json"), "node"));
        assert!(matches_language(Language::Node("package.json"), "NODE"));
        assert!(matches_language(Language::Node("package.json"), "Node"));

        assert!(matches_language(Language::Go("go.mod"), "go"));
        assert!(matches_language(Language::Go("go.mod"), "GO"));
        assert!(matches_language(Language::Go("go.mod"), "Go"));

        assert!(matches_language(Language::Python(&PYTHON_PATHS), "python"));
        assert!(matches_language(Language::Python(&PYTHON_PATHS), "PYTHON"));
        assert!(matches_language(Language::Python(&PYTHON_PATHS), "Python"));

        assert!(!matches_language(Language::Rust("Cargo.toml"), "node"));
        assert!(!matches_language(Language::Node("package.json"), "python"));
        assert!(!matches_language(Language::Go("go.mod"), "rust"));
        assert!(!matches_language(Language::Python(&PYTHON_PATHS), "go"));
        assert!(!matches_language(Language::C(&C_PATHS), "cpp"));
        assert!(!matches_language(Language::Cpp(&CPP_PATHS), "c"));

        assert!(!matches_language(Language::Rust("Cargo.toml"), "java"));
        assert!(!matches_language(Language::Node("package.json"), "java"));
    }

    #[test]
    fn test_check_which_python_file_exists() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Test when no Python files exist
        let result = check_which_python_file_exists(temp_dir.path());
        assert_eq!(result, None);

        // Test when requirements.txt exists
        std::fs::write(temp_dir.path().join("requirements.txt"), "requests==2.28.1").unwrap();
        let result = check_which_python_file_exists(temp_dir.path());
        assert_eq!(result, Some("requirements.txt".to_string()));

        // Test when multiple Python files exist
        std::fs::write(
            temp_dir.path().join("pyproject.toml"),
            "[project]\nname = \"test\"",
        )
        .unwrap();
        std::fs::write(temp_dir.path().join("Pipfile.lock"), "{}").unwrap();
        let result = check_which_python_file_exists(temp_dir.path());
        assert_eq!(result, Some("requirements.txt".to_string()));
    }

    #[test]
    fn test_find_project_roots_empty_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let result = find_project_roots(temp_dir.path().to_str().unwrap()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_project_roots_single_project() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root_path = temp_dir.path();

        // Create a single Rust project
        std::fs::write(root_path.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        let result = find_project_roots(root_path.to_str().unwrap()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].project_type, Language::Rust("Cargo.toml"));
        assert_eq!(result[0].path, root_path);
    }

    #[test]
    fn test_parse_root_with_language_filter() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root_path = temp_dir.path();

        // Create multiple project types
        std::fs::write(root_path.join("package.json"), r#"{"name": "test"}"#).unwrap();
        std::fs::write(root_path.join("go.mod"), "module test").unwrap();
        std::fs::write(root_path.join("requirements.txt"), "# No dependencies").unwrap();

        // Test filtering by node
        let result = parse_root(root_path, Some("node"));
        assert!(result.is_ok());

        // Test filtering by go
        let result = parse_root(root_path, Some("go"));
        assert!(result.is_ok());

        // Test filtering by python
        let result = parse_root(root_path, Some("python"));
        assert!(result.is_ok());

        // Test filtering by non-existent language
        let result = parse_root(root_path, Some("java"));
        assert!(result.is_ok());
        let licenses = result.unwrap();
        assert!(licenses.is_empty());

        // Test case-insensitive filtering
        let result = parse_root(root_path, Some("NODE"));
        assert!(result.is_ok());

        let result = parse_root(root_path, Some("Python"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_root_no_projects() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let result = parse_root(temp_dir.path(), None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_root_all_languages() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root_path = temp_dir.path();

        // Create project files for all supported languages
        std::fs::write(
            root_path.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"",
        )
        .unwrap();
        std::fs::write(
            root_path.join("package.json"),
            r#"{"name": "test", "version": "1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(root_path.join("go.mod"), "module test\n\ngo 1.19").unwrap();
        std::fs::write(root_path.join("requirements.txt"), "# No dependencies").unwrap();

        let result = parse_root(root_path, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_project_root_debug() {
        let project_root = ProjectRoot {
            path: std::path::PathBuf::from("/test/path"),
            project_type: Language::Rust("Cargo.toml"),
        };

        let debug_str = format!("{project_root:?}");
        assert!(debug_str.contains("/test/path"));
        assert!(debug_str.contains("Rust"));
        assert!(debug_str.contains("Cargo.toml"));
    }

    #[test]
    fn test_find_project_roots_nested_projects() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root_path = temp_dir.path();

        // Create nested structure
        let rust_dir = root_path.join("rust_project");
        let node_dir = root_path.join("node_project").join("nested");
        std::fs::create_dir_all(&rust_dir).unwrap();
        std::fs::create_dir_all(&node_dir).unwrap();

        // Create project files
        std::fs::write(rust_dir.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        std::fs::write(node_dir.join("package.json"), "{}").unwrap();
        std::fs::write(root_path.join("go.mod"), "module test").unwrap();

        let result = find_project_roots(root_path.to_str().unwrap()).unwrap();
        assert_eq!(result.len(), 3);

        let project_types: Vec<_> = result.iter().map(|r| r.project_type).collect();
        assert!(project_types.contains(&Language::Rust("Cargo.toml")));
        assert!(project_types.contains(&Language::Node("package.json")));
        assert!(project_types.contains(&Language::Go("go.mod")));
    }

    #[test]
    fn test_parse_dependencies_error_handling() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Test with invalid Rust project (missing lib.rs)
        let rust_project_root = ProjectRoot {
            path: temp_dir.path().to_path_buf(),
            project_type: Language::Rust("Cargo.toml"),
        };

        // Create Cargo.toml without lib.rs
        std::fs::write(
            temp_dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1.0\"",
        )
        .unwrap();

        let config = crate::config::FeludaConfig::default();
        let result = parse_dependencies(&rust_project_root, &config);
        assert!(result.is_ok());
        let licenses = result.unwrap();
        assert!(licenses.is_empty());
    }

    #[test]
    fn test_parse_dependencies_node_invalid_json() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let node_project_root = ProjectRoot {
            path: temp_dir.path().to_path_buf(),
            project_type: Language::Node("package.json"),
        };

        // Create invalid package.json
        std::fs::write(temp_dir.path().join("package.json"), "invalid json content").unwrap();

        let config = crate::config::FeludaConfig::default();
        let result = parse_dependencies(&node_project_root, &config);
        assert!(result.is_ok());
        let licenses = result.unwrap();
        assert!(licenses.is_empty());
    }

    #[test]
    fn test_parse_dependencies_python_no_dependencies() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let python_project_root = ProjectRoot {
            path: temp_dir.path().to_path_buf(),
            project_type: Language::Python(&PYTHON_PATHS),
        };

        // Create empty requirements.txt
        std::fs::write(temp_dir.path().join("requirements.txt"), "").unwrap();

        let config = crate::config::FeludaConfig::default();
        let result = parse_dependencies(&python_project_root, &config);
        assert!(result.is_ok());
        let licenses = result.unwrap();
        assert!(licenses.is_empty());
    }

    #[test]
    fn test_parse_root_invalid_path() {
        let result = parse_root("/definitely/nonexistent/path", None);
        assert!(result.is_ok());
        let licenses = result.unwrap();
        assert!(licenses.is_empty());
    }
}
