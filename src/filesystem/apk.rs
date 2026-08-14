//! Cataloging installed Alpine packages.
//!
//! apk keeps its installed database at `/lib/apk/db/installed`, in the same plain text format as
//! APKINDEX: records separated by blank lines, each line a single letter key and a value. The
//! license is one of those keys, so unlike dpkg there is no second file to read and no text to
//! match: what the package said about itself is right there.

use std::path::{Path, PathBuf};

use crate::debug::{log, FeludaResult, LogLevel};
use crate::licenses::LicenseInfo;
use crate::purl::Ecosystem;

use super::artifacts::is_artifact_metadata;
use super::{package_finding, read_database, Catalog};

/// Where apk records what is installed, relative to the root of the filesystem being scanned.
pub const DATABASE_PATH: &str = "lib/apk/db/installed";

/// Read every installed package out of an Alpine root filesystem.
///
/// `Ok(None)` means the tree has no apk database, which is the normal answer for a Debian or RPM
/// image and not an error. A database that is there but unreadable is.
pub fn catalog(root: &Path, namespace: Option<&str>) -> FeludaResult<Option<Catalog>> {
    let Some(content) = read_database(&root.join(DATABASE_PATH))? else {
        return Ok(None);
    };

    let catalog = parse_installed(&content, namespace);
    log(
        LogLevel::Info,
        &format!("Cataloged {} apk packages", catalog.packages.len()),
    );
    Ok(Some(catalog))
}

/// Turn the installed database into findings, and note the artifact metadata it claims.
///
/// The file list is in the same records as the packages: `F:` names a directory and the `R:` lines
/// under it name its files. Reading both in one pass is why the ownership set costs nothing.
fn parse_installed(content: &str, namespace: Option<&str>) -> Catalog {
    let mut catalog = Catalog::default();
    let mut record = Record::default();
    let mut directory: Option<PathBuf> = None;

    for line in content.lines() {
        if line.trim().is_empty() {
            if let Some(package) = record.take(namespace) {
                catalog.packages.push(package);
            }
            directory = None;
            continue;
        }

        // Every line is `K:value`. Anything else is not a record line, and apk itself ignores it.
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "P" => record.name = Some(value.to_string()),
            "V" => record.version = Some(value.to_string()),
            "L" => record.license = Some(value.to_string()),
            // A directory, given relative to the root with no leading slash.
            "F" => directory = Some(PathBuf::from(value)),
            // A file inside the directory the last `F:` named.
            "R" => {
                if let Some(directory) = &directory {
                    let path = directory.join(value);
                    if is_artifact_metadata(&path) {
                        catalog.owned.insert(path);
                    }
                }
            }
            _ => {}
        }
    }

    // A database that does not end with a blank line still described a package.
    if let Some(package) = record.take(namespace) {
        catalog.packages.push(package);
    }
    catalog
}

/// The fields of one record that say what the package is.
#[derive(Default)]
struct Record {
    name: Option<String>,
    version: Option<String>,
    license: Option<String>,
}

impl Record {
    /// Consume the record, yielding a finding when it named a package.
    fn take(&mut self, namespace: Option<&str>) -> Option<LicenseInfo> {
        let record = std::mem::take(self);
        let name = record.name?;
        Some(package_finding(
            Ecosystem::Apk,
            namespace,
            &name,
            record.version.as_deref().unwrap_or_default(),
            record.license.as_deref(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The findings of a parse, which is what most of these tests are about.
    fn parse(content: &str, namespace: Option<&str>) -> Vec<LicenseInfo> {
        parse_installed(content, namespace).packages
    }

    const INSTALLED: &str = "C:Q1eVpkasfnUyBcaVKnW2Wzv/kD0eE=\n\
        P:musl\n\
        V:1.2.5-r0\n\
        A:x86_64\n\
        T:the musl c library (libc) implementation\n\
        U:https://musl.libc.org/\n\
        L:MIT\n\
        o:musl\n\
        \n\
        C:Q1TtM7lJ9d5S38S8mHqUZaBLYb0nQ=\n\
        P:busybox\n\
        V:1.36.1-r29\n\
        A:x86_64\n\
        L:GPL-2.0-only\n\
        o:busybox\n\
        \n";

    #[test]
    fn test_reads_name_version_and_license() {
        let packages = parse(INSTALLED, Some("alpine"));
        assert_eq!(packages.len(), 2);

        assert_eq!(packages[0].name, "alpine/musl");
        assert_eq!(packages[0].version, "1.2.5-r0");
        assert_eq!(packages[0].license.as_deref(), Some("MIT"));
        assert_eq!(packages[0].ecosystem, Ecosystem::Apk);
        assert_eq!(
            packages[0].purl().as_deref(),
            Some("pkg:apk/alpine/musl@1.2.5-r0")
        );

        assert_eq!(packages[1].name, "alpine/busybox");
        assert_eq!(packages[1].license.as_deref(), Some("GPL-2.0-only"));
    }

    #[test]
    fn test_final_record_without_a_trailing_blank_line_is_kept() {
        let packages = parse("P:musl\nV:1.2.5-r0\nL:MIT", Some("alpine"));
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "alpine/musl");
    }

    #[test]
    fn test_missing_license_stays_unset() {
        // Reported as unknown rather than guessed at.
        let packages = parse("P:mystery\nV:1.0\n", Some("alpine"));
        assert_eq!(packages[0].license, None);
    }

    #[test]
    fn test_empty_license_field_is_not_a_license() {
        let packages = parse("P:mystery\nV:1.0\nL:\n", Some("alpine"));
        assert_eq!(packages[0].license, None);
    }

    #[test]
    fn test_records_without_a_name_are_skipped() {
        // File entries (`F:`) trail every real record; a stray fragment names no package.
        let packages = parse("V:1.0\nL:MIT\n\nP:musl\nV:1.2.5-r0\n", Some("alpine"));
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "alpine/musl");
    }

    #[test]
    fn test_without_os_release_the_name_has_no_namespace() {
        let packages = parse("P:musl\nV:1.2.5-r0\nL:MIT\n", None);
        assert_eq!(packages[0].name, "musl");
        assert_eq!(packages[0].purl().as_deref(), Some("pkg:apk/musl@1.2.5-r0"));
    }

    #[test]
    fn test_license_expressions_survive_intact() {
        // Alpine states SPDX expressions directly, and the classifier already understands them.
        let packages = parse(
            "P:linux-headers\nV:6.6-r0\nL:GPL-2.0-only WITH Linux-syscall-note\n",
            Some("alpine"),
        );
        assert_eq!(
            packages[0].license.as_deref(),
            Some("GPL-2.0-only WITH Linux-syscall-note")
        );
    }

    #[test]
    fn test_catalog_returns_none_without_a_database() {
        let temp = tempfile::tempdir().unwrap();
        assert!(catalog(temp.path(), Some("alpine")).unwrap().is_none());
    }

    #[test]
    fn test_catalog_reads_a_database_from_disk() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join(DATABASE_PATH);
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        std::fs::write(&database, INSTALLED).unwrap();

        let catalog = catalog(temp.path(), Some("alpine"))
            .unwrap()
            .expect("database is present");
        assert_eq!(catalog.packages.len(), 2);
    }

    #[test]
    fn test_the_file_list_claims_installed_artifact_metadata() {
        // py3-yaml's own record. The distribution it installs is not a second package.
        let catalog = parse_installed(
            "P:py3-yaml\n\
             V:6.0.1-r2\n\
             L:MIT\n\
             F:usr/lib/python3.12/site-packages/PyYAML-6.0.1-py3.12.egg-info\n\
             R:PKG-INFO\n\
             R:SOURCES.txt\n\
             F:usr/lib/python3.12/site-packages/yaml\n\
             R:__init__.py\n\
             \n",
            Some("alpine"),
        );
        assert_eq!(
            catalog.owned,
            std::collections::HashSet::from([PathBuf::from(
                "usr/lib/python3.12/site-packages/PyYAML-6.0.1-py3.12.egg-info/PKG-INFO"
            )]),
        );
    }

    #[test]
    fn test_a_file_entry_without_its_directory_is_ignored() {
        // `R:` only means something under the `F:` above it, and a record boundary clears that.
        let catalog = parse_installed("R:PKG-INFO\n\nP:musl\nV:1.2.5-r0\nL:MIT\n", Some("alpine"));
        assert!(catalog.owned.is_empty());
    }
}
