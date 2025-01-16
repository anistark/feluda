use cargo_metadata::MetadataCommand;
use std::path::Path;

use crate::licenses::{analyze_js_licenses, analyze_rust_licenses, analyze_go_licenses, LicenseInfo};

// Detect what type of project is this
fn detect_project_type(args_path: &str) -> Option<&'static str> {
    let project_path = std::fs::canonicalize(args_path)
        .unwrap_or_else(|_| panic!("❌ Error: Invalid path '{}'", args_path));

    if Path::new(&project_path).join("Cargo.toml").exists() {
        // println!("🦀");
        Some("rust")
    } else if Path::new(&project_path).join("package.json").exists() {
        // println!("⬢");
        Some("node")
    } else if Path::new(&project_path).join("go.mod").exists() {
        // println!("🐹");
        Some("go")
    } else {
        None
    }
}

pub fn parse_dependencies(project_path: &str) -> Vec<LicenseInfo> {
    match detect_project_type(project_path) {
        Some("rust") => {
            let project_path = Path::new(project_path).join("Cargo.toml");
            let metadata = MetadataCommand::new()
                .manifest_path(Path::new(&project_path))
                .exec()
                .expect("Failed to fetch cargo metadata");

            let analyzed_data = analyze_rust_licenses(metadata.packages);

            analyzed_data
        }
        Some("node") => {
            let project_path = Path::new(project_path).join("package.json");
            let analyzed_data = analyze_js_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            );

            analyzed_data
        }
        Some("go") => {
            let project_path = Path::new(project_path).join("go.mod");
            let analyzed_data = analyze_go_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            );
            analyzed_data
        }
        Some(_) => {
            eprintln!("⚠️ Unsupported project type detected.");
            std::process::exit(1);
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

        assert_eq!(detect_project_type(temp_dir.path().to_str().unwrap()), Some("rust"));
    }

    #[test]
    fn test_detect_project_type_node() {
        let temp_dir = tempfile::tempdir().unwrap();
        let package_json_path = temp_dir.path().join("package.json");
        fs::write(&package_json_path, "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}").unwrap();

        assert_eq!(detect_project_type(temp_dir.path().to_str().unwrap()), Some("node"));
    }

    #[test]
    fn test_detect_project_type_go() {
        let temp_dir = tempfile::tempdir().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");
        fs::write(&go_mod_path, "").unwrap();

        assert_eq!(detect_project_type(temp_dir.path().to_str().unwrap()), Some("go"));
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
        assert!(!result.is_empty());
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
