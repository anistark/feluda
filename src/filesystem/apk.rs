//! Cataloging installed Alpine packages.
//!
//! apk keeps its installed database at `/lib/apk/db/installed`, in the same plain text format as
//! APKINDEX: records separated by blank lines, each line a single letter key and a value. The
//! license is one of those keys, so unlike dpkg there is no second file to read and no text to
//! match: what the package said about itself is right there.

use std::path::Path;

use crate::debug::{log, FeludaResult, LogLevel};
use crate::licenses::LicenseInfo;
use crate::purl::Ecosystem;

use super::{package_finding, read_database};

/// Where apk records what is installed, relative to the root of the filesystem being scanned.
pub const DATABASE_PATH: &str = "lib/apk/db/installed";

/// Read every installed package out of an Alpine root filesystem.
///
/// `Ok(None)` means the tree has no apk database, which is the normal answer for a Debian or RPM
/// image and not an error. A database that is there but unreadable is.
pub fn catalog(root: &Path, namespace: Option<&str>) -> FeludaResult<Option<Vec<LicenseInfo>>> {
    let Some(content) = read_database(&root.join(DATABASE_PATH))? else {
        return Ok(None);
    };

    let packages = parse_installed(&content, namespace);
    log(
        LogLevel::Info,
        &format!("Cataloged {} apk packages", packages.len()),
    );
    Ok(Some(packages))
}

/// Turn the installed database into findings.
fn parse_installed(content: &str, namespace: Option<&str>) -> Vec<LicenseInfo> {
    let mut packages = Vec::new();
    let mut record = Record::default();

    for line in content.lines() {
        if line.trim().is_empty() {
            if let Some(package) = record.take(namespace) {
                packages.push(package);
            }
            continue;
        }

        // Every line is `K:value`. Anything else is not a record line, and apk itself ignores it.
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key {
            "P" => record.name = Some(value.trim().to_string()),
            "V" => record.version = Some(value.trim().to_string()),
            "L" => record.license = Some(value.trim().to_string()),
            _ => {}
        }
    }

    // A database that does not end with a blank line still described a package.
    if let Some(package) = record.take(namespace) {
        packages.push(package);
    }
    packages
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
        let packages = parse_installed(INSTALLED, Some("alpine"));
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
        let packages = parse_installed("P:musl\nV:1.2.5-r0\nL:MIT", Some("alpine"));
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "alpine/musl");
    }

    #[test]
    fn test_missing_license_stays_unset() {
        // Reported as unknown rather than guessed at.
        let packages = parse_installed("P:mystery\nV:1.0\n", Some("alpine"));
        assert_eq!(packages[0].license, None);
    }

    #[test]
    fn test_empty_license_field_is_not_a_license() {
        let packages = parse_installed("P:mystery\nV:1.0\nL:\n", Some("alpine"));
        assert_eq!(packages[0].license, None);
    }

    #[test]
    fn test_records_without_a_name_are_skipped() {
        // File entries (`F:`) trail every real record; a stray fragment names no package.
        let packages = parse_installed("V:1.0\nL:MIT\n\nP:musl\nV:1.2.5-r0\n", Some("alpine"));
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "alpine/musl");
    }

    #[test]
    fn test_without_os_release_the_name_has_no_namespace() {
        let packages = parse_installed("P:musl\nV:1.2.5-r0\nL:MIT\n", None);
        assert_eq!(packages[0].name, "musl");
        assert_eq!(packages[0].purl().as_deref(), Some("pkg:apk/musl@1.2.5-r0"));
    }

    #[test]
    fn test_license_expressions_survive_intact() {
        // Alpine states SPDX expressions directly, and the classifier already understands them.
        let packages = parse_installed(
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

        let packages = catalog(temp.path(), Some("alpine"))
            .unwrap()
            .expect("database is present");
        assert_eq!(packages.len(), 2);
    }
}
