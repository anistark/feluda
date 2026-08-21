//! Cataloging what a filesystem has installed, rather than what a project declares.
//!
//! Every other scan source answers "what did someone write down?" — a manifest, or an SBOM another
//! tool produced. This one answers "what is actually on this disk?", by reading the databases the
//! system's own package managers keep. That makes a shipped artifact analyzable directly:
//!
//! ```sh
//! docker export app | tar -x -C rootfs
//! feluda --filesystem rootfs --fail-on-restrictive
//! ```
//!
//! The same path works on an extracted image layer, a chroot, or a mounted disk. No image handling
//! and, for the OS packages, no network either: their licenses are already in the tree.
//!
//! Two questions get asked of the tree, because a distro's package database answers only half of
//! it. [`apk`] and [`dpkg`] report what the distribution installed; [`artifacts`] reports what
//! landed alongside it with no manifest behind it — the distributions in `site-packages`, the
//! packages in `node_modules`. In a container the second is usually the larger half, and it is the
//! half that is the application rather than the base image.
//!
//! Alpine and Debian are covered because their databases are text. RPM is a binary store, and
//! [`rpm`] reads the sqlite backend every in-support RPM distribution uses; the two older backends
//! are reported by name rather than read.

pub mod apk;
pub mod artifacts;
pub mod copyright;
pub mod deb822;
pub mod dpkg;
pub mod rpm;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::cli::with_spinner;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::licenses::{classify_findings, LicenseCompatibility, LicenseInfo, OsiStatus};
use crate::purl::Ecosystem;

/// Where a distribution records its identity, in preference order. The second is the fallback for
/// images that ship the file only under `/usr/lib`.
const OS_RELEASE_PATHS: &[&str] = &["etc/os-release", "usr/lib/os-release"];

/// What one package manager's database says about a filesystem.
#[derive(Debug, Default)]
pub struct Catalog {
    /// One finding per installed package.
    pub packages: Vec<LicenseInfo>,
    /// The installed artifact metadata files the manager claims, relative to the scan root.
    ///
    /// Filtered to what [`artifacts`] keys on as the database is read, so a root filesystem's whole
    /// file list — hundreds of thousands of entries — never has to be held. It is what keeps a
    /// distro's Python package and the distribution it installs from being reported as two things.
    pub owned: HashSet<PathBuf>,
}

/// A package manager's cataloger: reads a root filesystem, returns `Ok(None)` when its database is
/// not there.
type Cataloger = fn(&Path, Option<&str>) -> FeludaResult<Option<Catalog>>;

/// The catalogers, in the order they are tried. All of them run: an image can carry more than one
/// package manager's database, and reporting only the first would hide the rest.
const CATALOGERS: &[(&str, &str, Cataloger)] = &[
    ("apk", apk::DATABASE_PATH, apk::catalog),
    ("dpkg", dpkg::DATABASE_PATH, dpkg::catalog),
    ("rpm", rpm::DATABASE_PATH, rpm::catalog),
];

/// Catalog everything installed under `root`: the distro's own packages, then the language
/// artifacts that arrived alongside them.
///
/// Fails when the tree holds neither. An empty report would be indistinguishable from a clean one,
/// and a mistyped path should not read as a pass. A tree that holds only artifacts is fine — an
/// installation directory like `/opt/app` has no package database and is still worth scanning.
pub fn scan_filesystem(root: &Path, strict: bool) -> FeludaResult<Vec<LicenseInfo>> {
    if !root.is_dir() {
        return Err(source_error(format!(
            "Not a directory: {}. --filesystem takes a root filesystem or an installation tree.",
            root.display()
        )));
    }

    let namespace = distro_namespace(root);
    log(
        LogLevel::Info,
        &format!(
            "Scanning filesystem at {} (distro: {})",
            root.display(),
            namespace.as_deref().unwrap_or("unknown")
        ),
    );

    let mut findings: Vec<LicenseInfo> = Vec::new();
    let mut owned: HashSet<PathBuf> = HashSet::new();
    let mut cataloged: Vec<String> = Vec::new();

    for (manager, database, catalog) in CATALOGERS {
        let found = with_spinner(&format!("📦: {manager} packages"), |indicator| {
            let found = catalog(root, namespace.as_deref());
            let count = found.as_ref().map(|found| match found {
                Some(catalog) => catalog.packages.len(),
                None => 0,
            });
            indicator.update_progress(&match count {
                Ok(count) => format!("{count} package{}", if count == 1 { "" } else { "s" }),
                Err(_) => "unreadable".to_string(),
            });
            found
        })?;

        if let Some(catalog) = found {
            cataloged.push((*manager).to_string());
            findings.extend(catalog.packages);
            owned.extend(catalog.owned);
        } else {
            log(
                LogLevel::Info,
                &format!("No {manager} database at {database}"),
            );
        }
    }

    // Second, everything installed that no package manager recorded. This runs after the OS
    // catalogers because what they claim ownership of is what it has to skip.
    let installed = with_spinner("📦: installed language artifacts", |indicator| {
        let installed = artifacts::catalog(root, &owned);
        indicator.update_progress(&format!(
            "{} artifact{}",
            installed.len(),
            if installed.len() == 1 { "" } else { "s" }
        ));
        installed
    });
    if !installed.is_empty() {
        cataloged.push(format!("{} installed artifacts", installed.len()));
        findings.extend(installed);
    }

    if cataloged.is_empty() {
        let looked_for = CATALOGERS
            .iter()
            .map(|(manager, database, _)| format!("/{database} ({manager})"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(source_error(format!(
            "Nothing installed found under {}. Looked for: {looked_for}, and for installed \
             Python distributions and Node packages.",
            root.display()
        )));
    }

    log(
        LogLevel::Info,
        &format!(
            "Cataloged {} packages from {}",
            findings.len(),
            cataloged.join(" and ")
        ),
    );

    artifacts::resolve_missing_licenses(&mut findings);
    classify_findings(&mut findings, strict);
    crate::clearlydefined::resolve_unknown_licenses(&mut findings, strict);
    Ok(findings)
}

/// Read a package database, or `Ok(None)` when the tree does not have one.
///
/// Shared by the catalogers so "no database here" and "a database I could not read" stay distinct:
/// the first is how every image that uses a different package manager looks, the second is a
/// problem the user needs told about.
fn read_database(path: &Path) -> FeludaResult<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|error| source_error(format!("Failed to read {}: {error}", path.display())))
}

/// Build a finding for one installed package.
///
/// The distro namespace goes in front of the name, which is what puts it in the PURL:
/// `pkg:deb/debian/libssl3`. A Debian `libssl3` and an Ubuntu one are different packages, and a
/// consumer matching feluda's SBOM against anyone else's needs to see that.
fn package_finding(
    ecosystem: Ecosystem,
    namespace: Option<&str>,
    name: &str,
    version: &str,
    license: Option<&str>,
) -> LicenseInfo {
    let name = match namespace {
        Some(namespace) => format!("{namespace}/{name}"),
        None => name.to_string(),
    };

    LicenseInfo {
        name,
        version: version.to_string(),
        license: license
            .map(str::trim)
            .filter(|license| !license.is_empty() && !license.eq_ignore_ascii_case("unknown"))
            .map(str::to_string),
        // Filled in by `classify_findings`; compatibility is the shared pipeline's job.
        is_restrictive: false,
        compatibility: LicenseCompatibility::Unknown,
        osi_status: OsiStatus::Unknown,
        ecosystem,
        sub_project: None,
    }
}

/// The distribution's own name for itself, from `os-release`.
///
/// This is the PURL namespace: `debian`, `ubuntu`, `alpine`. A tree without an `os-release` file
/// (an installation directory rather than a full root filesystem) simply has no namespace, which
/// still leaves a valid PURL.
fn distro_namespace(root: &Path) -> Option<String> {
    for candidate in OS_RELEASE_PATHS {
        let Ok(content) = std::fs::read_to_string(root.join(candidate)) else {
            continue;
        };
        if let Some(id) = parse_os_release_id(&content) {
            return Some(id);
        }
    }
    log(
        LogLevel::Warn,
        "No os-release found; OS packages will carry no distro namespace",
    );
    None
}

/// Read `ID=` out of an `os-release` file.
fn parse_os_release_id(content: &str) -> Option<String> {
    content
        .lines()
        .filter_map(|line| line.trim().strip_prefix("ID="))
        .map(|id| id.trim().trim_matches(['"', '\'']).to_ascii_lowercase())
        .find(|id| !id.is_empty())
}

/// Report a bad source and turn it into an error.
///
/// `FeludaError::log` only prints under `--debug`, and someone who pointed `--filesystem` at the
/// wrong directory needs to be told why the scan found nothing.
fn source_error(message: String) -> FeludaError {
    eprintln!("❌ {message}");
    FeludaError::Parser(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn test_reads_the_distro_id() {
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "etc/os-release",
            "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nNAME=\"Debian GNU/Linux\"\nID=debian\nVERSION_ID=\"12\"\n",
        );
        assert_eq!(distro_namespace(temp.path()).as_deref(), Some("debian"));
    }

    #[test]
    fn test_quoted_and_uppercase_ids_normalize() {
        assert_eq!(
            parse_os_release_id("ID=\"Alpine\"\n").as_deref(),
            Some("alpine")
        );
        assert_eq!(
            parse_os_release_id("ID='ubuntu'\n").as_deref(),
            Some("ubuntu")
        );
    }

    #[test]
    fn test_id_like_fields_are_not_the_id() {
        // `ID_LIKE=debian` says what the distro resembles, not what it is.
        assert_eq!(
            parse_os_release_id("ID_LIKE=debian\nID=ubuntu\n").as_deref(),
            Some("ubuntu")
        );
        assert_eq!(parse_os_release_id("VERSION_ID=\"12\"\n"), None);
    }

    #[test]
    fn test_falls_back_to_usr_lib_os_release() {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "usr/lib/os-release", "ID=alpine\n");
        assert_eq!(distro_namespace(temp.path()).as_deref(), Some("alpine"));
    }

    #[test]
    fn test_no_os_release_means_no_namespace() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(distro_namespace(temp.path()), None);
    }

    #[test]
    fn test_a_tree_with_nothing_installed_is_an_error() {
        // Silence would look exactly like a clean scan.
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "etc/os-release", "ID=debian\n");

        let error = scan_filesystem(temp.path(), false).unwrap_err();
        assert!(
            error.to_string().contains("Nothing installed found"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_an_installation_tree_needs_no_package_database() {
        // `/opt/app` is a legitimate thing to point --filesystem at, and it has no distro behind it.
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "node_modules/lodash/package.json",
            r#"{"name":"lodash","version":"4.17.21","license":"MIT"}"#,
        );

        let findings = scan_filesystem(temp.path(), false).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].name, "lodash");
        assert_eq!(findings[0].license.as_deref(), Some("MIT"));
    }

    #[test]
    fn test_an_artifact_an_os_package_ships_is_reported_once() {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "etc/os-release", "ID=debian\n");
        write(
            temp.path(),
            dpkg::DATABASE_PATH,
            "Package: python3-yaml\nStatus: install ok installed\nVersion: 6.0-3\n",
        );
        let metadata = "usr/lib/python3/dist-packages/PyYAML-6.0.egg-info/PKG-INFO";
        write(
            temp.path(),
            metadata,
            "Metadata-Version: 2.1\nName: PyYAML\nVersion: 6.0\nLicense: MIT\n",
        );
        write(
            temp.path(),
            "var/lib/dpkg/info/python3-yaml.list",
            &format!("/{metadata}\n"),
        );

        let findings = scan_filesystem(temp.path(), false).unwrap();
        let names: Vec<&str> = findings.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["debian/python3-yaml"]);
    }

    #[test]
    fn test_an_artifact_no_os_package_ships_is_reported() {
        // The same tree without the file list: nothing says the deb owns it, so it is the
        // application's own dependency and belongs in the report.
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "etc/os-release", "ID=debian\n");
        write(
            temp.path(),
            dpkg::DATABASE_PATH,
            "Package: python3-minimal\nStatus: install ok installed\nVersion: 3.11.2-1\n",
        );
        write(
            temp.path(),
            "usr/local/lib/python3.11/site-packages/requests-2.32.3.dist-info/METADATA",
            "Metadata-Version: 2.1\nName: requests\nVersion: 2.32.3\nLicense: Apache-2.0\n",
        );

        let findings = scan_filesystem(temp.path(), false).unwrap();
        let requests = findings
            .iter()
            .find(|finding| finding.name == "requests")
            .expect("installed distribution missing");
        assert_eq!(requests.purl().as_deref(), Some("pkg:pypi/requests@2.32.3"));
    }

    #[test]
    fn test_a_path_that_is_not_a_directory_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "rootfs.tar", "not a directory");

        let error = scan_filesystem(&temp.path().join("rootfs.tar"), false).unwrap_err();
        assert!(
            error.to_string().contains("Not a directory"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_an_empty_license_field_is_not_a_license() {
        let finding = package_finding(
            Ecosystem::Apk,
            Some("alpine"),
            "musl",
            "1.2.5-r0",
            Some(" "),
        );
        assert_eq!(finding.license, None);
        assert_eq!(finding.name, "alpine/musl");
    }

    #[test]
    fn test_both_catalogers_run_on_one_tree() {
        // A tree can carry both databases; reporting only the first would hide the rest.
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "etc/os-release", "ID=debian\n");
        write(
            temp.path(),
            apk::DATABASE_PATH,
            "P:musl\nV:1.2.5-r0\nL:MIT\n",
        );
        write(
            temp.path(),
            dpkg::DATABASE_PATH,
            "Package: bash\nStatus: install ok installed\nVersion: 5.2.15-2\n",
        );

        let findings = scan_filesystem(temp.path(), false).unwrap();
        let names: Vec<&str> = findings.iter().map(|f| f.name.as_str()).collect();
        assert!(
            names.contains(&"debian/musl"),
            "apk package missing: {names:?}"
        );
        assert!(
            names.contains(&"debian/bash"),
            "dpkg package missing: {names:?}"
        );
    }
}
