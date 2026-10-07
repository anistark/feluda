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

/// Run `feluda` from `dir`, which is where it looks for `.feluda.toml`.
fn feluda_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .current_dir(dir)
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .env_remove("FELUDA_SBOM_SPDX")
        .env_remove("FELUDA_SBOM_CYCLONEDX")
        .args(args)
        .output()
        .expect("failed to run feluda binary")
}

/// Generate an SBOM from inside `dir` and return the parsed document.
fn generate_in(dir: &Path, args: &[&str], output: &Path) -> Value {
    let mut args = args.to_vec();
    args.extend(["--path", "app", "--output", output.to_str().unwrap()]);
    let result = feluda_in(dir, &args);
    assert!(
        result.status.success(),
        "{args:?} failed, stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_str(&fs::read_to_string(output).expect("SBOM should be written"))
        .expect("SBOM should be JSON")
}

/// feluda's validator accepts the document with no errors and no doubt about its version.
fn assert_validates_as_current(path: &Path) {
    assert_validates(path);
    let result = feluda(&["sbom", "validate", path.to_str().unwrap(), "--json"]);
    let report = String::from_utf8_lossy(&result.stdout);
    assert!(
        !report.contains("specVersion") && !report.contains("may not be fully supported"),
        "{report}"
    );
}

#[test]
fn spdx_is_written_in_the_version_asked_for() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    node_project(&temp.path().join("app"), "MIT");

    for (version, category) in [("2.2", "PACKAGE_MANAGER"), ("2.3", "PACKAGE-MANAGER")] {
        let output = temp.path().join(format!("app-{version}.spdx.json"));
        let document = generate_in(
            temp.path(),
            &["sbom", "spdx", "--spec-version", version],
            &output,
        );

        assert_eq!(document["spdxVersion"], format!("SPDX-{version}"));
        let package = document["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "longlic")
            .expect("longlic is missing from the SPDX document");
        assert_eq!(package["externalRefs"][0]["referenceCategory"], category);
        assert_eq!(package["licenseDeclared"], "MIT");
        assert_validates_as_current(&output);
    }
}

#[test]
fn cyclonedx_is_written_in_the_version_asked_for() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    node_project(&temp.path().join("app"), "MIT");

    for version in ["1.4", "1.5", "1.6", "1.7"] {
        let output = temp.path().join(format!("app-{version}.cdx.json"));
        let document = generate_in(
            temp.path(),
            &["sbom", "cyclonedx", "--spec-version", version],
            &output,
        );

        assert_eq!(document["specVersion"], version);
        let tools = &document["metadata"]["tools"];
        let license = &document["components"][0]["licenses"][0]["license"];
        assert_eq!(license["id"], "MIT");
        if version == "1.4" {
            assert_eq!(tools[0]["name"], "feluda");
        } else {
            assert_eq!(tools["components"][0]["name"], "feluda");
        }
        if version >= "1.6" {
            assert_eq!(license["acknowledgement"], "declared");
        } else {
            assert!(license.get("acknowledgement").is_none());
        }
        assert_validates_as_current(&output);
    }
}

#[test]
fn cyclonedx_defaults_to_1_6() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    node_project(&temp.path().join("app"), "MIT");
    let output = temp.path().join("app.cdx.json");

    let document = generate_in(temp.path(), &["sbom", "cyclonedx"], &output);
    assert_eq!(document["specVersion"], "1.6");
}

#[test]
fn versions_come_from_the_config_unless_a_flag_names_one() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    node_project(&temp.path().join("app"), "MIT");
    fs::write(
        temp.path().join(".feluda.toml"),
        "[sbom]\nspdx = \"2.2\"\ncyclonedx = \"1.4\"\n",
    )
    .unwrap();

    // Both formats at once take both configured versions.
    let output = temp.path().join("both");
    let result = feluda_in(
        temp.path(),
        &[
            "sbom",
            "--path",
            "app",
            "--output",
            output.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let read = |name: &str| -> Value {
        serde_json::from_str(&fs::read_to_string(temp.path().join(name)).unwrap()).unwrap()
    };
    assert_eq!(read("both.spdx.json")["spdxVersion"], "SPDX-2.2");
    assert_eq!(read("both.cyclonedx.json")["specVersion"], "1.4");

    // A flag beats the config, on the parent command and on the format subcommand alike.
    let output = temp.path().join("flag.cdx.json");
    let document = generate_in(
        temp.path(),
        &["sbom", "--cyclonedx-version", "1.5", "cyclonedx"],
        &output,
    );
    assert_eq!(document["specVersion"], "1.5");
    let document = generate_in(
        temp.path(),
        &["sbom", "cyclonedx", "--spec-version", "1.7"],
        &output,
    );
    assert_eq!(document["specVersion"], "1.7");
}

#[test]
fn unsupported_versions_are_refused_with_a_reason() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    node_project(&temp.path().join("app"), "MIT");

    let result = feluda_in(
        temp.path(),
        &[
            "sbom",
            "cyclonedx",
            "--spec-version",
            "1.3",
            "--path",
            "app",
        ],
    );
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("possible values: 1.4, 1.5, 1.6, 1.7"),
        "{stderr}"
    );

    fs::write(temp.path().join(".feluda.toml"), "[sbom]\nspdx = \"3.0\"\n").unwrap();
    let result = feluda_in(temp.path(), &["sbom", "spdx", "--path", "app"]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("❌") && stderr.contains("unsupported SPDX version '3.0'"),
        "{stderr}"
    );
}

#[test]
fn a_free_form_license_is_a_cyclonedx_name_not_an_id() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    node_project(&temp.path().join("app"), "SEE LICENSE IN LICENSE.txt");
    let output = temp.path().join("app.cdx.json");

    let document = generate_in(temp.path(), &["sbom", "cyclonedx"], &output);
    let license = &document["components"][0]["licenses"][0]["license"];
    assert!(license.get("id").is_none(), "{license}");
    assert_eq!(license["name"], "SEE LICENSE IN LICENSE.txt");

    let result = feluda(&["sbom", "validate", output.to_str().unwrap(), "--json"]);
    let report = String::from_utf8_lossy(&result.stdout);
    assert!(!report.contains("is not an SPDX license id"), "{report}");
    assert_validates(&output);
}

#[test]
fn a_free_form_license_is_an_spdx_license_ref() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    node_project(&temp.path().join("app"), "SEE LICENSE IN LICENSE.txt");

    for version in ["2.2", "2.3"] {
        let output = temp.path().join(format!("app-{version}.spdx.json"));
        let document = generate_in(
            temp.path(),
            &["sbom", "spdx", "--spec-version", version],
            &output,
        );

        let license_ref = "LicenseRef-feluda-SEE-LICENSE-IN-LICENSE.txt";
        let package = &document["packages"][0];
        assert_eq!(package["licenseDeclared"], license_ref);
        assert_eq!(package["licenseConcluded"], license_ref);
        let extracted = &document["hasExtractedLicensingInfos"][0];
        assert_eq!(extracted["licenseId"], license_ref);
        assert_eq!(extracted["extractedText"], "SEE LICENSE IN LICENSE.txt");

        let result = feluda(&["sbom", "validate", output.to_str().unwrap(), "--json"]);
        let report = String::from_utf8_lossy(&result.stdout);
        assert!(
            !report.contains("LicenseRef") && !report.contains("not an SPDX"),
            "{report}"
        );
        assert_validates_as_current(&output);
    }
}

#[test]
fn a_license_title_with_commas_survives_into_both_formats() {
    // Maven Central's titles have commas, which no SPDX expression allows. The title is still
    // the license, so it must not come out as NOASSERTION.
    let title = "The Apache Software License, Version 2.0";
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    node_project(&temp.path().join("app"), title);

    let spdx_output = temp.path().join("app.spdx.json");
    let spdx = generate_in(temp.path(), &["sbom", "spdx"], &spdx_output);
    let license_ref = "LicenseRef-feluda-The-Apache-Software-License-Version-2.0";
    assert_eq!(spdx["packages"][0]["licenseDeclared"], license_ref);
    assert_eq!(
        spdx["hasExtractedLicensingInfos"][0]["extractedText"],
        title
    );
    assert_validates_as_current(&spdx_output);

    let cdx_output = temp.path().join("app.cdx.json");
    let cdx = generate_in(temp.path(), &["sbom", "cyclonedx"], &cdx_output);
    assert_eq!(
        cdx["components"][0]["licenses"][0]["license"]["name"],
        title
    );
    assert_validates_as_current(&cdx_output);
}
