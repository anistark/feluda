//! Integration tests for the SBOM serializations beyond SPDX and CycloneDX JSON: SPDX tag:value,
//! SPDX 3.0 JSON-LD and CycloneDX XML, read as `--sbom-input`, enriched, validated and written.
//!
//! Licenses feluda has to resolve come from a ClearlyDefined definitions file, so the enriched
//! copies are exercised without the suite depending on a third party service. The fixture crates
//! do not exist, so a registry asked about them has nothing to say either.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

/// syft's tag:value layout: a package with nothing stated, a GPL one, and a file section whose
/// `SPDXID` must not be taken for the document's. Files listed after a package belong to it, so it
/// comes first.
const TAG_VALUE: &str = "SPDXVersion: SPDX-2.3
DataLicense: CC0-1.0
SPDXID: SPDXRef-DOCUMENT
DocumentName: fixture
DocumentNamespace: https://example.com/fixture-tv
Creator: Tool: syft-1.52.0
Created: 2026-10-07T10:00:00Z

##### File: ./usr/share/doc/readme

FileName: ./usr/share/doc/readme
SPDXID: SPDXRef-File-readme
FileChecksum: SHA1: 85ed0817af83a24ad8da68c2b5094de69833983c
LicenseConcluded: NOASSERTION
FileCopyrightText: NOASSERTION

##### Package: feluda-fixture-tv

PackageName: feluda-fixture-tv
SPDXID: SPDXRef-Package-tv
PackageVersion: 1.0.0
PackageDownloadLocation: NOASSERTION
FilesAnalyzed: false
PackageLicenseConcluded: NOASSERTION
PackageLicenseDeclared: NOASSERTION
PackageCopyrightText: NOASSERTION
ExternalRef: PACKAGE-MANAGER purl pkg:cargo/feluda-fixture-tv@1.0.0

##### Package: readline

PackageName: readline
SPDXID: SPDXRef-Package-readline
PackageVersion: 8.2-1.3
PackageDownloadLocation: NOASSERTION
FilesAnalyzed: false
PackageLicenseDeclared: GPL-3.0-or-later
PackageCopyrightText: <text>Copyright 1989-2022
Free Software Foundation</text>
ExternalRef: PACKAGE-MANAGER purl pkg:deb/debian/readline@8.2-1.3?arch=amd64

##### Relationships

Relationship: SPDXRef-DOCUMENT DESCRIBES SPDXRef-Package-tv
Relationship: SPDXRef-DOCUMENT DESCRIBES SPDXRef-Package-readline
";

/// cdxgen's XML layout at 1.6: one component with nothing stated, one with an expression.
const CYCLONEDX_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<bom xmlns="http://cyclonedx.org/schema/bom/1.6" serialNumber="urn:uuid:3e671687-395b-41f5-a30f-a58921a69b79" version="1">
  <metadata>
    <timestamp>2026-10-07T10:00:00Z</timestamp>
    <tools>
      <components>
        <component type="application">
          <name>cdxgen</name>
          <version>12.3.0</version>
        </component>
      </components>
    </tools>
    <component type="application" bom-ref="app">
      <name>app</name>
    </component>
  </metadata>
  <components>
    <component type="library" bom-ref="pkg:cargo/feluda-fixture-xml@2.0.0">
      <name>feluda-fixture-xml</name>
      <version>2.0.0</version>
      <purl>pkg:cargo/feluda-fixture-xml@2.0.0</purl>
    </component>
    <component type="library" bom-ref="pkg:maven/org.example/copyleft@1.0">
      <group>org.example</group>
      <name>copyleft</name>
      <version>1.0</version>
      <licenses>
        <expression>GPL-2.0-only</expression>
      </licenses>
      <purl>pkg:maven/org.example/copyleft@1.0</purl>
    </component>
  </components>
</bom>
"#;

/// Yocto's SPDX 3.0 layout: licenses as relationships to expression elements, a custom id mapped
/// to a licensing text, and a conclusion of "no assertion" for the package feluda resolves.
const SPDX_3: &str = r#"{
  "@context": "https://spdx.org/rdf/3.0.1/spdx-context.jsonld",
  "@graph": [
    {
      "type": "CreationInfo",
      "@id": "_:CreationInfo1",
      "specVersion": "3.0.1",
      "created": "2026-10-07T10:00:00Z",
      "createdBy": ["http://spdx.org/spdxdocs/image#Agent"]
    },
    {
      "type": "Organization",
      "spdxId": "http://spdx.org/spdxdocs/image#Agent",
      "creationInfo": "_:CreationInfo1",
      "name": "OpenEmbedded"
    },
    {
      "type": "SpdxDocument",
      "spdxId": "http://spdx.org/spdxdocs/image",
      "creationInfo": "_:CreationInfo1",
      "name": "core-image-minimal",
      "rootElement": ["http://spdx.org/spdxdocs/image#busybox"],
      "element": [
        "http://spdx.org/spdxdocs/image#Agent",
        "http://spdx.org/spdxdocs/image#busybox",
        "http://spdx.org/spdxdocs/image#three",
        "http://spdx.org/spdxdocs/image#License-busybox",
        "http://spdx.org/spdxdocs/image#Text-bzip2",
        "http://spdx.org/spdxdocs/image#R1",
        "http://spdx.org/spdxdocs/image#R2"
      ]
    },
    {
      "type": "software_Package",
      "spdxId": "http://spdx.org/spdxdocs/image#busybox",
      "creationInfo": "_:CreationInfo1",
      "name": "busybox",
      "software_packageVersion": "1.36.1",
      "software_packageUrl": "pkg:generic/busybox@1.36.1"
    },
    {
      "type": "software_Package",
      "spdxId": "http://spdx.org/spdxdocs/image#three",
      "creationInfo": "_:CreationInfo1",
      "name": "feluda-fixture-three",
      "software_packageVersion": "3.0.0",
      "software_packageUrl": "pkg:cargo/feluda-fixture-three@3.0.0"
    },
    {
      "type": "simplelicensing_LicenseExpression",
      "spdxId": "http://spdx.org/spdxdocs/image#License-busybox",
      "creationInfo": "_:CreationInfo1",
      "simplelicensing_licenseExpression": "GPL-2.0-only AND LicenseRef-bzip2-1.0.4",
      "simplelicensing_customIdToUri": [{
        "type": "DictionaryEntry",
        "key": "LicenseRef-bzip2-1.0.4",
        "value": "http://spdx.org/spdxdocs/image#Text-bzip2"
      }]
    },
    {
      "type": "simplelicensing_SimpleLicensingText",
      "spdxId": "http://spdx.org/spdxdocs/image#Text-bzip2",
      "creationInfo": "_:CreationInfo1",
      "name": "bzip2-1.0.4",
      "simplelicensing_licenseText": "This program, \"bzip2\", the associated library \"libbzip2\" ..."
    },
    {
      "type": "Relationship",
      "spdxId": "http://spdx.org/spdxdocs/image#R1",
      "creationInfo": "_:CreationInfo1",
      "from": "http://spdx.org/spdxdocs/image#busybox",
      "relationshipType": "hasDeclaredLicense",
      "to": ["http://spdx.org/spdxdocs/image#License-busybox"]
    },
    {
      "type": "Relationship",
      "spdxId": "http://spdx.org/spdxdocs/image#R2",
      "creationInfo": "_:CreationInfo1",
      "from": "http://spdx.org/spdxdocs/image#three",
      "relationshipType": "hasConcludedLicense",
      "to": ["expandedlicensing_NoAssertionLicense"]
    }
  ]
}"#;

/// What a connected machine would have saved from ClearlyDefined for the three fixture crates.
const DEFINITIONS: &str = r#"{
  "crate/cratesio/-/feluda-fixture-tv/1.0.0": "Acme Commercial License",
  "crate/cratesio/-/feluda-fixture-xml/2.0.0": "MIT",
  "crate/cratesio/-/feluda-fixture-three/3.0.0": "BSD-3-Clause"
}"#;

/// Run feluda with answers from the definitions file only, and a cache of its own.
fn feluda(home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let definitions = home.join("definitions.json");
    fs::write(&definitions, DEFINITIONS).expect("failed to write definitions");
    let mut child = Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("FELUDA_CLEARLYDEFINED_DEFINITIONS", &definitions)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn feluda binary");
    let mut input = child.stdin.take().expect("stdin should be piped");
    input
        .write_all(stdin.unwrap_or_default().as_bytes())
        .expect("failed to write stdin");
    drop(input);
    child
        .wait_with_output()
        .expect("failed to run feluda binary")
}

fn write(dir: &Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, content).expect("failed to write fixture");
    path.to_str().expect("UTF-8 path").to_string()
}

fn report(output: &Output) -> Vec<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "expected a JSON report, got {stdout:?}: {e}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn find<'a>(report: &'a [Value], name: &str) -> &'a Value {
    report
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("{name} missing from report: {report:#?}"))
}

/// Run `sbom validate --json`, and require it to find no errors and no warnings.
fn assert_validates(home: &Path, path: &str, sbom_type: &str) {
    let output = feluda(home, &["sbom", "validate", path, "--json"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let validation: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "validate emitted invalid JSON: {e}\n{stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(validation["sbom_type"], sbom_type, "{validation:#}");
    assert_eq!(validation["error_count"], 0, "{validation:#}");
    assert_eq!(validation["warning_count"], 0, "{validation:#}");
}

#[test]
fn tag_value_is_ingested_and_enriched_in_place() {
    let temp = tempfile::tempdir().unwrap();
    let input = write(temp.path(), "fixture.spdx", TAG_VALUE);
    let enriched = temp.path().join("enriched.spdx");

    let output = feluda(
        temp.path(),
        &[
            "--sbom-input",
            &input,
            "--json",
            "--sbom-enriched",
            enriched.to_str().unwrap(),
        ],
        None,
    );
    let report = report(&output);
    assert_eq!(report.len(), 2, "{report:#?}");
    let readline = find(&report, "debian/readline");
    assert_eq!(readline["license"], "GPL-3.0-or-later");
    assert_eq!(readline["is_restrictive"], true);
    assert_eq!(
        find(&report, "feluda-fixture-tv")["license"],
        "Acme Commercial License"
    );

    // Tag:value in, tag:value out: the conclusion replaces NOASSERTION on its own line, the
    // license title is defined as a ref at the end, and nothing else moves.
    let written = fs::read_to_string(&enriched).unwrap();
    let license_ref = "LicenseRef-feluda-Acme-Commercial-License";
    assert!(
        written.contains(&format!(
            "PackageLicenseConcluded: {license_ref}\nPackageLicenseDeclared: NOASSERTION"
        )),
        "{written}"
    );
    assert!(written.contains(&format!("LicenseID: {license_ref}\n")));
    assert!(written.starts_with(&TAG_VALUE[..TAG_VALUE.find("PackageLicenseConcluded").unwrap()]));
    let tail = &TAG_VALUE[TAG_VALUE
        .find("PackageLicenseDeclared: NOASSERTION")
        .unwrap()..];
    assert!(written.contains(tail.trim_end()));

    assert_validates(temp.path(), enriched.to_str().unwrap(), "SPDX tag:value");
}

#[test]
fn tag_value_arrives_on_stdin_and_fails_the_gate() {
    let temp = tempfile::tempdir().unwrap();
    let output = feluda(
        temp.path(),
        &["--sbom-input", "-", "--json", "--fail-on-restrictive"],
        Some(TAG_VALUE),
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cyclonedx_xml_is_ingested_and_enriched_in_place() {
    let temp = tempfile::tempdir().unwrap();
    let enriched = temp.path().join("enriched.cdx.xml");

    let output = feluda(
        temp.path(),
        &[
            "--sbom-input",
            "-",
            "--json",
            "--sbom-enriched",
            enriched.to_str().unwrap(),
        ],
        Some(CYCLONEDX_XML),
    );
    let report = report(&output);
    // The metadata component is what was described, not one of its dependencies.
    assert_eq!(report.len(), 2, "{report:#?}");
    let copyleft = find(&report, "org.example:copyleft");
    assert_eq!(copyleft["license"], "GPL-2.0-only");
    assert_eq!(copyleft["is_restrictive"], true);
    assert_eq!(find(&report, "feluda-fixture-xml")["license"], "MIT");

    // XML in, XML out: the new licenses sit where the schema orders them, before `<purl>`, and
    // say they were concluded, which 1.6 lets them.
    let written = fs::read_to_string(&enriched).unwrap();
    assert!(
        written.contains(
            "<version>2.0.0</version>\n      <licenses><license acknowledgement=\"concluded\"><id>MIT</id></license></licenses>\n      <purl>"
        ),
        "{written}"
    );
    assert_eq!(
        written.replace(
            "      <licenses><license acknowledgement=\"concluded\"><id>MIT</id></license></licenses>\n",
            ""
        ),
        CYCLONEDX_XML
    );

    assert_validates(temp.path(), enriched.to_str().unwrap(), "CycloneDX XML");
}

#[test]
fn spdx_3_is_ingested_and_enriched_with_relationships() {
    let temp = tempfile::tempdir().unwrap();
    let input = write(temp.path(), "image.spdx.json", SPDX_3);
    let enriched = temp.path().join("enriched.spdx.json");

    let output = feluda(
        temp.path(),
        &[
            "--sbom-input",
            &input,
            "--json",
            "--sbom-enriched",
            enriched.to_str().unwrap(),
        ],
        None,
    );
    let report = report(&output);
    assert_eq!(report.len(), 2, "{report:#?}");
    // The custom id's text is not one feluda recognises, so its name stands in for it.
    let busybox = find(&report, "busybox");
    assert_eq!(busybox["license"], "GPL-2.0-only AND bzip2-1.0.4");
    assert_eq!(busybox["is_restrictive"], true);
    assert_eq!(
        find(&report, "feluda-fixture-three")["license"],
        "BSD-3-Clause"
    );

    // The "no assertion" conclusion now points at a license element feluda added.
    let written: Value = serde_json::from_str(&fs::read_to_string(&enriched).unwrap()).unwrap();
    let graph = written["@graph"].as_array().unwrap();
    let relationship = graph
        .iter()
        .find(|element| element["spdxId"] == "http://spdx.org/spdxdocs/image#R2")
        .unwrap();
    let target = relationship["to"][0].as_str().unwrap();
    let expression = graph
        .iter()
        .find(|element| element["spdxId"] == target)
        .unwrap();
    assert_eq!(
        expression["simplelicensing_licenseExpression"],
        "BSD-3-Clause"
    );
    assert_eq!(relationship["creationInfo"], "_:feluda-creationinfo");

    assert_validates(temp.path(), &input, "SPDX 3.0");
    assert_validates(temp.path(), enriched.to_str().unwrap(), "SPDX 3.0");
}

/// A Node project whose dependencies state their licenses in `node_modules`, so they resolve
/// locally: a listed id, a registry style title, and an expression with an unlisted id in it.
fn node_project(dir: &Path) {
    let dependencies = [
        ("listed", "MIT"),
        ("titled", "The Apache Software License, Version 2.0"),
        ("mixed", "Custom-1.0 OR MIT"),
    ];
    let manifest = serde_json::json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": dependencies
            .iter()
            .map(|(name, _)| (name.to_string(), Value::from("1.0.0")))
            .collect::<serde_json::Map<_, _>>(),
    });
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
    for (name, license) in dependencies {
        let package = dir.join("node_modules").join(name);
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            serde_json::json!({ "name": name, "version": "1.0.0", "license": license }).to_string(),
        )
        .unwrap();
    }
}

#[test]
fn spdx_is_written_as_tag_value_and_reads_back() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("app");
    node_project(&project);

    for (version, expected) in [
        ("2.3", "SPDXVersion: SPDX-2.3\n"),
        ("2.2", "SPDXVersion: SPDX-2.2\n"),
    ] {
        let output_base = temp.path().join(format!("app-{version}"));
        let output = feluda(
            temp.path(),
            &[
                "sbom",
                "spdx",
                "--path",
                project.to_str().unwrap(),
                "--format",
                "tag-value",
                "--spec-version",
                version,
                "--output",
                output_base.to_str().unwrap(),
            ],
            None,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // `.spdx` is tag:value's extension, as `.spdx.json` is JSON's.
        let path = format!("{}.spdx", output_base.display());
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with(expected), "{written}");
        assert!(written.contains(
            "PackageLicenseConcluded: LicenseRef-feluda-The-Apache-Software-License-Version-2.0\n"
        ));
        assert!(written.contains("PackageLicenseDeclared: LicenseRef-feluda-Custom-1.0 OR MIT\n"));
        assert_validates(temp.path(), &path, "SPDX tag:value");

        // Read back as an input, the licenses are the ones the project stated.
        let report = report(&feluda(
            temp.path(),
            &["--sbom-input", &path, "--json"],
            None,
        ));
        assert_eq!(
            find(&report, "titled")["license"],
            "The Apache Software License, Version 2.0"
        );
        assert_eq!(find(&report, "mixed")["license"], "Custom-1.0 OR MIT");
    }
}

#[test]
fn spdx_3_is_written_and_reads_back() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("app");
    node_project(&project);
    let path = temp.path().join("app.spdx.json");

    let output = feluda(
        temp.path(),
        &[
            "sbom",
            "--spdx-version",
            "3.0",
            "spdx",
            "--path",
            project.to_str().unwrap(),
            "--output",
            path.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        written["@context"],
        "https://spdx.org/rdf/3.0.1/spdx-context.jsonld"
    );
    assert_eq!(written["@graph"][0]["specVersion"], "3.0.1");
    assert_validates(temp.path(), path.to_str().unwrap(), "SPDX 3.0");

    let report = report(&feluda(
        temp.path(),
        &["--sbom-input", path.to_str().unwrap(), "--json"],
        None,
    ));
    assert_eq!(report.len(), 3, "{report:#?}");
    assert_eq!(find(&report, "listed")["license"], "MIT");
    assert_eq!(
        find(&report, "titled")["license"],
        "The Apache Software License, Version 2.0"
    );
    assert_eq!(find(&report, "mixed")["license"], "Custom-1.0 OR MIT");
}

#[test]
fn spdx_3_has_no_tag_value() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("app");
    node_project(&project);

    let output = feluda(
        temp.path(),
        &[
            "sbom",
            "spdx",
            "--path",
            project.to_str().unwrap(),
            "--spec-version",
            "3.0",
            "--format",
            "tag-value",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("SPDX 3.0 has no tag:value serialization")
    );
}

#[test]
fn xml_that_is_not_cyclonedx_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let output = feluda(
        temp.path(),
        &["--sbom-input", "-", "--json"],
        Some(r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"/>"#),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("only read as CycloneDX"));
}

#[test]
fn cyclonedx_is_written_as_xml_and_reads_back() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("app");
    node_project(&project);

    for version in ["1.4", "1.6"] {
        let output_base = temp.path().join(format!("app-{version}"));
        let output = feluda(
            temp.path(),
            &[
                "sbom",
                "cyclonedx",
                "--path",
                project.to_str().unwrap(),
                "--spec-version",
                version,
                "--format",
                "xml",
                "--output",
                output_base.to_str().unwrap(),
            ],
            None,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let path = format!("{}.cyclonedx.xml", output_base.display());
        let written = fs::read_to_string(&path).unwrap();
        assert!(
            written.contains(&format!(
                "<bom xmlns=\"http://cyclonedx.org/schema/bom/{version}\""
            )),
            "{written}"
        );
        // A title is a name, never an id, and 1.6 says the license was declared.
        let acknowledgement = if version == "1.6" {
            " acknowledgement=\"declared\""
        } else {
            ""
        };
        assert!(
            written.contains(&format!(
                "<license{acknowledgement}><name>The Apache Software License, Version 2.0</name></license>"
            )),
            "{written}"
        );
        assert_validates(temp.path(), &path, "CycloneDX XML");

        let report = report(&feluda(
            temp.path(),
            &["--sbom-input", &path, "--json"],
            None,
        ));
        assert_eq!(report.len(), 3, "{report:#?}");
        assert_eq!(find(&report, "listed")["license"], "MIT");
        assert_eq!(find(&report, "mixed")["license"], "Custom-1.0 OR MIT");
    }
}

#[test]
fn sboms_say_what_they_were_made_from() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("app");
    node_project(&project);
    // The smallest root filesystem the catalogers recognise: one Alpine package.
    let rootfs = temp.path().join("rootfs");
    fs::create_dir_all(rootfs.join("lib/apk/db")).unwrap();
    fs::create_dir_all(rootfs.join("etc")).unwrap();
    fs::write(
        rootfs.join("etc/os-release"),
        "ID=alpine\nVERSION_ID=3.20.0\n",
    )
    .unwrap();
    fs::write(
        rootfs.join("lib/apk/db/installed"),
        "P:musl\nV:1.2.5-r0\nA:x86_64\nL:MIT\n\n",
    )
    .unwrap();

    // SPDX 3.0's `software_sbomType` and CycloneDX's lifecycle phase name the same thing.
    for (source, sbom_type, phase) in [
        (["--path", project.to_str().unwrap()], "source", "pre-build"),
        (
            ["--filesystem", rootfs.to_str().unwrap()],
            "analyzed",
            "post-build",
        ),
    ] {
        let generate = |args: &[&str], path: &Path| {
            let mut args = args.to_vec();
            args.extend(["--output", path.to_str().unwrap()]);
            args.extend(source);
            let output = feluda(temp.path(), &args, None);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            fs::read_to_string(path).unwrap()
        };

        let path = temp.path().join(format!("{sbom_type}.spdx.json"));
        let written: Value =
            serde_json::from_str(&generate(&["sbom", "spdx", "--spec-version", "3.0"], &path))
                .unwrap();
        let sbom = written["@graph"]
            .as_array()
            .unwrap()
            .iter()
            .find(|element| element["type"] == "software_Sbom")
            .unwrap();
        assert_eq!(sbom["software_sbomType"], serde_json::json!([sbom_type]));
        assert_validates(temp.path(), path.to_str().unwrap(), "SPDX 3.0");

        let path = temp.path().join(format!("{sbom_type}.cyclonedx.json"));
        let written: Value =
            serde_json::from_str(&generate(&["sbom", "cyclonedx"], &path)).unwrap();
        assert_eq!(
            written["metadata"]["lifecycles"],
            serde_json::json!([{ "phase": phase }])
        );
        assert_validates(temp.path(), path.to_str().unwrap(), "CycloneDX");

        let path = temp.path().join(format!("{sbom_type}.cyclonedx.xml"));
        let written = generate(&["sbom", "cyclonedx", "--format", "xml"], &path);
        assert!(
            written.contains(&format!(
                "<lifecycles>\n      <lifecycle>\n        <phase>{phase}</phase>"
            )),
            "{written}"
        );
        assert_validates(temp.path(), path.to_str().unwrap(), "CycloneDX XML");

        // 1.4 has no lifecycles.
        let path = temp.path().join(format!("{sbom_type}-1.4.cyclonedx.json"));
        let written: Value = serde_json::from_str(&generate(
            &["sbom", "cyclonedx", "--spec-version", "1.4"],
            &path,
        ))
        .unwrap();
        assert!(written["metadata"].get("lifecycles").is_none());
    }
}
