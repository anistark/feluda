//! Vendored and unmanaged dependency scanning.
//!
//! Every language analyzer starts from a manifest, so code that entered the tree without one is
//! invisible to them: libraries copied into `vendor/` or `third_party/`, amalgamated C sources,
//! snapshotted JS bundles. This module walks the project tree for exactly that code and reports
//! it as [`LicenseInfo`] entries alongside the dependency results — the directory's relative
//! path is the entry name and the version column carries [`VENDORED_MARKER`] or
//! [`UNMANAGED_MARKER`] — so every output mode and every filter applies to them unchanged.
//!
//! It is the directory-granularity companion to [`crate::source_scan`], which flags individual
//! own-source files bearing a foreign license header.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::debug::{log, LogLevel};
use crate::languages::Language;
use crate::licenses::{
    detect_license_in_dir, fetch_licenses_from_github, get_osi_status, is_license_ignored,
    is_license_restrictive, LicenseCompatibility, LicenseInfo, OsiStatus,
};

/// Marker placed in the version column of a package found inside a vendor directory.
pub const VENDORED_MARKER: &str = "vendored";

/// Marker placed in the version column of a licensed directory that no manifest accounts for.
pub const UNMANAGED_MARKER: &str = "unmanaged";

/// Directory names that conventionally hold copied-in third-party code.
const VENDOR_DIR_NAMES: &[&str] = &[
    "vendor",
    "vendored",
    "third_party",
    "third-party",
    "thirdparty",
    "3rdparty",
    "external",
    "externals",
    "extern",
    "deps",
];

/// Directory names never descended into: package-manager caches and build output. The dependency
/// analyzers already cover the former and the latter holds nothing worth attributing.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "venv",
    ".venv",
    "__pycache__",
    "site-packages",
    "bower_components",
    "Pods",
    "dist",
    "build",
    "out",
    "bin",
    "obj",
];

/// How far below a vendor directory a package may sit. Go's `go mod vendor` layout nests three
/// levels (`vendor/github.com/pkg/errors`); the extra level covers deeper module paths.
const MAX_VENDOR_DEPTH: usize = 4;

/// What kind of unrecorded code a directory holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FindingKind {
    /// A package directory inside a recognized vendor directory.
    Vendored,
    /// A licensed directory outside any vendor directory that no manifest accounts for.
    Unmanaged,
}

impl FindingKind {
    fn marker(self) -> &'static str {
        match self {
            FindingKind::Vendored => VENDORED_MARKER,
            FindingKind::Unmanaged => UNMANAGED_MARKER,
        }
    }
}

/// A directory holding code that no manifest records.
#[derive(Debug)]
struct Finding {
    /// Path relative to the project root — the reported entry name.
    path: PathBuf,
    kind: FindingKind,
    /// Resolved SPDX id, or `None` when nothing in the directory identifies a license.
    license: Option<String>,
}

/// Whether `name` is a conventional vendor directory.
fn is_vendor_dir_name(name: &str) -> bool {
    VENDOR_DIR_NAMES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name))
}

/// Whether the directory holds a manifest any language analyzer would recognize.
fn has_manifest(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_type().is_ok_and(|ft| ft.is_file())
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| Language::from_file_name(name).is_some())
    })
}

/// Whether the directory holds any regular file of its own.
///
/// Inside a vendor directory this separates a package from a path segment on the way to one:
/// `vendor/github.com/pkg/errors` holds `.go` files, while `vendor/github.com` holds only
/// subdirectories. Dotfiles don't count — `.gitkeep` in an otherwise empty segment is not code.
fn contains_files(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_type().is_ok_and(|ft| ft.is_file())
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| !name.starts_with('.'))
    })
}

/// Number of path components between `ancestor` and `path`, or `None` if unrelated.
fn depth_below(path: &Path, ancestor: &Path) -> Option<usize> {
    path.strip_prefix(ancestor)
        .ok()
        .map(|rel| rel.components().count())
}

/// The nearest ancestor of `path` (excluding `path` itself) that is a vendor directory.
///
/// Bounded by `root` so a `vendor` component in the path *above* the scanned project — a
/// checkout living in `~/vendor/myapp`, say — never turns the whole project into vendored code.
fn enclosing_vendor_dir(path: &Path, root: &Path) -> Option<PathBuf> {
    let mut current = path.parent()?;
    let mut nearest = None;
    while current.starts_with(root) {
        if current != root
            && current
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_vendor_dir_name)
        {
            // Keep walking up but remember the closest match found so far.
            if nearest.is_none() {
                nearest = Some(current.to_path_buf());
            }
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    nearest
}

/// The name a vendored package would carry in a manifest, used to suppress duplicates.
///
/// `go mod vendor` copies dependencies that `go.mod` already lists, so `vendor/github.com/pkg/errors`
/// is normally reported by the Go analyzer too. Both the module-path form
/// (`github.com/pkg/errors`) and the bare directory name (`errors`) are candidates, since
/// ecosystems differ in which one lands in the manifest.
fn dependency_name_candidates(path: &Path, vendor_root: &Path) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Ok(rel) = path.strip_prefix(vendor_root) {
        let joined = rel
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/");
        if !joined.is_empty() {
            candidates.push(joined);
        }
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        candidates.push(name.to_string());
    }
    candidates
}

/// Walk the project tree and return every directory holding code no manifest records.
///
/// The walk honours `.gitignore`, skips hidden entries, and never descends into [`SKIP_DIRS`].
/// Because `ignore` yields directories before their contents, recording a package and then
/// skipping anything beneath it reports each vendored library once, at its own root, rather than
/// once per nested subdirectory.
fn collect_findings(
    root: &Path,
    known_dependencies: &[String],
    project_license: Option<&str>,
) -> Vec<Finding> {
    let known: Vec<String> = known_dependencies
        .iter()
        .map(|name| name.to_lowercase())
        .collect();

    let walker = WalkBuilder::new(root)
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|entry| {
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            !(is_dir
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| SKIP_DIRS.contains(&name)))
        })
        .build();

    let mut findings: Vec<Finding> = Vec::new();
    let mut recorded: Vec<PathBuf> = Vec::new();

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        let path = entry.path();
        if path == root {
            continue;
        }
        // Everything under an already-reported package belongs to that package.
        if recorded.iter().any(|parent| path.starts_with(parent)) {
            continue;
        }
        // A vendor directory is a container, not a package; keep descending into it.
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(is_vendor_dir_name)
        {
            continue;
        }

        let (kind, license) = match enclosing_vendor_dir(path, root) {
            Some(vendor_root) => {
                if depth_below(path, &vendor_root).is_some_and(|d| d > MAX_VENDOR_DEPTH) {
                    continue;
                }
                let license = detect_license_in_dir(path);
                // A path segment on the way to a package (`vendor/github.com`) holds no files
                // of its own — keep descending rather than reporting it.
                if license.is_none() && !contains_files(path) {
                    continue;
                }
                if dependency_name_candidates(path, &vendor_root)
                    .iter()
                    .any(|candidate| known.contains(&candidate.to_lowercase()))
                {
                    log(
                        LogLevel::Info,
                        &format!(
                            "Skipping vendored {} — already reported by a manifest",
                            path.display()
                        ),
                    );
                    recorded.push(path.to_path_buf());
                    continue;
                }
                (FindingKind::Vendored, license)
            }
            None => {
                // Outside a vendor directory only a stray license file is evidence of foreign
                // code; a manifest here means the directory is a project of the user's own.
                if has_manifest(path) {
                    continue;
                }
                match detect_license_in_dir(path) {
                    // A copy of the project's own license is how a repo ships a
                    // sub-component (a skill, a plugin, an example), not foreign code.
                    Some(license)
                        if project_license.is_some_and(|proj| {
                            proj.trim().eq_ignore_ascii_case(license.trim())
                        }) =>
                    {
                        continue
                    }
                    Some(license) => (FindingKind::Unmanaged, Some(license)),
                    None => continue,
                }
            }
        };

        if is_license_ignored(license.as_deref()) {
            recorded.push(path.to_path_buf());
            continue;
        }

        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        log(
            LogLevel::Warn,
            &format!(
                "Unrecorded {} dependency at {} (license: {})",
                kind.marker(),
                rel.display(),
                license.as_deref().unwrap_or("unknown")
            ),
        );
        recorded.push(path.to_path_buf());
        findings.push(Finding {
            path: rel,
            kind,
            license,
        });
    }

    findings
}

/// Scan the project tree for vendored and otherwise unmanaged dependencies and return them as
/// [`LicenseInfo`] entries ready to be appended to the dependency report.
///
/// `known_dependencies` are the names the language analyzers already reported; a vendored
/// directory matching one of them is suppressed so `go mod vendor` trees are not reported twice.
/// `project_license` suppresses stray license files that merely restate the project's own license.
///
/// Compatibility is left [`LicenseCompatibility::Unknown`]; the caller's compatibility
/// annotation pass fills it in exactly as it does for dependencies. The license registry is
/// fetched only when at least one finding exists, so clean projects pay nothing.
pub fn scan_vendored_packages(
    root: &Path,
    known_dependencies: &[String],
    project_license: Option<&str>,
    strict: bool,
) -> Vec<LicenseInfo> {
    let findings = collect_findings(root, known_dependencies, project_license);
    if findings.is_empty() {
        return Vec::new();
    }

    let known_licenses = fetch_licenses_from_github().unwrap_or_else(|e| {
        log(
            LogLevel::Warn,
            &format!("Failed to fetch license registry for vendored scan: {e}"),
        );
        HashMap::new()
    });

    findings
        .into_iter()
        .map(|finding| {
            let osi_status = match &finding.license {
                Some(license) => get_osi_status(license),
                None => OsiStatus::Unknown,
            };
            let is_restrictive = is_license_restrictive(&finding.license, &known_licenses, strict);
            LicenseInfo {
                name: finding.path.display().to_string(),
                version: finding.kind.marker().to_string(),
                license: finding.license,
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
                osi_status,
                sub_project: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const MIT_TEXT: &str = "MIT License\n\nPermission is hereby granted, free of charge, to any \
person obtaining a copy of this software and associated documentation files.\n";

    const GPL3_TEXT: &str = "GNU GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007\n";

    fn write_license(dir: &Path, text: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("LICENSE"), text).unwrap();
    }

    fn names(findings: &[Finding]) -> Vec<String> {
        findings
            .iter()
            .map(|f| f.path.display().to_string().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn test_flags_vendored_package_with_license() {
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("vendor").join("leftpad"), MIT_TEXT);

        let findings = collect_findings(dir.path(), &[], None);
        assert_eq!(names(&findings), vec!["vendor/leftpad"]);
        assert_eq!(findings[0].kind, FindingKind::Vendored);
        assert_eq!(findings[0].license.as_deref(), Some("MIT"));
    }

    #[test]
    fn test_flags_go_style_nested_vendor_layout() {
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("vendor/github.com/pkg/errors"), GPL3_TEXT);

        let findings = collect_findings(dir.path(), &[], None);
        assert_eq!(names(&findings), vec!["vendor/github.com/pkg/errors"]);
        assert_eq!(findings[0].license.as_deref(), Some("GPL-3.0"));
    }

    #[test]
    fn test_vendored_package_without_license_is_still_reported() {
        let dir = tempfile::TempDir::new().unwrap();
        let pkg = dir.path().join("third_party").join("sqlite");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("Makefile"), "all:\n\techo hi\n").unwrap();

        let findings = collect_findings(dir.path(), &[], None);
        assert_eq!(names(&findings), vec!["third_party/sqlite"]);
        assert!(findings[0].license.is_none());
    }

    #[test]
    fn test_vendored_source_without_license_or_manifest_is_reported() {
        // The highest-risk case: code copied in with no attribution whatsoever.
        let dir = tempfile::TempDir::new().unwrap();
        let pkg = dir.path().join("vendor/github.com/pkg/errors");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("errors.go"), "package errors\n").unwrap();

        let findings = collect_findings(dir.path(), &[], None);
        assert_eq!(names(&findings), vec!["vendor/github.com/pkg/errors"]);
        assert!(findings[0].license.is_none());
    }

    #[test]
    fn test_package_is_reported_once_not_per_subdirectory() {
        let dir = tempfile::TempDir::new().unwrap();
        let pkg = dir.path().join("vendor").join("libfoo");
        write_license(&pkg, MIT_TEXT);
        write_license(&pkg.join("src"), MIT_TEXT);

        let findings = collect_findings(dir.path(), &[], None);
        assert_eq!(names(&findings), vec!["vendor/libfoo"]);
    }

    #[test]
    fn test_skips_vendored_package_already_in_a_manifest() {
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("vendor/github.com/pkg/errors"), MIT_TEXT);

        let findings = collect_findings(dir.path(), &["github.com/pkg/errors".to_string()], None);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_skips_vendored_package_matched_by_bare_name() {
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("vendor").join("leftpad"), MIT_TEXT);

        let findings = collect_findings(dir.path(), &["LeftPad".to_string()], None);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_flags_unmanaged_license_directory_outside_vendor() {
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("scripts").join("snippet"), GPL3_TEXT);

        let findings = collect_findings(dir.path(), &[], None);
        assert_eq!(names(&findings), vec!["scripts/snippet"]);
        assert_eq!(findings[0].kind, FindingKind::Unmanaged);
        assert_eq!(findings[0].license.as_deref(), Some("GPL-3.0"));
    }

    #[test]
    fn test_unmanaged_directory_carrying_the_project_license_is_not_a_finding() {
        // Repos ship sub-components (skills, plugins, examples) with a copy of their own
        // LICENSE; that is not foreign code.
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("skills").join("mytool"), MIT_TEXT);

        assert!(collect_findings(dir.path(), &[], Some("MIT")).is_empty());
        assert_eq!(
            names(&collect_findings(dir.path(), &[], Some("GPL-3.0"))),
            vec!["skills/mytool"]
        );
    }

    #[test]
    fn test_vendored_package_is_reported_even_under_the_project_license() {
        // Unlike a stray license file, code under vendor/ is third-party regardless of which
        // license it carries — it still needs attribution.
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("vendor").join("leftpad"), MIT_TEXT);

        let findings = collect_findings(dir.path(), &[], Some("MIT"));
        assert_eq!(names(&findings), vec!["vendor/leftpad"]);
    }

    #[test]
    fn test_directory_with_manifest_is_a_project_not_a_finding() {
        let dir = tempfile::TempDir::new().unwrap();
        let member = dir.path().join("crates").join("core");
        write_license(&member, MIT_TEXT);
        fs::write(member.join("Cargo.toml"), "[package]\nname = \"core\"\n").unwrap();

        let findings = collect_findings(dir.path(), &[], None);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_project_root_license_is_not_a_finding() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("LICENSE"), MIT_TEXT).unwrap();

        assert!(collect_findings(dir.path(), &[], None).is_empty());
    }

    #[test]
    fn test_skips_package_manager_directories() {
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("node_modules").join("leftpad"), GPL3_TEXT);
        write_license(&dir.path().join("target").join("debug"), GPL3_TEXT);

        assert!(collect_findings(dir.path(), &[], None).is_empty());
    }

    #[test]
    fn test_unlicensed_plain_directory_is_clean() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();

        assert!(collect_findings(dir.path(), &[], None).is_empty());
    }

    #[test]
    fn test_findings_are_deterministic() {
        let dir = tempfile::TempDir::new().unwrap();
        for name in ["c-lib", "a-lib", "b-lib"] {
            write_license(&dir.path().join("vendor").join(name), MIT_TEXT);
        }

        assert_eq!(
            names(&collect_findings(dir.path(), &[], None)),
            vec!["vendor/a-lib", "vendor/b-lib", "vendor/c-lib"]
        );
    }

    #[test]
    fn test_scan_produces_license_info_entries() {
        let dir = tempfile::TempDir::new().unwrap();
        write_license(&dir.path().join("vendor").join("gpl-lib"), GPL3_TEXT);

        let results = scan_vendored_packages(dir.path(), &[], None, false);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].version, VENDORED_MARKER);
        assert_eq!(results[0].license.as_deref(), Some("GPL-3.0"));
        assert_eq!(results[0].compatibility, LicenseCompatibility::Unknown);
    }

    #[test]
    fn test_scan_of_clean_project_returns_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();

        assert!(scan_vendored_packages(dir.path(), &[], None, false).is_empty());
    }

    #[test]
    fn test_enclosing_vendor_dir_ignores_components_above_root() {
        let base = tempfile::TempDir::new().unwrap();
        let root = base.path().join("vendor").join("myapp");
        let nested = root.join("src");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(enclosing_vendor_dir(&nested, &root), None);
    }
}
