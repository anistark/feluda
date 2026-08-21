//! Integration tests for native filesystem scanning (#252).
//!
//! Each test builds a miniature root filesystem in a temp directory and drives the real `feluda`
//! binary (`CARGO_BIN_EXE_feluda`) against it. Every license comes out of the tree itself, so
//! nothing here needs the network: OS packages have no registry to resolve against in the first
//! place, which is the whole reason the catalogers read licenses off disk.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

/// An Alpine installed database, in the format apk writes: blank-line separated records of
/// single-letter keys, with the license in `L:`.
const APK_INSTALLED: &str = "C:Q1eVpkasfnUyBcaVKnW2Wzv/kD0eE=\n\
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
    \n\
    C:Q1p2Zx8Kk0mM5s6Ml3rQGRi1oJ0mo=\n\
    P:mystery-lib\n\
    V:0.1.0-r0\n\
    A:x86_64\n\
    \n";

/// A dpkg status file. `libssl3` is built from the `openssl` source and ships no doc directory of
/// its own; `removed-thing` is a package dpkg still records but has deinstalled.
const DPKG_STATUS: &str = "Package: libssl3\n\
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
    Package: coreutils\n\
    Status: install ok installed\n\
    Architecture: amd64\n\
    Version: 9.1-1\n\
    \n\
    Package: removed-thing\n\
    Status: deinstall ok config-files\n\
    Architecture: amd64\n\
    Version: 1.0\n\
    \n";

/// Every test drives the binary with the ClearlyDefined fallback off: the fixtures are built so
/// license resolution succeeds locally, and a network lookup would make the suite depend on a
/// third party service being up.
fn feluda(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(args)
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .output()
        .expect("failed to run feluda binary")
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("path should have a parent"))
        .expect("failed to create fixture directory");
    fs::write(path, content).expect("failed to write fixture");
}

/// A DEP-5 copyright file stating one license for the whole package.
fn dep5(license: &str) -> String {
    format!(
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
         Upstream-Name: example\n\
         \n\
         Files: *\n\
         Copyright: 2020 Upstream Author\n\
         License: {license}\n"
    )
}

/// An Alpine root filesystem with an apk database.
fn alpine_rootfs() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    write(
        temp.path(),
        "etc/os-release",
        "NAME=\"Alpine Linux\"\nID=alpine\nVERSION_ID=3.20.3\n",
    );
    write(temp.path(), "lib/apk/db/installed", APK_INSTALLED);
    temp
}

/// A Fedora root filesystem carrying the checked in rpm database.
///
/// The fixture is seven real headers taken from a `fedora:41` image, so the licenses under test are
/// the ones Fedora actually ships rather than ones written to suit the test.
fn fedora_rootfs() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    write(
        temp.path(),
        "etc/os-release",
        "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n",
    );

    let database = temp.path().join("var/lib/rpm");
    fs::create_dir_all(&database).expect("failed to create rpm directory");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rpm/rpmdb.sqlite"),
        database.join("rpmdb.sqlite"),
    )
    .expect("failed to copy the rpm fixture");
    temp
}

/// A Debian root filesystem with a dpkg database and the copyright files it points at.
fn debian_rootfs() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    write(
        temp.path(),
        "etc/os-release",
        "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian\nVERSION_ID=\"12\"\n",
    );
    write(temp.path(), "var/lib/dpkg/status", DPKG_STATUS);
    // libssl3 has no doc directory; openssl, the source package it is built from, does.
    write(
        temp.path(),
        "usr/share/doc/openssl/copyright",
        &dep5("Apache-2.0"),
    );
    write(temp.path(), "usr/share/doc/bash/copyright", &dep5("GPL-3+"));
    write(
        temp.path(),
        "usr/share/doc/coreutils/copyright",
        &dep5("Expat"),
    );
    temp
}

fn report(output: &Output) -> Vec<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected a JSON report, got {stdout:?}: {e}"))
}

fn find<'a>(report: &'a [Value], name: &str) -> &'a Value {
    report
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("{name} missing from report"))
}

#[test]
fn alpine_rootfs_is_cataloged_from_the_apk_database() {
    let temp = alpine_rootfs();
    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    let report = report(&output);
    assert_eq!(report.len(), 3);

    // The license apk itself recorded, and the distro namespace from /etc/os-release.
    let musl = find(&report, "alpine/musl");
    assert_eq!(musl["license"], "MIT");
    assert_eq!(musl["is_restrictive"], false);
    assert_eq!(musl["ecosystem"], "apk");
    assert_eq!(musl["version"], "1.2.5-r0");
    assert_eq!(musl["purl"], "pkg:apk/alpine/musl@1.2.5-r0");

    let busybox = find(&report, "alpine/busybox");
    assert_eq!(busybox["license"], "GPL-2.0-only");
    assert_eq!(busybox["is_restrictive"], true);

    // A package whose record states no license is reported as unknown, never guessed.
    assert!(find(&report, "alpine/mystery-lib")["license"].is_null());
}

#[test]
fn debian_rootfs_resolves_licenses_from_copyright_files() {
    let temp = debian_rootfs();
    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    let report = report(&output);

    // The deinstalled package is not software the image ships.
    assert_eq!(report.len(), 3);
    assert!(!report
        .iter()
        .any(|entry| entry["name"] == "debian/removed-thing"));

    // libssl3 ships no copyright file of its own, so the source package's answers for it.
    let libssl = find(&report, "debian/libssl3");
    assert_eq!(libssl["license"], "Apache-2.0");
    assert_eq!(libssl["ecosystem"], "deb");
    assert_eq!(libssl["purl"], "pkg:deb/debian/libssl3@3.0.15-1~deb12u1");

    // Debian's own short names are not SPDX ids: GPL-3+ has to become GPL-3.0-or-later or the
    // restrictive gate never fires on it.
    let bash = find(&report, "debian/bash");
    assert_eq!(bash["license"], "GPL-3.0-or-later");
    assert_eq!(bash["is_restrictive"], true);

    assert_eq!(find(&report, "debian/coreutils")["license"], "MIT");
}

#[test]
fn restrictive_gate_fires_on_a_root_filesystem() {
    // The reason the whole feature exists: a container that ships GPL code fails CI.
    let temp = debian_rootfs();
    let output = feluda(&[
        "--filesystem",
        temp.path().to_str().unwrap(),
        "--json",
        "--fail-on-restrictive",
    ]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected a non-zero exit, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_clean_root_filesystem_passes_the_gate() {
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), "etc/os-release", "ID=alpine\n");
    write(
        temp.path(),
        "lib/apk/db/installed",
        "P:musl\nV:1.2.5-r0\nL:MIT\n",
    );

    let output = feluda(&[
        "--filesystem",
        temp.path().to_str().unwrap(),
        "--json",
        "--fail-on-restrictive",
    ]);
    assert!(
        output.status.success(),
        "expected a clean pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn both_package_managers_in_one_tree_are_reported_together() {
    // An image can carry both databases. Reporting only the first would hide the rest.
    let temp = debian_rootfs();
    write(temp.path(), "lib/apk/db/installed", APK_INSTALLED);

    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    let report = report(&output);
    assert_eq!(report.len(), 6);
    assert_eq!(find(&report, "debian/musl")["ecosystem"], "apk");
    assert_eq!(find(&report, "debian/bash")["ecosystem"], "deb");
}

#[test]
fn a_tree_with_nothing_installed_is_an_error() {
    // Silence here would be indistinguishable from a clean scan, so a mistyped path must not
    // report as a pass.
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), "etc/os-release", "ID=debian\n");

    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Nothing installed found"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn installed_language_artifacts_are_cataloged_alongside_os_packages() {
    // What the distro packaged is one half of an image; the application's own dependencies, which
    // arrive with no manifest behind them, are the other.
    let temp = debian_rootfs();
    write(
        temp.path(),
        "usr/local/lib/python3.11/site-packages/requests-2.32.3.dist-info/METADATA",
        "Metadata-Version: 2.1\nName: requests\nVersion: 2.32.3\nLicense: Apache-2.0\n",
    );
    write(
        temp.path(),
        "usr/local/lib/python3.11/site-packages/chardet-5.2.0.dist-info/METADATA",
        "Metadata-Version: 2.1\n\
         Name: chardet\n\
         Version: 5.2.0\n\
         Classifier: License :: OSI Approved :: GNU Lesser General Public License v2 or later (LGPLv2+)\n",
    );
    write(
        temp.path(),
        "srv/app/node_modules/@babel/core/package.json",
        r#"{"name":"@babel/core","version":"7.24.0","license":"MIT"}"#,
    );

    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    let report = report(&output);
    assert_eq!(report.len(), 6);

    let requests = find(&report, "requests");
    assert_eq!(requests["license"], "Apache-2.0");
    assert_eq!(requests["ecosystem"], "pypi");
    assert_eq!(requests["purl"], "pkg:pypi/requests@2.32.3");

    // A Trove classifier is a fixed vocabulary, so it maps onto SPDX rather than being reported as
    // the prose it is written in.
    assert_eq!(find(&report, "chardet")["license"], "LGPL-2.0-or-later");

    // The npm scope is part of the name and therefore part of the PURL.
    let babel = find(&report, "@babel/core");
    assert_eq!(babel["ecosystem"], "npm");
    assert_eq!(babel["purl"], "pkg:npm/%40babel/core@7.24.0");

    // And the OS packages are still all there.
    assert_eq!(find(&report, "debian/bash")["ecosystem"], "deb");
}

#[test]
fn an_artifact_an_os_package_ships_is_not_reported_twice() {
    // python3-yaml installs a real PyYAML distribution. It is one library, so it is one finding.
    let temp = debian_rootfs();
    let metadata = "usr/lib/python3/dist-packages/PyYAML-6.0.egg-info/PKG-INFO";
    write(
        temp.path(),
        metadata,
        "Metadata-Version: 2.1\nName: PyYAML\nVersion: 6.0\nLicense: MIT\n",
    );
    write(
        temp.path(),
        "var/lib/dpkg/info/python3-yaml.list",
        &format!("/usr/lib/python3/dist-packages\n/{metadata}\n"),
    );
    write(
        temp.path(),
        "var/lib/dpkg/status",
        &format!(
            "{DPKG_STATUS}Package: python3-yaml\nStatus: install ok installed\nVersion: 6.0-3\n\n"
        ),
    );
    write(
        temp.path(),
        "usr/share/doc/python3-yaml/copyright",
        &dep5("Expat"),
    );

    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    let report = report(&output);

    assert_eq!(find(&report, "debian/python3-yaml")["license"], "MIT");
    assert!(
        !report.iter().any(|entry| entry["name"] == "PyYAML"),
        "the distribution the deb installs was reported a second time: {report:?}"
    );
}

#[test]
fn an_installation_tree_needs_no_package_database() {
    // `/opt/app` has no distro behind it and is still worth scanning, which is the case that
    // motivated a filesystem source in the first place.
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        "node_modules/copyleft-thing/package.json",
        r#"{"name":"copyleft-thing","version":"1.0.0","license":"GPL-3.0-only"}"#,
    );

    let output = feluda(&[
        "--filesystem",
        temp.path().to_str().unwrap(),
        "--json",
        "--fail-on-restrictive",
    ]);
    let report = report(&output);
    assert_eq!(report.len(), 1);
    assert_eq!(find(&report, "copyleft-thing")["is_restrictive"], true);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn an_artifact_stating_no_license_goes_to_its_registry() {
    // Unlike an OS package, an installed distribution has coordinates a registry can answer for.
    // The fixture names nothing real, so the lookup comes back empty whether or not there is a
    // network, and the package is reported as unknown rather than guessed at.
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        "node_modules/feluda-fixture-no-such-package/package.json",
        r#"{"name":"feluda-fixture-no-such-package","version":"9.9.9"}"#,
    );

    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    let report = report(&output);
    assert!(find(&report, "feluda-fixture-no-such-package")["license"].is_null());

    // The resolution pass has to actually run, or the fall-through is only theoretical.
    let debug = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--debug"]);
    let stdout = String::from_utf8_lossy(&debug.stdout);
    assert!(
        stdout.contains("whose metadata stated none"),
        "the registry fall-through did not run: {stdout}"
    );
}

#[test]
fn sbom_generation_describes_installed_artifacts_too() {
    let temp = debian_rootfs();
    write(
        temp.path(),
        "usr/local/lib/python3.11/site-packages/requests-2.32.3.dist-info/METADATA",
        "Metadata-Version: 2.1\nName: requests\nVersion: 2.32.3\nLicense: Apache-2.0\n",
    );
    let output_path = temp.path().join("app.cdx.json");

    let output = feluda(&[
        "sbom",
        "cyclonedx",
        "--filesystem",
        temp.path().to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "sbom generation failed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(&output_path).expect("SBOM should be written"))
            .expect("SBOM should be JSON");
    let purls: Vec<&str> = document["components"]
        .as_array()
        .expect("CycloneDX document should list components")
        .iter()
        .filter_map(|component| component["purl"].as_str())
        .collect();
    assert!(
        purls.contains(&"pkg:pypi/requests@2.32.3"),
        "the installed distribution is missing from {purls:?}"
    );
    assert!(
        purls.contains(&"pkg:deb/debian/libssl3@3.0.15-1~deb12u1"),
        "the OS package is missing from {purls:?}"
    );
}

#[test]
fn a_missing_path_is_an_error() {
    let output = feluda(&["--filesystem", "/definitely/not/a/rootfs", "--json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Not a directory"));
}

#[test]
fn sbom_generation_takes_the_same_source() {
    let temp = debian_rootfs();
    let output_path = temp.path().join("rootfs.spdx.json");

    let output = feluda(&[
        "sbom",
        "spdx",
        "--filesystem",
        temp.path().to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "sbom generation failed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(&output_path).expect("SBOM should be written"))
            .expect("SBOM should be JSON");
    let packages = document["packages"]
        .as_array()
        .expect("SPDX document should list packages");
    assert_eq!(packages.len(), 3);

    // The PURL is what makes the document useful to anyone else, so it has to carry the namespace
    // that identifies which distro's libssl3 this is.
    let purls: Vec<&str> = packages
        .iter()
        .filter_map(|package| package["externalRefs"].as_array())
        .flatten()
        .filter_map(|reference| reference["referenceLocator"].as_str())
        .collect();
    assert!(
        purls.contains(&"pkg:deb/debian/libssl3@3.0.15-1~deb12u1"),
        "namespaced PURL missing from {purls:?}"
    );
}

#[test]
fn fedora_rootfs_is_cataloged_from_the_rpm_database() {
    let temp = fedora_rootfs();
    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    let report = report(&output);

    // Seven headers, less the gpg-pubkey pseudo package rpm records alongside them.
    assert_eq!(report.len(), 6);
    assert!(!report.iter().any(|entry| entry["name"]
        .as_str()
        .is_some_and(|name| name.contains("gpg-pubkey"))));

    let bzip2 = find(&report, "fedora/bzip2-libs");
    assert_eq!(bzip2["license"], "BSD-4-Clause");
    assert_eq!(bzip2["ecosystem"], "rpm");
    assert_eq!(bzip2["version"], "1.0.8-19.fc41");
    assert_eq!(bzip2["purl"], "pkg:rpm/fedora/bzip2-libs@1.0.8-19.fc41");

    // Fedora states SPDX expressions directly, and an AND expression has to survive intact for the
    // restrictive half of it to count.
    let lz4 = find(&report, "fedora/lz4-libs");
    assert_eq!(lz4["license"], "GPL-2.0-or-later AND BSD-2-Clause");
    assert_eq!(lz4["is_restrictive"], true);

    assert_eq!(
        find(&report, "fedora/libssh-config")["license"],
        "LGPL-2.1-or-later"
    );
    assert_eq!(
        find(&report, "fedora/publicsuffix-list-dafsa")["license"],
        "MPL-2.0"
    );
}

#[test]
fn an_unsupported_rpm_backend_names_itself() {
    // A CentOS 7 image. Reporting nothing installed would read as a clean scan of a machine with
    // several hundred packages on it, which is the failure this message exists to prevent.
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), "etc/os-release", "ID=centos\n");
    write(
        temp.path(),
        "var/lib/rpm/Packages",
        "a berkeley db lives here",
    );

    let output = feluda(&["--filesystem", temp.path().to_str().unwrap(), "--json"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Berkeley DB") && stderr.contains("sqlite"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn an_rpm_root_filesystem_generates_an_sbom() {
    let temp = fedora_rootfs();
    let output_path = temp.path().join("rpm-sbom.json");
    let output = feluda(&[
        "sbom",
        "spdx",
        "--filesystem",
        temp.path().to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "sbom generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(&output_path).expect("SBOM should be written"))
            .expect("SBOM should be JSON");
    let purls: Vec<&str> = document["packages"]
        .as_array()
        .expect("SPDX document should list packages")
        .iter()
        .filter_map(|package| package["externalRefs"].as_array())
        .flatten()
        .filter_map(|reference| reference["referenceLocator"].as_str())
        .collect();
    assert!(
        purls.contains(&"pkg:rpm/fedora/bzip2-libs@1.0.8-19.fc41"),
        "namespaced rpm PURL missing from {purls:?}"
    );
}

#[test]
fn filesystem_and_sbom_input_are_mutually_exclusive() {
    let output = feluda(&["--filesystem", "./", "--sbom-input", "-"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn watch_mode_rejects_filesystem() {
    let output = feluda(&["--filesystem", "./", "watch"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--filesystem is not supported"));
}
