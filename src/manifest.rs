//! Single source of truth for the dependency-descriptor files Feluda understands.
//!
//! Both the scanner and `feluda watch` agree on "what counts as a dependency
//! file" through this module, so the two never drift apart. Manifest
//! recognition delegates to [`Language::from_file_name`] (the authority used by
//! the parser to discover project roots); lockfiles are recognised on top of
//! that here, because they are inputs to the analyzers rather than entry points.
//!
//! The scanner and the watcher differ only in *traversal*: the scanner scans a
//! project root and resolves workspaces inside each language analyzer, while
//! [`discover_dependency_files`] walks the whole tree (gitignore-aware) to build
//! the set of files to monitor.

use crate::languages::Language;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// What kind of dependency descriptor a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepFileKind {
    /// A manifest Feluda parses directly as a project entry point
    /// (e.g. `Cargo.toml`, `package.json`, `pom.xml`).
    Manifest,
    /// A resolved lockfile whose changes reflect dependency changes
    /// (e.g. `Cargo.lock`, `package-lock.json`, `go.sum`).
    Lock,
}

/// Lockfiles that affect resolved dependencies but are not themselves project
/// entry points recognised by [`Language::from_file_name`]. Lockfiles that are
/// already recognised there (e.g. `Pipfile.lock`, `renv.lock`) are intentionally
/// omitted to avoid double-counting.
const LOCK_FILES: &[&str] = &[
    "Cargo.lock",         // Rust
    "package-lock.json",  // npm
    "yarn.lock",          // Yarn
    "pnpm-lock.yaml",     // pnpm
    "go.sum",             // Go modules
    "go.work.sum",        // Go workspaces
    "uv.lock",            // Python (uv)
    "packages.lock.json", // .NET
];

/// Directory names that are pruned from both discovery and change detection.
/// These hold installed/vendored dependencies or VCS metadata and would
/// otherwise produce huge, noisy watch sets.
const PRUNED_DIRS: &[&str] = &["node_modules", ".git", "target", "vendor", ".venv", "venv"];

/// Classify a file name as a dependency descriptor, if it is one.
///
/// Manifest recognition is delegated to [`Language::from_file_name`] so there is
/// exactly one authority for it.
pub fn classify(file_name: &str) -> Option<DepFileKind> {
    if Language::from_file_name(file_name).is_some() {
        Some(DepFileKind::Manifest)
    } else if LOCK_FILES.contains(&file_name) {
        Some(DepFileKind::Lock)
    } else {
        None
    }
}

/// Whether a file name is any kind of dependency descriptor.
pub fn is_dependency_file(file_name: &str) -> bool {
    classify(file_name).is_some()
}

/// Whether `name` is a directory we prune from traversal/watching.
fn is_pruned_dir(name: &str) -> bool {
    PRUNED_DIRS.contains(&name)
}

/// Whether a filesystem change at `path` is a dependency change worth acting on.
///
/// Returns `true` only when the file itself is a dependency descriptor and none
/// of its ancestor directories are pruned (so a `package.json` deep inside
/// `node_modules/` is ignored).
pub fn is_relevant_change(path: &Path) -> bool {
    let pruned = path
        .components()
        .any(|component| component.as_os_str().to_str().is_some_and(is_pruned_dir));
    if pruned {
        return false;
    }

    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_dependency_file)
}

/// Recursively discover every dependency-descriptor file under `root`,
/// honouring `.gitignore`/`.ignore` and skipping vendored dependency
/// directories. This is the set `feluda watch` monitors.
pub fn discover_dependency_files(root: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut found = Vec::new();

    let walker = WalkBuilder::new(root.as_ref())
        .filter_entry(|entry| {
            // Prune known vendor/VCS directories from traversal entirely.
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                if let Some(name) = entry.file_name().to_str() {
                    return !is_pruned_dir(name);
                }
            }
            true
        })
        .build();

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            if is_dependency_file(name) {
                found.push(entry.into_path());
            }
        }
    }

    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn classifies_manifests_via_language_authority() {
        assert_eq!(classify("Cargo.toml"), Some(DepFileKind::Manifest));
        assert_eq!(classify("package.json"), Some(DepFileKind::Manifest));
        assert_eq!(classify("pom.xml"), Some(DepFileKind::Manifest));
        assert_eq!(classify("requirements.txt"), Some(DepFileKind::Manifest));
        assert_eq!(classify("MyApp.csproj"), Some(DepFileKind::Manifest));
    }

    #[test]
    fn classifies_lockfiles() {
        assert_eq!(classify("Cargo.lock"), Some(DepFileKind::Lock));
        assert_eq!(classify("package-lock.json"), Some(DepFileKind::Lock));
        assert_eq!(classify("go.sum"), Some(DepFileKind::Lock));
        assert_eq!(classify("uv.lock"), Some(DepFileKind::Lock));
    }

    #[test]
    fn ignores_unrelated_files() {
        assert_eq!(classify("README.md"), None);
        assert_eq!(classify("main.rs"), None);
        assert!(!is_dependency_file("LICENSE"));
    }

    #[test]
    fn change_under_pruned_dir_is_ignored() {
        assert!(is_relevant_change(Path::new("Cargo.toml")));
        assert!(is_relevant_change(Path::new("crates/foo/Cargo.toml")));
        assert!(!is_relevant_change(Path::new(
            "node_modules/foo/package.json"
        )));
        assert!(!is_relevant_change(Path::new("target/debug/Cargo.toml")));
        assert!(!is_relevant_change(Path::new(".git/config")));
    }

    #[test]
    fn discovers_dependency_files_recursively_and_prunes() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        fs::write(root.join("Cargo.lock"), "").unwrap();
        fs::create_dir_all(root.join("crates/sub")).unwrap();
        fs::write(root.join("crates/sub/Cargo.toml"), "[package]").unwrap();
        fs::create_dir_all(root.join("node_modules/dep")).unwrap();
        fs::write(root.join("node_modules/dep/package.json"), "{}").unwrap();
        fs::write(root.join("README.md"), "# hi").unwrap();

        let mut found: Vec<String> = discover_dependency_files(root)
            .into_iter()
            .filter_map(|p| {
                p.strip_prefix(root)
                    .ok()
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            })
            .collect();
        found.sort();

        assert_eq!(
            found,
            vec![
                "Cargo.lock".to_string(),
                "Cargo.toml".to_string(),
                "crates/sub/Cargo.toml".to_string(),
            ]
        );
    }
}
