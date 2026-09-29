//! Integration tests for the SBOM writers.
//!
//! Each test writes a Node fixture whose dependency states its license in
//! `node_modules/*/package.json`, so the license resolves locally and the suite holds offline.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

/// 119 characters, every part a well formed SPDX id. The shape a package that bundles many
/// licensed components declares, and the one #257 was filed with.
const LONG_EXPRESSION: &str = "MIT AND Apache-2.0 AND BSD-3-Clause AND ISC AND Zlib AND MPL-2.0 AND GPL-2.0-or-later AND LGPL-2.1-or-later AND CC0-1.0";

fn feluda(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .args(args)
        .output()
        .expect("failed to run feluda binary")
}

fn node_project(dir: &Path, license: &str) {
    fs::create_dir_all(dir.join("node_modules/longlic")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"longlic":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("node_modules/longlic/package.json"),
        serde_json::json!({"name": "longlic", "version": "1.0.0", "license": license}).to_string(),
    )
    .unwrap();
}

/// Generate one SBOM format for `project` and return the parsed document.
fn generate(project: &Path, format: &str, output: &Path) -> Value {
    let result = feluda(&[
        "sbom",
        format,
        "--path",
        project.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "sbom {format} failed, stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_str(&fs::read_to_string(output).expect("SBOM should be written"))
        .expect("SBOM should be JSON")
}

fn assert_validates(path: &Path) {
    let result = feluda(&["sbom", "validate", path.to_str().unwrap(), "--json"]);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let report: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("validate emitted invalid JSON: {e}\n{stdout}"));
    assert_eq!(report["error_count"], 0, "{report:#}");
}

#[test]
fn a_long_expression_survives_into_spdx() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let project = temp.path().join("app");
    node_project(&project, LONG_EXPRESSION);
    let output = temp.path().join("app.spdx.json");

    let document = generate(&project, "spdx", &output);
    let package = document["packages"]
        .as_array()
        .expect("SPDX document should list packages")
        .iter()
        .find(|package| package["name"] == "longlic")
        .expect("longlic is missing from the SPDX document");
    assert_eq!(package["licenseDeclared"], LONG_EXPRESSION);
    assert_eq!(package["licenseConcluded"], LONG_EXPRESSION);

    assert_validates(&output);
}

#[test]
fn a_long_expression_survives_into_cyclonedx() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let project = temp.path().join("app");
    node_project(&project, LONG_EXPRESSION);
    let output = temp.path().join("app.cdx.json");

    let document = generate(&project, "cyclonedx", &output);
    let component = document["components"]
        .as_array()
        .expect("CycloneDX document should list components")
        .iter()
        .find(|component| component["name"] == "longlic")
        .expect("longlic is missing from the CycloneDX document");
    assert_eq!(component["licenses"][0]["expression"], LONG_EXPRESSION);

    assert_validates(&output);
}
