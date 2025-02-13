use cargo_metadata::MetadataCommand;
use std::path::Path;
use colored::*;
use crate::cli;

use crate::licenses::{
    analyze_go_licenses, analyze_js_licenses, analyze_python_licenses, analyze_rust_licenses,
    LicenseInfo,
};

#[derive(Debug, PartialEq, Clone)]
pub enum ProjectType {
    Rust,
    Node,
    Go,
    Python,
}

const PYTHON_PATHS: [&str; 3] = ["requirements.txt", "Pipfile.lock", "pip_freeze.txt"];

/// Detect what type of project is this
fn detect_project_type(args_path: &str) -> Option<ProjectType> {
    let project_path = std::fs::canonicalize(args_path)
        .unwrap_or_else(|_| panic!("❌ Error: Invalid path '{}'", args_path));

    let is_rust = Path::new(&project_path).join("Cargo.toml").exists();
    let is_node = Path::new(&project_path).join("package.json").exists();
    let is_go = Path::new(&project_path).join("go.mod").exists();
    let is_python = PYTHON_PATHS
        .iter()
        .any(|path| Path::new(&project_path).join(path).exists());

    let mut project_types = vec![];

    if is_rust {
        project_types.push(ProjectType::Rust);
    }
    if is_node {
        project_types.push(ProjectType::Node);
    }
    if is_go {
        project_types.push(ProjectType::Go);
    }
    if is_python {
        project_types.push(ProjectType::Python);
    }

    match project_types.len() {
        0 => None,
        1 => Some(project_types[0].clone()),
        _ => {
            println!("❌ Multiple project types detected: {:?}", project_types);
            println!("Please specify which one to run Feluda for by entering the corresponding number:");

            for (index, project_type) in project_types.iter().enumerate() {
                println!("{}: {:?}", index + 1, project_type);
            }

            let mut input = String::new();
            println!("{}", "Please enter your choice:".cyan());
            std::io::stdin().read_line(&mut input).expect("Failed to read input");
            let choice: usize = input.trim().parse().expect("Invalid input");

            if choice == 0 || choice > project_types.len() {
                eprintln!("❌ Invalid choice.");
                std::process::exit(1);
            }

            Some(project_types[choice - 1].clone())
        }
    }
}

fn check_which_python_file_exists(project_path: &str) -> Option<&str> {
    PYTHON_PATHS
        .into_iter()
        .find(|&path| Path::new(project_path).join(path).exists())
}

pub fn parse_dependencies(project_path: &str) -> Vec<LicenseInfo> {
    let project_type = detect_project_type(project_path);
    
    cli::with_spinner("🔎", || {
        match project_type {
            Some(ProjectType::Rust) => {
                let project_path = Path::new(project_path).join("Cargo.toml");
                let metadata = MetadataCommand::new()
                    .manifest_path(Path::new(&project_path))
                    .exec()
                    .expect("Failed to fetch cargo metadata");

                analyze_rust_licenses(metadata.packages)
            }
            Some(ProjectType::Node) => {
                let project_path = Path::new(project_path).join("package.json");
                analyze_js_licenses(
                    project_path
                        .to_str()
                        .expect("Failed to convert path to string"),
                )
            }
            Some(ProjectType::Go) => {
                let project_path = Path::new(project_path).join("go.mod");
                analyze_go_licenses(
                    project_path
                        .to_str()
                        .expect("Failed to convert path to string"),
                )
            }
            Some(ProjectType::Python) => {
                let python_package_file = check_which_python_file_exists(project_path)
                    .expect("Python package file not found");
                let project_path = Path::new(project_path).join(python_package_file);
                analyze_python_licenses(
                    project_path
                        .to_str()
                        .expect("Failed to convert path to string"),
                )
            }
            None => {
                eprintln!("❌ Unable to detect project type.");
                std::process::exit(1);
            }
        }
    })
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
        fs::write(
            &cargo_toml_path,
            "[package]\nname = \"test\"\nversion = \"0.1.0\"",
        )
        .unwrap();

        assert_eq!(
            detect_project_type(temp_dir.path().to_str().unwrap()),
            Some(ProjectType::Rust)
        );
    }

    #[test]
    fn test_detect_project_type_node() {
        let temp_dir = tempfile::tempdir().unwrap();
        let package_json_path = temp_dir.path().join("package.json");
        fs::write(
            &package_json_path,
            "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}",
        )
        .unwrap();

        assert_eq!(
            detect_project_type(temp_dir.path().to_str().unwrap()),
            Some(ProjectType::Node)
        );
    }

    #[test]
    fn test_detect_project_type_go() {
        let temp_dir = tempfile::tempdir().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");
        fs::write(&go_mod_path, "").unwrap();

        assert_eq!(
            detect_project_type(temp_dir.path().to_str().unwrap()),
            Some(ProjectType::Go)
        );
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
        fs::write(
            &cargo_toml_path,
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"",
        )
        .unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        let lib_rs_path = src_dir.join("lib.rs");
        fs::write(&lib_rs_path, "").unwrap();

        let result = parse_dependencies(temp_dir.path().to_str().unwrap());
        let mut license_names = vec![];
        for license in &result {
            license_names.push(&license.name)
        }
        assert!(license_names.iter().any(|name| *name == "serde"));
        assert!(!result.is_empty())
    }

    #[test]
    fn test_parse_dependencies_node() {
        let temp_dir = tempfile::tempdir().unwrap();
        let package_json_path = temp_dir.path().join("package.json");
        fs::write(
            &package_json_path,
            "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}",
        )
        .unwrap();

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
