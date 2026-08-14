//! Cataloging installed Node packages.
//!
//! npm, pnpm and yarn all install a package by writing its published tarball into a directory under
//! `node_modules`, `package.json` and all. That manifest is the package's own metadata rather than
//! the application's, which is what makes an installed tree readable without resolving anything:
//! what is on disk is what is installed, at the version that is installed.
//!
//! The `license` field has had three spellings over npm's life, and all three still appear in
//! packages people depend on today.

use std::path::Path;

use serde_json::Value as JsonValue;

use crate::licenses::detect_license_in_dir;
use crate::purl::Ecosystem;

use super::Artifact;

/// The manifest npm writes into every installed package.
const MANIFEST: &str = "package.json";

/// The directory an installed package sits in.
const NODE_MODULES: &str = "node_modules";

/// Whether `path` is the manifest of an installed package.
///
/// Being somewhere below a `node_modules` is not enough: a package's own test fixtures are full of
/// manifests, and reporting those would invent dependencies that are not installed. What identifies
/// an installed package is its position — a direct child of `node_modules`, or of an `@scope`
/// directory inside one.
pub fn is_metadata(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(MANIFEST)
        && package_directory(path).is_some()
}

/// The installed package's directory, when the manifest sits where npm puts one.
fn package_directory(path: &Path) -> Option<&Path> {
    let directory = path.parent()?;
    let parent = directory.parent()?;
    let parent_name = parent.file_name()?.to_str()?;

    if parent_name == NODE_MODULES {
        return Some(directory);
    }
    // `node_modules/@scope/name`: the scope is a directory, not part of the package's own.
    if parent_name.starts_with('@') && parent.parent()?.file_name()?.to_str()? == NODE_MODULES {
        return Some(directory);
    }
    None
}

/// Read the package described by an installed manifest.
///
/// Returns `None` when the manifest names no package, which is the only field there is no
/// reasonable substitute for: a finding with no name has no PURL and cannot be reported.
pub fn read(path: &Path) -> Option<Artifact> {
    let content = std::fs::read_to_string(path).ok()?;
    let manifest: JsonValue = serde_json::from_str(&content).ok()?;

    let name = manifest.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let version = manifest
        .get("version")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();

    // The tarball ships the package's own LICENSE, so the local file is right there when the
    // manifest omits the field.
    let license = manifest_license(&manifest)
        .or_else(|| package_directory(path).and_then(detect_license_in_dir));

    Some(Artifact {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        version: version.trim().to_string(),
        license,
    })
}

/// The license a manifest states, across the three shapes npm has accepted.
///
/// `license` as a string is current. The `{ "type": ..., "url": ... }` object and the `licenses`
/// array both predate npm 1.x and were never removed from packages that still ship.
fn manifest_license(manifest: &JsonValue) -> Option<String> {
    let field = manifest.get("license");

    if let Some(license) = field.and_then(JsonValue::as_str) {
        return stated(license);
    }
    if let Some(license) = field.and_then(|value| value.get("type")) {
        return license.as_str().and_then(stated);
    }

    // A `licenses` array lists alternatives, which SPDX spells with `OR`.
    let alternatives: Vec<String> = manifest
        .get("licenses")?
        .as_array()?
        .iter()
        .filter_map(|entry| match entry {
            JsonValue::String(license) => stated(license),
            _ => entry
                .get("type")
                .and_then(JsonValue::as_str)
                .and_then(stated),
        })
        .collect();

    match alternatives.len() {
        0 => None,
        1 => alternatives.into_iter().next(),
        _ => Some(alternatives.join(" OR ")),
    }
}

/// A license value, or `None` when it states the absence of one.
///
/// `SEE LICENSE IN <file>` is npm's way of saying the license is not an identifier, and
/// `UNLICENSED` is its way of saying there is no grant at all. Neither is a license, and only the
/// second means anything a report can act on — but calling it unknown is closer to the truth than
/// treating the literal string as an SPDX id.
fn stated(license: &str) -> Option<String> {
    let license = license.trim();
    let unstated = license.is_empty()
        || license.eq_ignore_ascii_case("unknown")
        || license.to_ascii_uppercase().starts_with("SEE LICENSE IN");
    (!unstated).then(|| license.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write(root: &Path, relative: &str, content: &str) -> PathBuf {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_recognises_an_installed_manifest() {
        assert!(is_metadata(&PathBuf::from(
            "srv/app/node_modules/lodash/package.json"
        )));
        assert!(is_metadata(&PathBuf::from(
            "srv/app/node_modules/@babel/core/package.json"
        )));
        // A nested dependency is installed too.
        assert!(is_metadata(&PathBuf::from(
            "node_modules/a/node_modules/b/package.json"
        )));
    }

    #[test]
    fn test_a_manifest_elsewhere_is_not_an_installed_package() {
        // The application's own manifest describes what it wants, not what is installed.
        assert!(!is_metadata(&PathBuf::from("srv/app/package.json")));
        // A package's test fixtures are full of manifests naming packages that are not there.
        assert!(!is_metadata(&PathBuf::from(
            "node_modules/tar/test/fixtures/package.json"
        )));
        assert!(!is_metadata(&PathBuf::from("node_modules/lodash/index.js")));
    }

    #[test]
    fn test_reads_name_version_and_license() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            temp.path(),
            "node_modules/lodash/package.json",
            r#"{"name":"lodash","version":"4.17.21","license":"MIT"}"#,
        );

        let artifact = read(&path).expect("manifest names a package");
        assert_eq!(artifact.name, "lodash");
        assert_eq!(artifact.version, "4.17.21");
        assert_eq!(artifact.license.as_deref(), Some("MIT"));
        assert_eq!(artifact.ecosystem, Ecosystem::Npm);
    }

    #[test]
    fn test_a_scope_stays_part_of_the_name() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            temp.path(),
            "node_modules/@babel/core/package.json",
            r#"{"name":"@babel/core","version":"7.24.0","license":"MIT"}"#,
        );

        let artifact = read(&path).unwrap();
        assert_eq!(artifact.name, "@babel/core");
    }

    #[test]
    fn test_the_legacy_license_object() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            temp.path(),
            "node_modules/old/package.json",
            r#"{"name":"old","version":"0.1.0","license":{"type":"ISC","url":"https://example.com"}}"#,
        );
        assert_eq!(read(&path).unwrap().license.as_deref(), Some("ISC"));
    }

    #[test]
    fn test_the_legacy_licenses_array_becomes_an_expression() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            temp.path(),
            "node_modules/older/package.json",
            r#"{"name":"older","version":"0.1.0","licenses":[{"type":"MIT"},{"type":"GPL-2.0"}]}"#,
        );
        assert_eq!(
            read(&path).unwrap().license.as_deref(),
            Some("MIT OR GPL-2.0")
        );
    }

    #[test]
    fn test_falls_back_to_the_packages_own_license_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            temp.path(),
            "node_modules/quiet/package.json",
            r#"{"name":"quiet","version":"1.0.0"}"#,
        );
        write(
            temp.path(),
            "node_modules/quiet/LICENSE",
            "MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining \
             a copy of this software and associated documentation files.",
        );
        assert_eq!(read(&path).unwrap().license.as_deref(), Some("MIT"));
    }

    #[test]
    fn test_a_package_stating_no_license_is_left_unresolved() {
        // `SEE LICENSE IN` points at a file rather than naming a license; taking it literally
        // would put a nonsense identifier in the report.
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            temp.path(),
            "node_modules/proprietary/package.json",
            r#"{"name":"proprietary","version":"1.0.0","license":"SEE LICENSE IN LICENSE.txt"}"#,
        );
        assert_eq!(read(&path).unwrap().license, None);
    }

    #[test]
    fn test_unlicensed_is_reported_as_stated() {
        // Unlike `SEE LICENSE IN`, this is a statement: there is no grant.
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            temp.path(),
            "node_modules/private/package.json",
            r#"{"name":"private","version":"1.0.0","license":"UNLICENSED"}"#,
        );
        assert_eq!(read(&path).unwrap().license.as_deref(), Some("UNLICENSED"));
    }

    #[test]
    fn test_a_manifest_without_a_name_yields_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            temp.path(),
            "node_modules/broken/package.json",
            r#"{"version":"1.0.0","license":"MIT"}"#,
        );
        assert!(read(&path).is_none());
    }

    #[test]
    fn test_unparseable_json_yields_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(temp.path(), "node_modules/broken/package.json", "{not json");
        assert!(read(&path).is_none());
    }
}
