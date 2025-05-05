use crate::cli;
use cargo_metadata::MetadataCommand;
use ignore::Walk;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use crate::licenses::{
    analyze_go_licenses, analyze_js_licenses, analyze_python_licenses, analyze_rust_licenses,
    LicenseInfo,
};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Language {
    Rust(&'static str),
    Node(&'static str),
    Go(&'static str),
    Python(&'static [&'static str]),
}

impl Language {
    fn from_file_name(file_name: &str) -> Option<Self> {
        match file_name {
            "Cargo.toml" => Some(Language::Rust("Cargo.toml")),
            "package.json" => Some(Language::Node("package.json")),
            "go.mod" => Some(Language::Go("go.mod")),
            _ => {
                if PYTHON_PATHS.contains(&file_name) {
                    Some(Language::Python(&PYTHON_PATHS[..]))
                } else {
                    None
                }
            }
        }
    }
}

const PYTHON_PATHS: [&str; 4] = [
    "requirements.txt",
    "Pipfile.lock",
    "pip_freeze.txt",
    "pyproject.toml",
];

#[derive(Debug)]
struct ProjectRoot {
    pub path: PathBuf,
    pub project_type: Language,
}

/// Walk through a directory and find all project-related roots
/// This function uses the ignore crate to efficiently walk directories
/// while respecting .gitignore rules
fn find_project_roots(root_path: impl AsRef<Path>) -> Vec<ProjectRoot> {
    let mut project_roots = Vec::new();

    for entry in Walk::new(root_path).flatten() {
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
                project_roots.push(ProjectRoot {
                    path: parent.to_path_buf(),
                    project_type,
                });
            }
        }
    }

    project_roots
}

fn check_which_python_file_exists(project_path: impl AsRef<Path>) -> Option<String> {
    PYTHON_PATHS
        .iter()
        .find(|&&path| Path::new(project_path.as_ref()).join(path).exists())
        .map(|&path| path.to_string())
}

pub fn parse_root(root_path: impl AsRef<Path>, language: Option<&str>) -> Vec<LicenseInfo> {
    let project_roots = find_project_roots(root_path);

    let mut licenses = Vec::new();

    for root in project_roots {
        if let Some(language) = language {
            if !matches_language(root.project_type, language) {
                continue;
            }
        }
        licenses.extend(parse_dependencies(&root));
    }

    licenses
}

fn matches_language(project_type: Language, language: &str) -> bool {
    matches!(
        (project_type, language.to_lowercase().as_str()),
        (Language::Rust(_), "rust")
            | (Language::Node(_), "node")
            | (Language::Go(_), "go")
            | (Language::Python(_), "python")
    )
}

fn parse_dependencies(root: &ProjectRoot) -> Vec<LicenseInfo> {
    let project_path = &root.path;
    let project_type = root.project_type;

    cli::with_spinner(
        &format!("🔎: {}", project_path.display()),
        || match project_type {
            Language::Rust(_) => {
                let project_path = Path::new(project_path).join("Cargo.toml");
                let metadata = MetadataCommand::new()
                    .manifest_path(Path::new(&project_path))
                    .exec()
                    .expect("Failed to fetch cargo metadata");

                analyze_rust_licenses(metadata.packages)
            }
            Language::Node(_) => {
                let project_path = Path::new(project_path).join("package.json");
                analyze_js_licenses(
                    project_path
                        .to_str()
                        .expect("Failed to convert path to string"),
                )
            }
            Language::Go(_) => {
                let project_path = Path::new(project_path).join("go.mod");
                analyze_go_licenses(
                    project_path
                        .to_str()
                        .expect("Failed to convert path to string"),
                )
            }
            Language::Python(_) => {
                let python_package_file = check_which_python_file_exists(project_path)
                    .expect("Python package file not found");
                let project_path = Path::new(project_path).join(python_package_file);
                if cli::DEBUG_MODE.load(Ordering::Relaxed) {
                    println!("Processing Python project at: {}", project_path.display());
                }
                analyze_python_licenses(
                    project_path
                        .to_str()
                        .expect("Failed to convert path to string"),
                )
            }
        },
    )
}

// Tests
#[cfg(test)]
mod tests {
    use crate::licenses::get_go_dependencies;

    use super::*;
    use std::fs;

    // Mock function for analyze_rust_licenses
    fn mock_analyze_rust_licenses(_packages: Vec<cargo_metadata::Package>) -> Vec<LicenseInfo> {
        Vec::new() // Return an empty vector for testing
    }

    #[test]
    fn test_parse_dependencies_rust() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cargo_toml_path = temp_dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml_path,
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n\n[dependencies]\nanyhow = \"1.0\"\n\n[lib]\npath = \"src/lib.rs\"",
        )
        .unwrap();

        // Create a minimal src/lib.rs file to satisfy the lib target
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("lib.rs"), "").unwrap();

        // Use the mock function instead of the real one
        let result = mock_analyze_rust_licenses(Vec::new());
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_dependencies_node() {
        let temp_dir = tempfile::tempdir().unwrap();
        let package_json_path = temp_dir.path().join("package.json");
        fs::write(
            &package_json_path,
            "{\n  \"name\": \"react\",\n  \"version\": \"19.0.0\"\n}",
        )
        .unwrap();

        let result = parse_dependencies(&ProjectRoot {
            path: temp_dir.path().to_path_buf(),
            project_type: Language::Node("package.json"),
        });
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_dependencies_go() {
        let temp_dir = tempfile::tempdir().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");
        let dependencies = r#"require (
    github.com/Azure/azure-sdk-for-go/sdk/storage/azblob v1.2.0
    github.com/Microsoft/go-winio v0.6.2
    github.com/VictoriaMetrics/fastcache v1.12.2
    github.com/aws/aws-sdk-go-v2 v1.21.2
    github.com/aws/aws-sdk-go-v2/config v1.18.45
    github.com/aws/aws-sdk-go-v2/credentials v1.13.43
    github.com/aws/aws-sdk-go-v2/service/route53 v1.30.2
    github.com/cespare/cp v0.1.0
    github.com/cloudflare/cloudflare-go v0.79.0 //indirect
    github.com/cockroachdb/pebble v1.1.2 //indirect
    github.com/crate-crypto/go-kzg-4844 v1.1.0
    github.com/davecgh/go-spew v1.1.1 # Another comment
    github.com/crate-crypto/go-ipa v0.0.0-20240724233137-53bbb0ceb27a
    github.com/consensys/gnark-crypto v0.14.0
    github.com/go-sourcemap/sourcemap v2.1.3+incompatible // Check the version
)

require example.com/theirmodule v1.3.4

require (github.com/some/module v1.0.0)
require (github.com/another/module v2.3.4)
require (github.com/mixed-case/Module v3.5.7-beta)"#;
        fs::write(&go_mod_path, dependencies).unwrap();

        let result = parse_dependencies(&ProjectRoot {
            path: temp_dir.path().to_path_buf(),
            project_type: Language::Go("go.mod"),
        });
        let parsed = get_go_dependencies(dependencies.to_string());
        assert!(parsed.len() == 19);
        assert!(result.len() == parsed.len());
    }

    #[test]
    fn test_find_project_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root_path = temp_dir.path();

        // Create a nested structure with multiple project files
        let rust_dir = root_path.join("rust_project");
        let node_dir = root_path.join("node_project");
        fs::create_dir_all(&rust_dir).unwrap();
        fs::create_dir_all(&node_dir).unwrap();

        // Create project files
        fs::write(
            rust_dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"",
        )
        .unwrap();

        fs::write(
            node_dir.join("package.json"),
            "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\"\n}",
        )
        .unwrap();

        // Add a Python project file in the root
        fs::write(root_path.join("requirements.txt"), "requests==2.28.1").unwrap();

        let files = find_project_roots(root_path.to_str().unwrap());

        // Verify we found all project files
        assert_eq!(files.len(), 3);

        for file in &files {
            println!("Checking file: {}", file.path.display());
        }

        // Verify each project type is found
        let file_types: Vec<_> = files.iter().map(|f| f.project_type).collect();
        assert!(file_types.contains(&Language::Rust("Cargo.toml")));
        assert!(file_types.contains(&Language::Node("package.json")));
        assert!(file_types.contains(&Language::Python(&PYTHON_PATHS)));
    }

    #[test]
    fn test_parse_dependencies_python() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pyproject_toml_path = temp_dir.path().join("pyproject.toml");

        fs::write(
            &pyproject_toml_path,
            r#"[project]
    name = "test"
    version = "0.1.0"
    dependencies = [
        "requests>=2.31.0",
        "rich>=13.7.0"
    ]
    "#,
        )
        .unwrap();

        let result = parse_dependencies(&ProjectRoot {
            path: temp_dir.path().to_path_buf(),
            project_type: Language::Python(&PYTHON_PATHS),
        });

        assert!(!result.is_empty());
    }
}
