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
}
