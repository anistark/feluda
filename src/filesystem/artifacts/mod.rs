//! Cataloging installed language artifacts, rather than the packages a distro shipped.
//!
//! The OS catalogers answer what the distribution installed. In a container that is a fraction of
//! what is there: the application's own dependencies arrive as installed artifacts with no manifest
//! behind them. An image built `FROM python:3.12` has around ninety dpkg packages and several
//! hundred distributions in `site-packages` that dpkg knows nothing about. Reporting only the
//! former would describe the base image and call it the application.
//!
//! So the same tree is walked for the metadata an installer leaves next to the code it installed:
//! Python's `dist-info` and `egg-info` directories, and the `package.json` inside every installed
//! `node_modules` entry. Ruby gemspecs, jar manifests and Go build info are the same idea and are
//! tracked separately.
//!
//! Two things keep the result honest. Anything the OS package manager already claims ownership of
//! is skipped, so Debian's `python3-yaml` and the PyYAML distribution it installs are one finding
//! rather than two. And an artifact whose metadata states no license goes to its registry, which is
//! something an OS package can never do: an installed distribution has real coordinates.

pub mod node;
pub mod python;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::cli::with_spinner;
use crate::debug::{log, LogLevel};
use crate::languages::resolve_license_for;
use crate::licenses::{LicenseCompatibility, LicenseInfo, OsiStatus};
use crate::purl::Ecosystem;

/// Top-level directories never descended into.
///
/// The first four are kernel-backed pseudo filesystems and volatile state. They hold no installed
/// software, `/proc` is effectively unbounded, and a scan of a live root rather than an extracted
/// one would otherwise spend its time there. `.git` is skipped because a checked out tree is not an
/// installed one.
const SKIP_DIRS: &[&str] = &["proc", "sys", "dev", "run", ".git"];

/// One installed artifact, as its metadata describes it.
///
/// Deliberately not a [`LicenseInfo`]: a cataloger's job is to say what the metadata contains, and
/// classification is the shared pipeline's.
pub struct Artifact {
    pub ecosystem: Ecosystem,
    pub name: String,
    pub version: String,
    pub license: Option<String>,
}

/// A cataloger: recognises its own metadata files and reads one.
struct Cataloger {
    /// What the artifacts are called, for the log.
    kind: &'static str,
    recognises: fn(&Path) -> bool,
    read: fn(&Path) -> Option<Artifact>,
}

/// The catalogers, in the order a path is offered to them.
const CATALOGERS: &[Cataloger] = &[
    Cataloger {
        kind: "Python distribution",
        recognises: python::is_metadata,
        read: python::read,
    },
    Cataloger {
        kind: "Node package",
        recognises: node::is_metadata,
        read: node::read,
    },
];

/// Whether a path is a metadata file some cataloger keys on.
///
/// The OS catalogers filter their file lists through this while they read them, so the only paths
/// they have to remember are the ones an artifact could be claimed from — a full root filesystem's
/// file list runs to hundreds of thousands of entries and none of the rest are ever asked about.
pub fn is_artifact_metadata(path: &Path) -> bool {
    cataloger_for(path).is_some()
}

/// The cataloger that claims a path, if any.
fn cataloger_for(path: &Path) -> Option<&'static Cataloger> {
    CATALOGERS
        .iter()
        .find(|cataloger| (cataloger.recognises)(path))
}

/// Catalog every installed language artifact under `root`.
///
/// `owned` holds the artifact metadata files the OS package managers claim, relative to `root`.
/// Anything in it is already reported as an OS package and is skipped here.
pub fn catalog(root: &Path, owned: &HashSet<PathBuf>) -> Vec<LicenseInfo> {
    let candidates = find_metadata(root, owned);
    log(
        LogLevel::Info,
        &format!("Found {} installed language artifacts", candidates.len()),
    );

    let artifacts: Vec<Artifact> = candidates
        .par_iter()
        .filter_map(|(path, cataloger)| (cataloger.read)(path))
        .collect();

    dedupe(artifacts)
}

/// Walk the tree for metadata files a cataloger claims and the OS does not.
fn find_metadata(root: &Path, owned: &HashSet<PathBuf>) -> Vec<(PathBuf, &'static Cataloger)> {
    // A root filesystem is not a repository: nothing here should honour a `.gitignore` that
    // happens to be in the tree, and dot directories hold installed software as readily as any
    // other. `standard_filters` turns all of that off.
    let walker = WalkBuilder::new(root)
        .standard_filters(false)
        .follow_links(false)
        .filter_entry(|entry| entry.depth() == 0 || !is_skipped(entry.path(), entry.depth()))
        .build();

    let mut candidates = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        let Some(cataloger) = cataloger_for(path) else {
            continue;
        };

        if path
            .strip_prefix(root)
            .is_ok_and(|relative| owned.contains(relative))
        {
            log(
                LogLevel::Info,
                &format!("Skipping {}: shipped by an OS package", path.display()),
            );
            continue;
        }

        log(
            LogLevel::Info,
            &format!("Found {} at {}", cataloger.kind, path.display()),
        );
        candidates.push((path.to_path_buf(), cataloger));
    }
    candidates
}

/// Whether a directory is one of the roots that holds no installed software.
///
/// Only at the top of the scanned tree: `proc` and `dev` mean something at the root of a filesystem
/// and nothing inside a package that happens to have directories by those names.
fn is_skipped(path: &Path, depth: usize) -> bool {
    depth == 1
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| SKIP_DIRS.contains(&name))
}

/// Turn the artifacts into findings, dropping the ones already accounted for.
///
/// One library can be installed in several places — a virtualenv beside the system interpreter, a
/// dependency hoisted into two `node_modules` trees — and each is the same package at the same
/// version with the same license. Reporting it once per install location would inflate every count
/// in the report without adding a fact to it.
fn dedupe(artifacts: Vec<Artifact>) -> Vec<LicenseInfo> {
    let mut seen: HashSet<(Ecosystem, String)> = HashSet::new();
    let mut findings = Vec::new();

    for artifact in artifacts {
        let identity = (
            artifact.ecosystem,
            format!("{}@{}", artifact.name, artifact.version),
        );
        if !seen.insert(identity) {
            log(
                LogLevel::Info,
                &format!(
                    "Skipping duplicate {} {} (installed more than once)",
                    artifact.name, artifact.version
                ),
            );
            continue;
        }

        findings.push(LicenseInfo {
            name: artifact.name,
            version: artifact.version,
            license: artifact.license,
            // Filled in by `classify_findings`; compatibility is the shared pipeline's job.
            is_restrictive: false,
            compatibility: LicenseCompatibility::Unknown,
            osi_status: OsiStatus::Unknown,
            ecosystem: artifact.ecosystem,
            sub_project: None,
        });
    }

    findings
}

/// Ask each package's registry about the ones whose metadata stated no license.
///
/// This is the one thing a filesystem scan can do for an installed artifact that it cannot do for
/// an OS package: a distribution in `site-packages` is a real PyPI release, so there is somewhere
/// to ask. `resolve_license_for` returns `None` for the OS ecosystems without touching the network,
/// so passing every finding through costs nothing.
pub fn resolve_missing_licenses(findings: &mut [LicenseInfo]) {
    let unresolved = findings
        .iter()
        .filter(|finding| finding.license.is_none())
        .count();
    if unresolved == 0 {
        return;
    }

    log(
        LogLevel::Info,
        &format!("Resolving licenses for {unresolved} artifacts whose metadata stated none"),
    );

    with_spinner(
        "🔍: licenses missing from installed metadata",
        |indicator| {
            let resolved: Vec<Option<String>> = findings
                .par_iter()
                .map(|finding| match finding.license {
                    Some(_) => None,
                    None => resolve_license_for(finding.ecosystem, &finding.name, &finding.version),
                })
                .collect();

            let found = resolved.iter().filter(|license| license.is_some()).count();
            for (finding, license) in findings.iter_mut().zip(resolved) {
                if let Some(license) = license {
                    finding.license = Some(license);
                }
            }
            indicator.update_progress(&format!("{found} of {unresolved} resolved"));
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// A tree holding one Python distribution and one Node package.
    fn installed_tree() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "usr/lib/python3.12/site-packages/requests-2.32.3.dist-info/METADATA",
            "Metadata-Version: 2.1\nName: requests\nVersion: 2.32.3\nLicense: Apache-2.0\n",
        );
        write(
            temp.path(),
            "srv/app/node_modules/lodash/package.json",
            r#"{"name":"lodash","version":"4.17.21","license":"MIT"}"#,
        );
        temp
    }

    #[test]
    fn test_catalogs_python_and_node_artifacts() {
        let temp = installed_tree();
        let findings = catalog(temp.path(), &HashSet::new());

        let requests = findings
            .iter()
            .find(|finding| finding.name == "requests")
            .expect("python distribution missing");
        assert_eq!(requests.version, "2.32.3");
        assert_eq!(requests.license.as_deref(), Some("Apache-2.0"));
        assert_eq!(requests.ecosystem, Ecosystem::Pypi);
        assert_eq!(requests.purl().as_deref(), Some("pkg:pypi/requests@2.32.3"));

        let lodash = findings
            .iter()
            .find(|finding| finding.name == "lodash")
            .expect("node package missing");
        assert_eq!(lodash.ecosystem, Ecosystem::Npm);
        assert_eq!(lodash.purl().as_deref(), Some("pkg:npm/lodash@4.17.21"));
    }

    #[test]
    fn test_an_os_owned_artifact_is_not_reported_again() {
        let temp = tempfile::tempdir().unwrap();
        let metadata = "usr/lib/python3/dist-packages/PyYAML-6.0.egg-info/PKG-INFO";
        write(
            temp.path(),
            metadata,
            "Metadata-Version: 2.1\nName: PyYAML\nVersion: 6.0\nLicense: MIT\n",
        );

        assert_eq!(catalog(temp.path(), &HashSet::new()).len(), 1);

        // dpkg's python3-yaml lists this exact file, and the deb is already in the report.
        let owned = HashSet::from([PathBuf::from(metadata)]);
        assert!(catalog(temp.path(), &owned).is_empty());
    }

    #[test]
    fn test_one_library_installed_twice_is_one_finding() {
        let temp = tempfile::tempdir().unwrap();
        for root in ["srv/api", "srv/worker"] {
            write(
                temp.path(),
                &format!("{root}/node_modules/lodash/package.json"),
                r#"{"name":"lodash","version":"4.17.21","license":"MIT"}"#,
            );
        }
        assert_eq!(catalog(temp.path(), &HashSet::new()).len(), 1);
    }

    #[test]
    fn test_two_versions_of_one_library_are_two_findings() {
        let temp = tempfile::tempdir().unwrap();
        for (root, version) in [("srv/api", "4.17.21"), ("srv/worker", "3.10.1")] {
            write(
                temp.path(),
                &format!("{root}/node_modules/lodash/package.json"),
                &format!(r#"{{"name":"lodash","version":"{version}","license":"MIT"}}"#),
            );
        }
        assert_eq!(catalog(temp.path(), &HashSet::new()).len(), 2);
    }

    #[test]
    fn test_a_tree_with_nothing_installed_yields_nothing() {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "srv/app/package.json", r#"{"name":"app"}"#);
        write(temp.path(), "etc/os-release", "ID=debian\n");
        assert!(catalog(temp.path(), &HashSet::new()).is_empty());
    }

    #[test]
    fn test_pseudo_filesystems_are_not_descended_into() {
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "proc/1/root/node_modules/ghost/package.json",
            r#"{"name":"ghost","version":"1.0.0","license":"MIT"}"#,
        );
        assert!(catalog(temp.path(), &HashSet::new()).is_empty());
    }

    #[test]
    fn test_only_metadata_paths_are_worth_remembering() {
        // What the OS catalogers filter their file lists through.
        assert!(is_artifact_metadata(Path::new(
            "usr/lib/python3/dist-packages/PyYAML-6.0.egg-info/PKG-INFO"
        )));
        assert!(is_artifact_metadata(Path::new(
            "srv/app/node_modules/lodash/package.json"
        )));
        assert!(!is_artifact_metadata(Path::new("usr/bin/python3")));
        assert!(!is_artifact_metadata(Path::new("srv/app/package.json")));
    }
}
