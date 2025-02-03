use cargo_metadata::MetadataCommand;
use std::{path::Path};

use crate::licenses::{analyze_js_licenses, analyze_rust_licenses, analyze_go_licenses,analyze_python_licenses, LicenseInfo};

#[derive(Debug, PartialEq)]
pub enum ProjectType {
    Rust,
    Node,
    Go,
    Python
}

const PYTHON_PATHS: [&str; 3] = ["requirements.txt", "Pipfile.lock", "pip_freeze.txt"];

/// Detect what type of project is this
fn detect_project_type(args_path: &str) -> Option<ProjectType> {
    let project_path = std::fs::canonicalize(args_path)
        .unwrap_or_else(|_| panic!("❌ Error: Invalid path '{}'", args_path));

    if Path::new(&project_path).join("Cargo.toml").exists() {
        // println!("🦀");
        Some(ProjectType::Rust)
    } else if Path::new(&project_path).join("package.json").exists() {
        // println!("⬢");
        Some(ProjectType::Node)
    } else if Path::new(&project_path).join("go.mod").exists() {
        // println!("🐹");
        Some(ProjectType::Go)
    } else if PYTHON_PATHS.iter().any(|path| Path::new(&project_path).join(path).exists()) {
        // println!("🐍");
        Some(ProjectType::Python)
    }
    else {
        None
    }
}

fn check_which_python_file_exists(project_path: &str) -> Option<&str> {
    for path in PYTHON_PATHS.iter() {
        if Path::new(project_path).join(path).exists() {
            return Some(path);
        }
    }
    None
}

pub fn parse_dependencies(project_path: &str) -> Vec<LicenseInfo> {
    match detect_project_type(project_path) {
        Some(ProjectType::Rust) => {
            let project_path = Path::new(project_path).join("Cargo.toml");
            let metadata = MetadataCommand::new()
                .manifest_path(Path::new(&project_path))
                .exec()
                .expect("Failed to fetch cargo metadata");

            let analyzed_data = analyze_rust_licenses(metadata.packages);

            analyzed_data
        }
        Some(ProjectType::Node) => {
            let project_path = Path::new(project_path).join("package.json");
            let analyzed_data = analyze_js_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            );

            analyzed_data
        }
        Some(ProjectType::Go) => {
            let project_path = Path::new(project_path).join("go.mod");
            let analyzed_data = analyze_go_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            );
            analyzed_data
        }
        Some(ProjectType::Python) => {
            let python_package_file = check_which_python_file_exists(project_path).expect("Python package file not found");
            let project_path = Path::new(project_path).join(python_package_file);
            let analyzed_data = analyze_python_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            );
            analyzed_data
        }
        None => {
            eprintln!("❌ Unable to detect project type.");
            std::process::exit(1);
        }
    }
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_project_type_rust() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cargo_toml_path = temp_dir.path().join("Cargo.toml");
        fs::write(&cargo_toml_path, "[package]\nname = \"test\"\nversion = \"0.1.0\"").unwrap();

        assert_eq!(detect_project_type(temp_dir.path().to_str().unwrap()), Some(ProjectType::Rust));
    }

    #[test]
    fn test_detect_project_type_node() {
        let temp_dir = tempfile::tempdir().unwrap();
        let package_json_path = temp_dir.path().join("package.json");
        fs::write(&package_json_path, "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}").unwrap();

        assert_eq!(detect_project_type(temp_dir.path().to_str().unwrap()), Some(ProjectType::Node));
    }

    #[test]
    fn test_detect_project_type_go() {
        let temp_dir = tempfile::tempdir().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");
        fs::write(&go_mod_path, "").unwrap();

        assert_eq!(detect_project_type(temp_dir.path().to_str().unwrap()), Some(ProjectType::Go));
    }

    #[test]
    fn test_detect_project_type_none() {
        let temp_dir = tempfile::tempdir().unwrap();

        assert_eq!(detect_project_type(temp_dir.path().to_str().unwrap()), None);
    }

    #[test]
    fn test_parse_dependencies_rust() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cargo_toml_path = temp_dir.path().join("Cargo.toml");
        fs::write(&cargo_toml_path, "[package]\nname = \"test\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"").unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        let lib_rs_path = src_dir.join("lib.rs");
        fs::write(&lib_rs_path, "").unwrap();

        let result = parse_dependencies(temp_dir.path().to_str().unwrap());
        let mut license_names = vec![];
        for license in &result {
            license_names.push(&license.name)
        }
        assert!(license_names.iter().any(|name| *name == "serde" ));
        assert!(!result.is_empty())
    }

    #[test]
    fn test_parse_dependencies_node() {
        let temp_dir = tempfile::tempdir().unwrap();
        let package_json_path = temp_dir.path().join("package.json");
        fs::write(&package_json_path, "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}").unwrap();

        let result = parse_dependencies(temp_dir.path().to_str().unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_dependencies_go() {
        let temp_dir = tempfile::tempdir().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");
        fs::write(&go_mod_path, "").unwrap();

        let result = parse_dependencies(temp_dir.path().to_str().unwrap());
        assert!(result.is_empty());
    }
}
