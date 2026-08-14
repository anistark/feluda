//! Cataloging installed Debian and Ubuntu packages.
//!
//! dpkg is the awkward one. `/var/lib/dpkg/status` says exactly what is installed and at what
//! version, but it carries no license field at all: nothing in dpkg's database does. What Debian
//! Policy guarantees instead is `/usr/share/doc/<package>/copyright` for every package, so
//! cataloging is two steps, the database for identity and a file per package for the license.
//!
//! That second step is a read per package, so it runs in parallel. Nothing here touches the
//! network: every answer is already inside the filesystem being scanned.

use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::debug::{log, FeludaResult, LogLevel};
use crate::purl::Ecosystem;

use super::artifacts::is_artifact_metadata;
use super::copyright::license_from_copyright;
use super::deb822::parse_stanzas;
use super::{package_finding, read_database, Catalog};

/// Where dpkg records what is installed, relative to the root of the filesystem being scanned.
pub const DATABASE_PATH: &str = "var/lib/dpkg/status";

/// Where dpkg records the files each package installed, one `<package>.list` per package.
const INFO_DIRECTORY: &str = "var/lib/dpkg/info";

/// The extension of a file list in that directory. The same directory holds maintainer scripts and
/// checksums, which say nothing about what a package owns.
const FILE_LIST_EXTENSION: &str = "list";

/// Where Debian Policy puts each package's copyright file.
const DOC_DIRECTORY: &str = "usr/share/doc";

/// The only `Status:` state that means the package's files are on disk.
///
/// dpkg keeps stanzas for packages it has removed (`deinstall ok config-files`) and for ones whose
/// installation broke part way (`install ok half-installed`). Neither is software the image ships.
const INSTALLED_STATE: &str = "installed";

/// One installed package, before its license has been looked up.
struct Entry {
    name: String,
    version: String,
    /// The source package, when it differs from the binary package. Several binaries built from
    /// one source share a single copyright file, which dpkg records by pointing the binary's doc
    /// directory at the source's.
    source: Option<String>,
}

/// Read every installed package out of a Debian or Ubuntu root filesystem.
///
/// `Ok(None)` means the tree has no dpkg database, which is the normal answer for an Alpine or RPM
/// image and not an error. A database that is there but unreadable is.
pub fn catalog(root: &Path, namespace: Option<&str>) -> FeludaResult<Option<Catalog>> {
    let Some(content) = read_database(&root.join(DATABASE_PATH))? else {
        return Ok(None);
    };

    let entries = parse_status(&content);
    log(
        LogLevel::Info,
        &format!("Found {} installed dpkg packages", entries.len()),
    );

    let packages = entries
        .par_iter()
        .map(|entry| {
            let license = read_license(root, entry);
            package_finding(
                Ecosystem::Deb,
                namespace,
                &entry.name,
                &entry.version,
                license.as_deref(),
            )
        })
        .collect();

    Ok(Some(Catalog {
        packages,
        owned: owned_paths(root),
    }))
}

/// The artifact metadata files dpkg records as belonging to a package.
///
/// A Debian Python package installs a real distribution into `dist-packages`, metadata directory
/// and all. Without this the same library reports twice, once as `pkg:deb/debian/python3-yaml` and
/// once as `pkg:pypi/pyyaml`, and a reader has no way to tell that they are one thing.
///
/// Filtered as it is read: a full root filesystem's file list runs to hundreds of thousands of
/// entries and only the metadata files are ever asked about.
fn owned_paths(root: &Path) -> HashSet<PathBuf> {
    let directory = root.join(INFO_DIRECTORY);
    let Ok(entries) = std::fs::read_dir(&directory) else {
        log(
            LogLevel::Warn,
            &format!(
                "No dpkg file lists at {}; installed artifacts cannot be attributed to packages",
                directory.display()
            ),
        );
        return HashSet::new();
    };

    let lists: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == FILE_LIST_EXTENSION)
        })
        .collect();

    let owned: HashSet<PathBuf> = lists
        .par_iter()
        .flat_map(|list| {
            let Ok(content) = std::fs::read_to_string(list) else {
                return Vec::new();
            };
            content.lines().filter_map(owned_path).collect::<Vec<_>>()
        })
        .collect();

    log(
        LogLevel::Info,
        &format!(
            "{} of the files in {} dpkg packages are installed artifact metadata",
            owned.len(),
            lists.len()
        ),
    );
    owned
}

/// One line of a file list, as a path relative to the scan root, when it is metadata worth
/// remembering.
///
/// dpkg writes absolute paths; everything else in this module is relative to the root being
/// scanned, which is what the artifact catalogers compare against.
fn owned_path(line: &str) -> Option<PathBuf> {
    let path = PathBuf::from(line.trim().trim_start_matches('/'));
    is_artifact_metadata(&path).then_some(path)
}

/// Read the installed packages out of a `status` file.
///
/// Two architectures of one package are two stanzas with the same name and version, and the same
/// license. Since the architecture is a PURL qualifier rather than part of a package's identity,
/// they would produce identical findings, so the second is dropped.
fn parse_status(content: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();

    for stanza in parse_stanzas(content) {
        let Some(name) = stanza.first_line("Package") else {
            continue;
        };
        if !is_installed(stanza.first_line("Status")) {
            log(
                LogLevel::Info,
                &format!(
                    "Skipping {name}: not installed ({:?})",
                    stanza.get("Status")
                ),
            );
            continue;
        }
        let version = stanza.first_line("Version").unwrap_or_default();

        if entries
            .iter()
            .any(|entry| entry.name == name && entry.version == version)
        {
            log(
                LogLevel::Info,
                &format!("Skipping duplicate {name} {version} (another architecture)"),
            );
            continue;
        }

        entries.push(Entry {
            name: name.to_string(),
            version: version.to_string(),
            source: source_package(stanza.first_line("Source")),
        });
    }

    entries
}

/// Whether a `Status:` field says the package's files are on disk.
///
/// The field is three words, `<want> <error> <state>`, and only the state matters here.
fn is_installed(status: Option<&str>) -> bool {
    status.is_some_and(|status| {
        status
            .split_whitespace()
            .next_back()
            .is_some_and(|state| state == INSTALLED_STATE)
    })
}

/// The source package name, dropping the version dpkg appends when it differs from the binary's.
///
/// `Source: openssl (3.0.15-1)` names the package; the parenthesised version is not part of it.
fn source_package(source: Option<&str>) -> Option<String> {
    let source = source?.split('(').next()?.trim();
    (!source.is_empty()).then(|| source.to_string())
}

/// Find a package's license in the copyright file Debian Policy requires it to ship.
///
/// The binary package's own directory comes first. Packages built from a shared source often have
/// no directory of their own, so the source package's is tried next.
fn read_license(root: &Path, entry: &Entry) -> Option<String> {
    let candidates = [Some(entry.name.as_str()), entry.source.as_deref()];
    for package in candidates.into_iter().flatten() {
        let path = copyright_path(root, package);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        match license_from_copyright(&content) {
            Some(license) => return Some(license),
            // The file is there but says nothing a license can be read out of. Reporting the
            // package as unknown is the honest answer; the source package's file, if there is one,
            // is worth a look first.
            None => log(
                LogLevel::Warn,
                &format!("No license found in {}", path.display()),
            ),
        }
    }

    log(
        LogLevel::Warn,
        &format!("No copyright file found for {}", entry.name),
    );
    None
}

/// The path to a package's copyright file inside the scanned tree.
fn copyright_path(root: &Path, package: &str) -> PathBuf {
    root.join(DOC_DIRECTORY).join(package).join("copyright")
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Package: libssl3\n\
        Status: install ok installed\n\
        Priority: optional\n\
        Section: libs\n\
        Architecture: amd64\n\
        Multi-Arch: same\n\
        Source: openssl\n\
        Version: 3.0.15-1~deb12u1\n\
        Description: Secure Sockets Layer toolkit\n\
         This package is part of the OpenSSL project's implementation.\n\
        \n\
        Package: bash\n\
        Status: install ok installed\n\
        Architecture: amd64\n\
        Version: 5.2.15-2+b7\n\
        \n\
        Package: removed-thing\n\
        Status: deinstall ok config-files\n\
        Architecture: amd64\n\
        Version: 1.0\n\
        \n";

    fn rootfs(status: &str) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join(DATABASE_PATH);
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        std::fs::write(&database, status).unwrap();
        temp
    }

    fn write_copyright(root: &Path, package: &str, content: &str) {
        let path = copyright_path(root, package);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    const GPL_COPYRIGHT: &str =
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
        \n\
        Files: *\n\
        Copyright: 2020 Someone\n\
        License: GPL-3+\n";

    #[test]
    fn test_reads_installed_packages_only() {
        let entries = parse_status(STATUS);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "libssl3");
        assert_eq!(entries[0].version, "3.0.15-1~deb12u1");
        assert_eq!(entries[0].source.as_deref(), Some("openssl"));
        assert_eq!(entries[1].name, "bash");
        assert_eq!(entries[1].source, None);
    }

    #[test]
    fn test_half_installed_is_not_installed() {
        // The state is the last word, and `half-installed` is not `installed`.
        assert!(!is_installed(Some("install ok half-installed")));
        assert!(!is_installed(Some("deinstall ok config-files")));
        assert!(!is_installed(Some("purge ok not-installed")));
        assert!(is_installed(Some("install ok installed")));
        assert!(!is_installed(None));
    }

    #[test]
    fn test_source_version_is_not_part_of_the_source_name() {
        assert_eq!(
            source_package(Some("openssl (3.0.15-1)")).as_deref(),
            Some("openssl")
        );
        assert_eq!(source_package(Some("openssl")).as_deref(), Some("openssl"));
        assert_eq!(source_package(None), None);
    }

    #[test]
    fn test_second_architecture_of_one_package_is_dropped() {
        let status = "Package: libssl3\n\
            Status: install ok installed\n\
            Architecture: amd64\n\
            Version: 3.0.15-1\n\
            \n\
            Package: libssl3\n\
            Status: install ok installed\n\
            Architecture: i386\n\
            Version: 3.0.15-1\n\
            \n";
        assert_eq!(parse_status(status).len(), 1);
    }

    #[test]
    fn test_different_versions_of_one_package_both_count() {
        let status = "Package: linux-image\n\
            Status: install ok installed\n\
            Version: 6.1.0-13\n\
            \n\
            Package: linux-image\n\
            Status: install ok installed\n\
            Version: 6.1.0-18\n\
            \n";
        assert_eq!(parse_status(status).len(), 2);
    }

    #[test]
    fn test_catalog_resolves_licenses_from_copyright_files() {
        let temp = rootfs(STATUS);
        write_copyright(temp.path(), "libssl3", GPL_COPYRIGHT);
        write_copyright(
            temp.path(),
            "bash",
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
             \n\
             Files: *\n\
             License: Expat\n",
        );

        let packages = catalog(temp.path(), Some("debian"))
            .unwrap()
            .unwrap()
            .packages;
        assert_eq!(packages.len(), 2);

        let libssl = &packages[0];
        assert_eq!(libssl.name, "debian/libssl3");
        assert_eq!(libssl.license.as_deref(), Some("GPL-3.0-or-later"));
        assert_eq!(libssl.ecosystem, Ecosystem::Deb);
        assert_eq!(
            libssl.purl().as_deref(),
            Some("pkg:deb/debian/libssl3@3.0.15-1~deb12u1")
        );
        assert_eq!(packages[1].license.as_deref(), Some("MIT"));
    }

    #[test]
    fn test_falls_back_to_the_source_packages_copyright() {
        // libssl3 ships no doc directory of its own; openssl, which it is built from, does.
        let temp = rootfs(STATUS);
        write_copyright(temp.path(), "openssl", GPL_COPYRIGHT);

        let packages = catalog(temp.path(), Some("debian"))
            .unwrap()
            .unwrap()
            .packages;
        assert_eq!(packages[0].license.as_deref(), Some("GPL-3.0-or-later"));
    }

    #[test]
    fn test_a_package_with_no_copyright_file_reports_unknown() {
        let temp = rootfs(STATUS);
        let packages = catalog(temp.path(), Some("debian"))
            .unwrap()
            .unwrap()
            .packages;
        assert!(packages.iter().all(|package| package.license.is_none()));
    }

    #[test]
    fn test_catalog_returns_none_without_a_database() {
        let temp = tempfile::tempdir().unwrap();
        assert!(catalog(temp.path(), Some("debian")).unwrap().is_none());
    }

    #[test]
    fn test_file_lists_claim_the_artifact_metadata_a_package_installs() {
        let temp = rootfs(STATUS);
        std::fs::create_dir_all(temp.path().join(INFO_DIRECTORY)).unwrap();
        std::fs::write(
            temp.path().join(INFO_DIRECTORY).join("python3-yaml.list"),
            "/.\n\
             /usr/lib/python3/dist-packages\n\
             /usr/lib/python3/dist-packages/PyYAML-6.0.egg-info/PKG-INFO\n\
             /usr/lib/python3/dist-packages/yaml/__init__.py\n",
        )
        .unwrap();
        std::fs::write(
            // Not a file list, and its contents are not paths.
            temp.path()
                .join(INFO_DIRECTORY)
                .join("python3-yaml.md5sums"),
            "d41d8cd98f00b204e9800998ecf8427e  usr/lib/python3/dist-packages/yaml/__init__.py\n",
        )
        .unwrap();

        let owned = catalog(temp.path(), Some("debian")).unwrap().unwrap().owned;
        assert_eq!(
            owned,
            HashSet::from([PathBuf::from(
                "usr/lib/python3/dist-packages/PyYAML-6.0.egg-info/PKG-INFO"
            )])
        );
    }

    #[test]
    fn test_a_tree_with_no_file_lists_claims_nothing() {
        // Some minimal images ship the status file and drop `/var/lib/dpkg/info` to save space.
        let temp = rootfs(STATUS);
        assert!(catalog(temp.path(), Some("debian"))
            .unwrap()
            .unwrap()
            .owned
            .is_empty());
    }
}
