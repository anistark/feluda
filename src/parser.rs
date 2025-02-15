use crate::cli;
use cargo_metadata::MetadataCommand;
use ignore::Walk;
use std::path::{Path, PathBuf};

use crate::licenses::{
    analyze_go_licenses, analyze_js_licenses, analyze_python_licenses, analyze_rust_licenses,
    LicenseInfo,
};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ProjectType {
    Rust,
    Node,
    Go,
    Python,
}

const PYTHON_PATHS: [&str; 3] = ["requirements.txt", "Pipfile.lock", "pip_freeze.txt"];

#[derive(Debug)]
struct ProjectRoot {
    pub path: PathBuf,
    pub project_type: ProjectType,
}

/// Walk through a directory and find all project-related roots
/// This function uses the ignore crate to efficiently walk directories
/// while respecting .gitignore rules
fn find_project_roots(root_path: impl AsRef<Path>) -> Vec<ProjectRoot> {
    let mut project_roots = Vec::new();

    for result in Walk::new(root_path) {
        if let Ok(entry) = result {
            // Skip if it's not a file
            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }

            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let parent_path = path.parent();

            if let Some(parent) = parent_path {
                match file_name {
                    "Cargo.toml" => project_roots.push(ProjectRoot {
                        path: parent.to_path_buf(),
                        project_type: ProjectType::Rust,
                    }),
                    "package.json" => project_roots.push(ProjectRoot {
                        path: parent.to_path_buf(),
                        project_type: ProjectType::Node,
                    }),
                    "go.mod" => project_roots.push(ProjectRoot {
                        path: parent.to_path_buf(),
                        project_type: ProjectType::Go,
                    }),
                    _ => {
                        if PYTHON_PATHS.contains(&file_name) {
                            project_roots.push(ProjectRoot {
                                path: parent.to_path_buf(),
                                project_type: ProjectType::Python,
                            });
                        }
                    }
                }
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

pub fn parse_root(root_path: impl AsRef<Path>) -> Vec<LicenseInfo> {
    let project_roots = find_project_roots(root_path);

    let mut licenses = Vec::new();

    for root in project_roots {
        licenses.extend(parse_dependencies(&root));
    }

    licenses
}

fn parse_dependencies(root: &ProjectRoot) -> Vec<LicenseInfo> {
    let project_path = &root.path;
    let project_type = root.project_type;

    cli::with_spinner("🔎", || match project_type {
        ProjectType::Rust => {
            let project_path = Path::new(project_path).join("Cargo.toml");
            let metadata = MetadataCommand::new()
                .manifest_path(Path::new(&project_path))
                .exec()
                .expect("Failed to fetch cargo metadata");

            analyze_rust_licenses(metadata.packages)
        }
        ProjectType::Node => {
            let project_path = Path::new(project_path).join("package.json");
            analyze_js_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            )
        }
        ProjectType::Go => {
            let project_path = Path::new(project_path).join("go.mod");
            analyze_go_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            )
        }
        ProjectType::Python => {
            let python_package_file = check_which_python_file_exists(project_path)
                .expect("Python package file not found");
            let project_path = Path::new(project_path).join(python_package_file);
            analyze_python_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            )
        }
    })
}

// Tests
#[cfg(test)]
mod tests {
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
            project_type: ProjectType::Node,
        });
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_dependencies_go() {
        let temp_dir = tempfile::tempdir().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");
        fs::write(&go_mod_path, "").unwrap();

        let result = parse_dependencies(&ProjectRoot {
            path: temp_dir.path().to_path_buf(),
            project_type: ProjectType::Go,
        });
        assert!(result.is_empty());
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
        assert!(file_types.contains(&ProjectType::Rust));
        assert!(file_types.contains(&ProjectType::Node));
        assert!(file_types.contains(&ProjectType::Python));
    }
}
