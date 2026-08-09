//! Integration tests for SBOM ingest (#249).
//!
//! Each test drives the real `feluda` binary (`CARGO_BIN_EXE_feluda`) against a document shaped
//! the way syft, Trivy and cdxgen actually emit them. Every component states a license, so no
//! registry lookup is attempted and the tests hold with or without network access; the one
//! deliberately unstated component belongs to an ecosystem with no registry to ask.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

/// An image scan as syft emits it: OS packages carrying distro-qualified PURLs, a declared
/// license the concluded field leaves at NOASSERTION, and one package with no license at all.
const SYFT_SPDX: &str = r#"{
  "spdxVersion": "SPDX-2.3",
  "dataLicense": "CC0-1.0",
  "SPDXID": "SPDXRef-DOCUMENT",
  "name": "nginx-latest",
  "documentNamespace": "https://anchore.com/syft/image/nginx-latest-7f2c",
  "creationInfo": { "created": "2026-08-09T10:00:00Z", "creators": ["Tool: syft-1.20.0"] },
  "packages": [
    {
      "name": "readline",
      "SPDXID": "SPDXRef-Package-deb-readline",
      "versionInfo": "8.2-1.3",
      "downloadLocation": "NOASSERTION",
      "licenseConcluded": "NOASSERTION",
      "licenseDeclared": "GPL-3.0-or-later",
      "externalRefs": [{
        "referenceCategory": "PACKAGE-MANAGER",
        "referenceType": "purl",
        "referenceLocator": "pkg:deb/debian/readline@8.2-1.3?arch=amd64&distro=debian-12"
      }]
    },
    {
      "name": "zlib1g",
      "SPDXID": "SPDXRef-Package-deb-zlib1g",
      "versionInfo": "1.2.13",
      "downloadLocation": "NOASSERTION",
      "licenseConcluded": "NOASSERTION",
      "licenseDeclared": "Zlib",
      "externalRefs": [{
        "referenceCategory": "PACKAGE-MANAGER",
        "referenceType": "purl",
        "referenceLocator": "pkg:deb/debian/zlib1g@1.2.13?arch=amd64"
      }]
    },
    {
      "name": "libmystery",
      "SPDXID": "SPDXRef-Package-deb-libmystery",
      "versionInfo": "0.1",
      "downloadLocation": "NOASSERTION",
      "licenseConcluded": "NOASSERTION",
      "licenseDeclared": "NOASSERTION",
      "externalRefs": [{
        "referenceCategory": "PACKAGE-MANAGER",
        "referenceType": "purl",
        "referenceLocator": "pkg:deb/debian/libmystery@0.1"
      }]
    }
  ]
}"#;

/// A container scan as Trivy emits it: the image itself in `metadata.component`, and the
/// inventory in `components`.
const TRIVY_CYCLONEDX: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "serialNumber": "urn:uuid:3e671687-395b-41f5-a30f-a58921a69b79",
  "version": 1,
  "metadata": {
    "timestamp": "2026-08-09T10:00:00Z",
    "tools": { "components": [{ "type": "application", "name": "trivy", "version": "0.58.0" }] },
    "component": { "bom-ref": "root", "type": "container", "name": "nginx:latest" }
  },
  "components": [
    {
      "bom-ref": "c1",
      "type": "library",
      "name": "openssl",
      "version": "3.0.15-1",
      "purl": "pkg:deb/debian/openssl@3.0.15-1?arch=amd64",
      "licenses": [{ "license": { "name": "OpenSSL" } }]
    },
    {
      "bom-ref": "c2",
      "type": "library",
      "name": "coreutils",
      "version": "9.1-1",
      "purl": "pkg:deb/debian/coreutils@9.1-1?arch=amd64",
      "licenses": [{ "license": { "id": "GPL-3.0-or-later" } }]
    }
  ]
}"#;

/// An application scan as cdxgen emits it: language packages with namespaced PURLs, an SPDX
/// expression, and multiple stated licenses on one component.
const CDXGEN_CYCLONEDX: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.6",
  "version": 1,
  "metadata": {
    "tools": { "components": [{ "type": "application", "name": "cdxgen", "version": "11.0.0" }] },
    "component": { "bom-ref": "app", "type": "application", "name": "billing-service" }
  },
  "components": [
    {
      "bom-ref": "b1",
      "type": "library",
      "group": "@babel",
      "name": "core",
      "version": "7.24.0",
      "purl": "pkg:npm/%40babel/core@7.24.0",
      "licenses": [{ "license": { "id": "MIT" } }]
    },
    {
      "bom-ref": "b2",
      "type": "library",
      "group": "com.fasterxml.jackson.core",
      "name": "jackson-databind",
      "version": "2.17.0",
      "purl": "pkg:maven/com.fasterxml.jackson.core/jackson-databind@2.17.0",
      "licenses": [{ "expression": "Apache-2.0" }]
    },
    {
      "bom-ref": "b3",
      "type": "library",
      "name": "ghostscript-bindings",
      "version": "1.0.0",
      "purl": "pkg:pypi/ghostscript-bindings@1.0.0",
      "licenses": [
        { "license": { "id": "MIT" } },
        { "license": { "id": "AGPL-3.0-only" } }
      ]
    }
  ]
}"#;

fn feluda(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(args)
        .output()
        .expect("failed to run feluda binary")
}

/// Run feluda with the document on stdin, the way a `syft ... | feluda` pipeline does.
fn feluda_with_stdin(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn feluda binary");
    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(input.as_bytes())
        .expect("failed to write SBOM to stdin");
    child
        .wait_with_output()
        .expect("failed to run feluda binary")
}

fn write_fixture(dir: &Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, content).expect("failed to write fixture");
    path.to_str()
        .expect("fixture path should be UTF-8")
        .to_string()
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
fn spdx_from_stdin_gates_on_restrictive_licenses() {
    let output = feluda_with_stdin(
        &["--sbom-input", "-", "--json", "--fail-on-restrictive"],
        SYFT_SPDX,
    );

    // The gate is the whole point of piping a container SBOM through feluda.
    assert_eq!(
        output.status.code(),
        Some(1),
        "a GPL-3.0 component should fail the run: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = report(&output);
    assert_eq!(report.len(), 3);

    // The declared license stands in when nothing was concluded, and OS packages classify like
    // any other dependency.
    let readline = find(&report, "readline");
    assert_eq!(readline["license"], "GPL-3.0-or-later");
    assert_eq!(readline["is_restrictive"], true);
    assert_eq!(readline["ecosystem"], "deb");
    assert_eq!(readline["version"], "8.2-1.3");

    let zlib = find(&report, "zlib1g");
    assert_eq!(zlib["license"], "Zlib");
    assert_eq!(zlib["is_restrictive"], false);

    // Nothing stated it, and a Debian package has no registry to ask.
    assert!(find(&report, "libmystery")["license"].is_null());
}

#[test]
fn cyclonedx_file_input_maps_components() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), "trivy.cdx.json", TRIVY_CYCLONEDX);

    let output = feluda(&["--sbom-input", &path, "--json"]);
    let report = report(&output);

    // The image in `metadata.component` is what was scanned, not one of its dependencies.
    assert_eq!(report.len(), 2);
    assert!(!report.iter().any(|entry| entry["name"] == "nginx:latest"));

    // A free-form license name survives as stated.
    assert_eq!(find(&report, "openssl")["license"], "OpenSSL");
    assert_eq!(find(&report, "coreutils")["is_restrictive"], true);
}

#[test]
fn cdxgen_purls_carry_namespaces_through() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), "cdxgen.cdx.json", CDXGEN_CYCLONEDX);

    let output = feluda(&["--sbom-input", &path, "--json"]);
    let report = report(&output);
    assert_eq!(report.len(), 3);

    // An npm scope and a Maven group are part of the package's identity, and the PURL feluda
    // reports back is the one the document carried.
    let babel = find(&report, "@babel/core");
    assert_eq!(babel["ecosystem"], "npm");
    assert_eq!(babel["purl"], "pkg:npm/%40babel/core@7.24.0");

    let jackson = find(&report, "com.fasterxml.jackson.core:jackson-databind");
    assert_eq!(jackson["ecosystem"], "maven");
    assert_eq!(
        jackson["purl"],
        "pkg:maven/com.fasterxml.jackson.core/jackson-databind@2.17.0"
    );
    assert_eq!(jackson["license"], "Apache-2.0");

    // Several stated licenses are cumulative, so the restrictive one still counts.
    let bindings = find(&report, "ghostscript-bindings");
    assert_eq!(bindings["license"], "MIT AND AGPL-3.0-only");
    assert_eq!(bindings["is_restrictive"], true);
}

#[test]
fn fully_stated_documents_round_trip_through_enrichment() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), "trivy.cdx.json", TRIVY_CYCLONEDX);
    let enriched_path = temp.path().join("enriched.cdx.json");

    let output = feluda(&[
        "--sbom-input",
        &path,
        "--sbom-enriched",
        enriched_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success());

    // Nothing needed resolving, so feluda has no conclusions to add and the document is
    // reproduced as it arrived.
    let original: Value = serde_json::from_str(TRIVY_CYCLONEDX).unwrap();
    let enriched: Value =
        serde_json::from_str(&fs::read_to_string(&enriched_path).unwrap()).unwrap();
    assert_eq!(enriched, original);

    // The confirmation goes to stderr, leaving the JSON report on stdout parseable.
    assert!(String::from_utf8_lossy(&output.stderr).contains("Enriched SBOM written"));
    assert_eq!(report(&output).len(), 2);
}

#[test]
fn documents_that_are_neither_format_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(
        temp.path(),
        "deps.json",
        r#"{"dependencies": {"lodash": "4.17.21"}}"#,
    );

    let output = feluda(&["--sbom-input", &path, "--json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("neither SPDX nor CycloneDX"));
}

#[test]
fn missing_input_file_is_reported() {
    let output = feluda(&["--sbom-input", "/nonexistent/sbom.json", "--json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to read SBOM file"));
}

#[test]
fn sbom_input_and_repo_are_mutually_exclusive() {
    let output = feluda(&[
        "--sbom-input",
        "sbom.json",
        "--repo",
        "https://github.com/anistark/feluda",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
}

#[test]
fn watch_mode_rejects_sbom_input() {
    let output = feluda(&["--sbom-input", "sbom.json", "watch"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--sbom-input is not supported"));
}
