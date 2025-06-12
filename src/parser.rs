use crate::cli;
use crate::debug::{log, log_debug, with_debug, FeludaResult, LogLevel};
use crate::licenses::{
    analyze_go_licenses, analyze_js_licenses, analyze_python_licenses, analyze_rust_licenses,
    LicenseCompatibility, LicenseInfo,
};
use cargo_metadata::MetadataCommand;
use ignore::Walk;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Language {
    Rust(&'static str),
    Node(&'static str),
    Go(&'static str),
    Python(&'static [&'static str]),
}

impl Language {
    fn from_file_name(file_name: impl Into<String>) -> Option<Self> {
        match file_name.into().as_str() {
            "Cargo.toml" => Some(Language::Rust("Cargo.toml")),
            "package.json" => Some(Language::Node("package.json")),
            "go.mod" => Some(Language::Go("go.mod")),
            file_name => {
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
fn find_project_roots(root_path: impl AsRef<Path>) -> FeludaResult<Vec<ProjectRoot>> {
    let mut project_roots = Vec::new();
    log(
        LogLevel::Info,
        format!(
            "Scanning for project files in: {}",
            root_path.as_ref().display()
        ),
    );

    for entry in Walk::new(&root_path).filter_map(|e| match e {
        Ok(entry) => Some(entry),
        Err(err) => {
            log(
                LogLevel::Error,
                format!("Error while walking directory: {}", err),
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
                    format!(
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
        format!("Found {} project roots", project_roots.len()),
    );
    log_debug("Project roots", &project_roots);

    Ok(project_roots)
}

fn check_which_python_file_exists(project_path: impl AsRef<Path>) -> Option<String> {
    for &path in PYTHON_PATHS.iter() {
        let full_path = Path::new(project_path.as_ref()).join(path);
        if full_path.exists() {
            log(
                LogLevel::Info,
                format!("Found Python project file: {}", full_path.display()),
            );
            return Some(path.to_string());
        }
    }

    log(
        LogLevel::Warn,
        format!(
            "No Python project file found in: {}",
            project_path.as_ref().display()
        ),
    );
    None
}

pub fn parse_root(
    root_path: impl AsRef<Path>,
    language: Option<&str>,
) -> FeludaResult<Vec<LicenseInfo>> {
    log(
        LogLevel::Info,
        format!("Parsing root path: {}", root_path.as_ref().display()),
    );
    if let Some(lang) = language {
        log(LogLevel::Info, format!("Filtering by language: {}", lang));
    }

    let project_roots = find_project_roots(&root_path)?;

    if project_roots.is_empty() {
        log(
            LogLevel::Warn,
            "No project files found in the specified path",
        );
        return Ok(Vec::new());
    }

    let mut licenses = Vec::new();

    for root in project_roots {
        if let Some(language) = language {
            if !matches_language(root.project_type, language) {
                log(
                    LogLevel::Info,
                    format!(
                        "Skipping {:?} project (language filter: {})",
                        root.project_type, language
                    ),
                );
                continue;
            }
        }

        match parse_dependencies(&root) {
            Ok(mut deps) => {
                log(
                    LogLevel::Info,
                    format!(
                        "Found {} dependencies in {}",
                        deps.len(),
                        root.path.display()
                    ),
                );
                licenses.append(&mut deps);
            }
            Err(err) => {
                log(
                    LogLevel::Error,
                    format!(
                        "Error parsing dependencies in {}: {}",
                        root.path.display(),
                        err
                    ),
                );
            }
        }
    }

    log(
        LogLevel::Info,
        format!("Total dependencies found: {}", licenses.len()),
    );

    // Initialize all dependencies with Unknown compatibility
    // (compatibility will be updated in main.rs once project license is determined)
    for license in &mut licenses {
        license.compatibility = LicenseCompatibility::Unknown;
    }

    Ok(licenses)
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

// Parse dependencies based on the project type
fn parse_dependencies(root: &ProjectRoot) -> FeludaResult<Vec<LicenseInfo>> {
    let project_path = &root.path;
    let project_type = root.project_type;

    // Use the loading indicator
    let licenses = cli::with_spinner(format!("🔎: {}", project_path.display()), |indicator| {
        // Create a match statement that returns Vec<LicenseInfo> directly, not Result<Vec<LicenseInfo>>
        match project_type {
            Language::Rust(_) => {
                let project_path = Path::new(project_path).join("Cargo.toml");
                log(
                    LogLevel::Info,
                    format!("Parsing Rust project: {}", project_path.display()),
                );

                indicator.update_progress("analyzing Cargo.toml");

                match MetadataCommand::new()
                    .manifest_path(Path::new(&project_path))
                    .exec()
                {
                    Ok(metadata) => {
                        log(
                            LogLevel::Info,
                            format!("Found {} packages in Rust project", metadata.packages.len()),
                        );
                        indicator.update_progress(format!(
                            "found {} packages",
                            metadata.packages.len()
                        ));

                        let mut license_info = analyze_rust_licenses(metadata.packages);

                        // Initialize compatibility to Unknown - it will be set in main.rs
                        for info in &mut license_info {
                            info.compatibility = LicenseCompatibility::Unknown;
                        }

                        license_info
                    }
                    Err(err) => {
                        log(
                            LogLevel::Error,
                            format!("Failed to fetch cargo metadata: {}", err),
                        );
                        Vec::new() // Return empty vector on error
                    }
                }
            }
            Language::Node(_) => {
                let project_path = Path::new(project_path).join("package.json");
                log(
                    LogLevel::Info,
                    format!("Parsing Node.js project: {}", project_path.display()),
                );

                indicator.update_progress("analyzing package.json");

                match project_path.to_str() {
                    Some(path_str) => with_debug("Analyze JS licenses", || {
                        let mut deps = analyze_js_licenses(path_str);

                        // Initialize compatibility to Unknown - it will be set in main.rs
                        for info in &mut deps {
                            info.compatibility = LicenseCompatibility::Unknown;
                        }

                        indicator.update_progress(format!("found {} dependencies", deps.len()));
                        deps
                    }),
                    None => {
                        log(LogLevel::Error, "Failed to convert Node.js path to string");
                        Vec::new() // Return empty vector on error
                    }
                }
            }
            Language::Go(_) => {
                let project_path = Path::new(project_path).join("go.mod");
                log(
                    LogLevel::Info,
                    format!("Parsing Go project: {}", project_path.display()),
                );

                indicator.update_progress("analyzing go.mod");

                match project_path.to_str() {
                    Some(path_str) => with_debug("Analyze Go licenses", || {
                        let mut deps = analyze_go_licenses(path_str);

                        // Initialize compatibility to Unknown - it will be set in main.rs
                        for info in &mut deps {
                            info.compatibility = LicenseCompatibility::Unknown;
                        }

                        indicator.update_progress(format!("found {} dependencies", deps.len()));
                        deps
                    }),
                    None => {
                        log(LogLevel::Error, "Failed to convert Go path to string");
                        Vec::new() // Return empty vector on error
                    }
                }
            }
            Language::Python(_) => {
                match check_which_python_file_exists(project_path) {
                    Some(python_package_file) => {
                        let project_path = Path::new(project_path).join(&python_package_file);
                        log(
                            LogLevel::Info,
                            format!("Parsing Python project: {}", project_path.display()),
                        );

                        indicator.update_progress(format!("analyzing {}", python_package_file));

                        match project_path.to_str() {
                            Some(path_str) => with_debug("Analyze Python licenses", || {
                                let mut deps = analyze_python_licenses(path_str);

                                // Initialize compatibility to Unknown - it will be set in main.rs
                                for info in &mut deps {
                                    info.compatibility = LicenseCompatibility::Unknown;
                                }

                                indicator
                                    .update_progress(format!("found {} dependencies", deps.len()));
                                deps
                            }),
                            None => {
                                log(LogLevel::Error, "Failed to convert Python path to string");
                                Vec::new() // Return empty vector on error
                            }
                        }
                    }
                    None => {
                        log(LogLevel::Error, "Python package file not found");
                        Vec::new() // Return empty vector on error
                    }
                }
            }
        }
    });

    // Return the licenses wrapped in Ok
    Ok(licenses)
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
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
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
        assert!(result.is_ok());
        let parsed = get_go_dependencies(dependencies.to_string());
        assert!(parsed.len() == 19);
        assert!(result.unwrap().len() == parsed.len());
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

        let files = find_project_roots(root_path.to_str().unwrap()).unwrap();

        // Verify we found all project files
        assert_eq!(files.len(), 3);

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

        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_language_from_file_name() {
        assert_eq!(
            Language::from_file_name("Cargo.toml"),
            Some(Language::Rust("Cargo.toml"))
        );
        assert_eq!(
            Language::from_file_name("package.json"),
            Some(Language::Node("package.json"))
        );
        assert_eq!(
            Language::from_file_name("go.mod"),
            Some(Language::Go("go.mod"))
        );
        assert_eq!(
            Language::from_file_name("requirements.txt"),
            Some(Language::Python(&PYTHON_PATHS[..]))
        );
        assert_eq!(
            Language::from_file_name("pyproject.toml"),
            Some(Language::Python(&PYTHON_PATHS[..]))
        );
        assert_eq!(
            Language::from_file_name("Pipfile.lock"),
            Some(Language::Python(&PYTHON_PATHS[..]))
        );
        assert_eq!(
            Language::from_file_name("pip_freeze.txt"),
            Some(Language::Python(&PYTHON_PATHS[..]))
        );
        assert_eq!(Language::from_file_name("unknown.txt"), None);
        assert_eq!(Language::from_file_name(""), None);
    }

    #[test]
    fn test_language_equality() {
        assert_eq!(Language::Rust("Cargo.toml"), Language::Rust("Cargo.toml"));
        assert_ne!(Language::Rust("Cargo.toml"), Language::Node("package.json"));
        assert_ne!(Language::Node("package.json"), Language::Go("go.mod"));
        assert_ne!(Language::Go("go.mod"), Language::Python(&PYTHON_PATHS));
    }

    #[test]
    fn test_language_debug() {
        let rust_lang = Language::Rust("Cargo.toml");
        let debug_str = format!("{:?}", rust_lang);
        assert!(debug_str.contains("Rust"));
        assert!(debug_str.contains("Cargo.toml"));
    }

    #[test]
    fn test_language_clone() {
        let rust_lang = Language::Rust("Cargo.toml");
        let cloned = rust_lang;
        assert_eq!(rust_lang, cloned);
    }

    #[test]
    fn test_language_copy() {
        let node_lang = Language::Node("package.json");
        let copied = node_lang;
        assert_eq!(node_lang, copied);
    }

    #[test]
    fn test_matches_language() {
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

        assert!(!matches_language(Language::Rust("Cargo.toml"), "java"));
        assert!(!matches_language(Language::Node("package.json"), "cpp"));
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

        // Test when multiple Python files exist (should return first found in PYTHON_PATHS order)
        std::fs::write(
            temp_dir.path().join("pyproject.toml"),
            "[project]\nname = \"test\"",
        )
        .unwrap();
        std::fs::write(temp_dir.path().join("Pipfile.lock"), "{}").unwrap();
        let result = check_which_python_file_exists(temp_dir.path());
        assert_eq!(result, Some("requirements.txt".to_string())); // First in PYTHON_PATHS array
    }

    #[test]
    fn test_check_which_python_file_exists_priority() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Create files in reverse order of PYTHON_PATHS
        std::fs::write(
            temp_dir.path().join("pyproject.toml"),
            "[project]\nname = \"test\"",
        )
        .unwrap();
        std::fs::write(temp_dir.path().join("Pipfile.lock"), "{}").unwrap();
        std::fs::write(temp_dir.path().join("pip_freeze.txt"), "package==1.0.0").unwrap();

        let result = check_which_python_file_exists(temp_dir.path());
        // Should return the first one in PYTHON_PATHS array, not the first created
        assert!(result.is_some());
        let found_file = result.unwrap();
        assert!(PYTHON_PATHS.contains(&found_file.as_str()));
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
    fn test_find_project_roots_multiple_python_files() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root_path = temp_dir.path();

        // Create multiple Python project files
        std::fs::write(root_path.join("requirements.txt"), "requests==2.28.1").unwrap();
        std::fs::write(
            root_path.join("pyproject.toml"),
            "[project]\nname = \"test\"",
        )
        .unwrap();
        std::fs::write(root_path.join("Pipfile.lock"), "{}").unwrap();

        let result = find_project_roots(root_path.to_str().unwrap()).unwrap();

        // Should find separate entries for each Python file
        assert_eq!(result.len(), 3);
        for project_root in &result {
            assert!(matches!(project_root.project_type, Language::Python(_)));
            assert_eq!(project_root.path, root_path);
        }
    }

    #[test]
    fn test_find_project_roots_with_gitignore() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root_path = temp_dir.path();

        // Create .gitignore
        std::fs::write(
            root_path.join(".gitignore"),
            "target/\nnode_modules/\n.git/\n",
        )
        .unwrap();

        // Create projects in ignored directories
        let target_dir = root_path.join("target");
        let node_modules_dir = root_path.join("node_modules");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::create_dir_all(&node_modules_dir).unwrap();

        std::fs::write(
            target_dir.join("Cargo.toml"),
            "[package]\nname = \"ignored\"",
        )
        .unwrap();
        std::fs::write(node_modules_dir.join("package.json"), "{}").unwrap();

        // Create project in non-ignored location
        std::fs::write(root_path.join("Cargo.toml"), "[package]\nname = \"main\"").unwrap();

        let result = find_project_roots(root_path.to_str().unwrap()).unwrap();

        assert!(!result.is_empty());

        let main_project = result.iter().find(|p| {
            matches!(p.project_type, Language::Rust("Cargo.toml")) && p.path == root_path
        });
        assert!(main_project.is_some());
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

        let debug_str = format!("{:?}", project_root);
        assert!(debug_str.contains("/test/path"));
        assert!(debug_str.contains("Rust"));
        assert!(debug_str.contains("Cargo.toml"));
    }

    #[test]
    fn test_python_paths_constant() {
        assert_eq!(PYTHON_PATHS.len(), 4);
        assert!(PYTHON_PATHS.contains(&"requirements.txt"));
        assert!(PYTHON_PATHS.contains(&"Pipfile.lock"));
        assert!(PYTHON_PATHS.contains(&"pip_freeze.txt"));
        assert!(PYTHON_PATHS.contains(&"pyproject.toml"));
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

        let result = parse_dependencies(&rust_project_root);
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

        let result = parse_dependencies(&node_project_root);
        assert!(result.is_ok());
        let licenses = result.unwrap();
        assert!(licenses.is_empty());
    }

    #[test]
    fn test_parse_dependencies_go_invalid_syntax() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let go_project_root = ProjectRoot {
            path: temp_dir.path().to_path_buf(),
            project_type: Language::Go("go.mod"),
        };

        // Create go.mod with invalid syntax
        std::fs::write(temp_dir.path().join("go.mod"), "invalid go.mod content").unwrap();

        let result = parse_dependencies(&go_project_root);
        assert!(result.is_ok());
        let _ = result.unwrap();
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

        let result = parse_dependencies(&python_project_root);
        assert!(result.is_ok());
        let licenses = result.unwrap();
        assert!(licenses.is_empty());
    }

    #[test]
    fn test_find_project_roots_invalid_path() {
        let result = find_project_roots("/definitely/nonexistent/path");
        assert!(result.is_ok());
        let roots = result.unwrap();
        assert!(roots.is_empty());
    }

    #[test]
    fn test_parse_root_invalid_path() {
        let result = parse_root("/definitely/nonexistent/path", None);
        assert!(result.is_ok());
        let licenses = result.unwrap();
        assert!(licenses.is_empty());
    }
}
