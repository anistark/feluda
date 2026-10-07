//! Integration tests for the ClearlyDefined fallback.
//!
//! The lookup is pointed at a stub server on localhost through `FELUDA_CLEARLYDEFINED_ENDPOINT`,
//! so the whole path runs for real (coordinates out, declared licenses back, findings reclassified)
//! without the suite depending on a third party service being up. Findings arrive through
//! `--sbom-input`, which is the shortest way to hand feluda packages it cannot resolve.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;

use serde_json::Value;

/// Two components with no license, one of which ClearlyDefined will answer for.
const SBOM: &str = r#"{
  "spdxVersion": "SPDX-2.3",
  "SPDXID": "SPDXRef-DOCUMENT",
  "name": "fixture",
  "documentNamespace": "https://example.com/fixture",
  "creationInfo": {"created": "2026-01-01T00:00:00Z", "creators": ["Tool: fixture"]},
  "packages": [
    {
      "SPDXID": "SPDXRef-1",
      "name": "mystery",
      "versionInfo": "1.2.3",
      "licenseConcluded": "NOASSERTION",
      "licenseDeclared": "NOASSERTION",
      "externalRefs": [{
        "referenceCategory": "PACKAGE-MANAGER",
        "referenceType": "purl",
        "referenceLocator": "pkg:cargo/mystery@1.2.3"
      }]
    },
    {
      "SPDXID": "SPDXRef-2",
      "name": "copyleft-mystery",
      "versionInfo": "4.5.6",
      "licenseConcluded": "NOASSERTION",
      "licenseDeclared": "NOASSERTION",
      "externalRefs": [{
        "referenceCategory": "PACKAGE-MANAGER",
        "referenceType": "purl",
        "referenceLocator": "pkg:cargo/copyleft-mystery@4.5.6"
      }]
    }
  ]
}"#;

const RESPONSE: &str = r#"{
  "crate/cratesio/-/mystery/1.2.3": {
    "described": {"releaseDate": "2026-01-01"},
    "licensed": {"declared": "Apache-2.0"},
    "scores": {"effective": 80}
  },
  "crate/cratesio/-/copyleft-mystery/4.5.6": {
    "licensed": {"declared": "GPL-3.0"},
    "scores": {"effective": 70}
  }
}"#;

/// A stub of the definitions endpoint, serving one canned response and reporting back the request
/// body it was sent so the coordinates can be asserted on.
struct Stub {
    endpoint: String,
    requests: mpsc::Receiver<String>,
}

impl Stub {
    fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind stub server");
        let endpoint = format!(
            "http://{}/definitions",
            listener.local_addr().expect("stub server has no address")
        );
        let (sender, requests) = mpsc::channel();

        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let Some(request) = serve(stream, body) else {
                    continue;
                };
                if sender.send(request).is_err() {
                    break;
                }
            }
        });

        Self { endpoint, requests }
    }

    fn request_body(&self) -> String {
        self.requests
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("stub server received no request")
    }
}

/// Read one HTTP request and answer it. Returns the request body.
fn serve(stream: TcpStream, body: &str) -> Option<String> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().ok()?;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }

    let mut request = vec![0u8; content_length];
    reader.read_exact(&mut request).ok()?;

    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    stream.flush().ok()?;

    String::from_utf8(request).ok()
}

/// `home` isolates the run's cache directory. Answers are cached, so without this a second run of
/// the suite would be served from the developer's own cache and never reach the stub.
fn feluda(
    home: &std::path::Path,
    sbom: &str,
    endpoint: Option<&str>,
    extra_args: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_feluda"));
    command
        .args(["--sbom-input", sbom, "--json"])
        .args(extra_args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"));
    match endpoint {
        Some(endpoint) => command.env("FELUDA_CLEARLYDEFINED_ENDPOINT", endpoint),
        None => command.env("FELUDA_CLEARLYDEFINED_ENABLED", "false"),
    };
    command.output().expect("failed to run feluda binary")
}

fn report(output: &Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("feluda emitted invalid JSON: {e}\n{stdout}"))
}

fn entry<'a>(entries: &'a [Value], name: &str) -> &'a Value {
    entries
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("no entry named {name:?} in report: {entries:#?}"))
}

fn write_sbom(dir: &std::path::Path) -> String {
    let path = dir.join("fixture.spdx.json");
    std::fs::write(&path, SBOM).expect("failed to write fixture SBOM");
    path.to_string_lossy().to_string()
}

#[test]
fn unresolved_licenses_are_filled_in_and_reclassified() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = write_sbom(temp.path());
    let stub = Stub::start(RESPONSE);

    let output = feluda(temp.path(), &sbom, Some(&stub.endpoint), &[]);
    let entries = report(&output);

    let permissive = entry(&entries, "mystery");
    assert_eq!(permissive["license"], "Apache-2.0");
    assert_eq!(permissive["is_restrictive"], false);
    assert_eq!(permissive["osi_status"], "Approved");

    // The point of reclassifying: a license that arrives from ClearlyDefined has to reach the gate
    // the same way one from a manifest does.
    let restrictive = entry(&entries, "copyleft-mystery");
    assert_eq!(restrictive["license"], "GPL-3.0");
    assert_eq!(restrictive["is_restrictive"], true);
}

#[test]
fn packages_are_asked_about_by_coordinate() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = write_sbom(temp.path());
    let stub = Stub::start(RESPONSE);

    feluda(temp.path(), &sbom, Some(&stub.endpoint), &[]);

    let requested: Vec<String> =
        serde_json::from_str(&stub.request_body()).expect("stub received invalid JSON");
    assert!(requested.contains(&"crate/cratesio/-/mystery/1.2.3".to_string()));
    assert!(requested.contains(&"crate/cratesio/-/copyleft-mystery/4.5.6".to_string()));
}

#[test]
fn a_resolved_license_fails_the_restrictive_gate() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = write_sbom(temp.path());
    let stub = Stub::start(RESPONSE);

    let output = feluda(
        temp.path(),
        &sbom,
        Some(&stub.endpoint),
        &["--fail-on-restrictive"],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_lookup_can_be_turned_off() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = write_sbom(temp.path());

    for entries in [
        report(&feluda(temp.path(), &sbom, None, &[])),
        report(&feluda(temp.path(), &sbom, None, &["--no-clearlydefined"])),
    ] {
        assert!(entry(&entries, "mystery")["license"].is_null());
        assert!(entry(&entries, "copyleft-mystery")["license"].is_null());
    }
}

#[test]
fn a_definitions_file_answers_instead_of_the_service() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = write_sbom(temp.path());
    // The file holds the stub's response verbatim, which is what a curl against the real batch
    // endpoint on a connected machine would have saved.
    let definitions = temp.path().join("clearlydefined.json");
    std::fs::write(&definitions, RESPONSE).expect("failed to write definitions file");
    // A stub is still listening so that a request reaching it can be told apart from no request.
    let stub = Stub::start(RESPONSE);

    let mut command = Command::new(env!("CARGO_BIN_EXE_feluda"));
    command
        .args(["--sbom-input", &sbom, "--json"])
        .env("HOME", temp.path())
        .env("XDG_CACHE_HOME", temp.path().join("cache"))
        .env("FELUDA_CLEARLYDEFINED_ENDPOINT", &stub.endpoint)
        .env("FELUDA_CLEARLYDEFINED_DEFINITIONS", &definitions);
    let output = command.output().expect("failed to run feluda binary");
    let entries = report(&output);

    assert_eq!(entry(&entries, "mystery")["license"], "Apache-2.0");
    let restrictive = entry(&entries, "copyleft-mystery");
    assert_eq!(restrictive["license"], "GPL-3.0");
    assert_eq!(restrictive["is_restrictive"], true);

    assert!(
        stub.requests
            .recv_timeout(std::time::Duration::from_millis(500))
            .is_err(),
        "the service was asked although a definitions file was configured"
    );
    assert!(
        !temp
            .path()
            .join("cache/feluda/clearlydefined.json")
            .exists(),
        "answers from a file must not be written to the cache"
    );
}

/// One component per way an update can learn about a package: ClearlyDefined answers the first,
/// has never heard of the second, and the third already states its license in the document.
const UPDATE_SBOM: &str = r#"{
  "spdxVersion": "SPDX-2.3",
  "SPDXID": "SPDXRef-DOCUMENT",
  "name": "fixture",
  "documentNamespace": "https://example.com/fixture",
  "creationInfo": {"created": "2026-01-01T00:00:00Z", "creators": ["Tool: fixture"]},
  "packages": [
    {
      "SPDXID": "SPDXRef-1",
      "name": "feluda-fixture-answered",
      "versionInfo": "1.0.0",
      "licenseConcluded": "NOASSERTION",
      "licenseDeclared": "NOASSERTION",
      "externalRefs": [{
        "referenceCategory": "PACKAGE-MANAGER",
        "referenceType": "purl",
        "referenceLocator": "pkg:cargo/feluda-fixture-answered@1.0.0"
      }]
    },
    {
      "SPDXID": "SPDXRef-2",
      "name": "feluda-fixture-unknown",
      "versionInfo": "2.0.0",
      "licenseConcluded": "NOASSERTION",
      "licenseDeclared": "NOASSERTION",
      "externalRefs": [{
        "referenceCategory": "PACKAGE-MANAGER",
        "referenceType": "purl",
        "referenceLocator": "pkg:cargo/feluda-fixture-unknown@2.0.0"
      }]
    },
    {
      "SPDXID": "SPDXRef-3",
      "name": "feluda-fixture-stated",
      "versionInfo": "3.0.0",
      "licenseConcluded": "MIT",
      "licenseDeclared": "MIT",
      "externalRefs": [{
        "referenceCategory": "PACKAGE-MANAGER",
        "referenceType": "purl",
        "referenceLocator": "pkg:cargo/feluda-fixture-stated@3.0.0"
      }]
    }
  ]
}"#;

const UPDATE_RESPONSE: &str = r#"{
  "crate/cratesio/-/feluda-fixture-answered/1.0.0": {
    "licensed": {"declared": "Apache-2.0"}
  },
  "crate/cratesio/-/feluda-fixture-unknown/2.0.0": {
    "licensed": {"toolScore": {"total": 0}}
  }
}"#;

fn feluda_with_definitions(
    home: &std::path::Path,
    sbom: &str,
    endpoint: &str,
    definitions: &std::path::Path,
    extra_args: &[&str],
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(["--sbom-input", sbom, "--json"])
        .args(extra_args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("FELUDA_CLEARLYDEFINED_ENDPOINT", endpoint)
        .env("FELUDA_CLEARLYDEFINED_DEFINITIONS", definitions)
        .output()
        .expect("failed to run feluda binary")
}

fn read_definitions(path: &std::path::Path) -> serde_json::Map<String, Value> {
    let contents = std::fs::read_to_string(path).expect("definitions file was not written");
    serde_json::from_str(&contents).expect("definitions file is not a JSON object")
}

#[test]
fn an_update_records_what_a_connected_run_resolved() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = temp.path().join("fixture.spdx.json");
    std::fs::write(&sbom, UPDATE_SBOM).expect("failed to write fixture SBOM");
    let definitions = temp.path().join("clearlydefined.json");
    let stub = Stub::start(UPDATE_RESPONSE);

    let output = feluda_with_definitions(
        temp.path(),
        &sbom.to_string_lossy(),
        &stub.endpoint,
        &definitions,
        &["--update-definitions"],
    );
    let entries = report(&output);
    assert_eq!(
        entry(&entries, "feluda-fixture-answered")["license"],
        "Apache-2.0"
    );

    // The file did not exist, so the service was asked, and only about what was unresolved.
    let requested: Vec<String> =
        serde_json::from_str(&stub.request_body()).expect("stub received invalid JSON");
    assert!(!requested.contains(&"crate/cratesio/-/feluda-fixture-stated/3.0.0".to_string()));

    let recorded = read_definitions(&definitions);
    assert_eq!(
        recorded["crate/cratesio/-/feluda-fixture-answered/1.0.0"],
        "Apache-2.0"
    );
    // A package the service has never heard of is left as a placeholder to fill in by hand.
    assert_eq!(
        recorded["crate/cratesio/-/feluda-fixture-unknown/2.0.0"],
        "NOASSERTION"
    );
    // A license another tier resolved is recorded too: offline, that tier may not answer.
    assert_eq!(
        recorded["crate/cratesio/-/feluda-fixture-stated/3.0.0"],
        "MIT"
    );
    assert_eq!(recorded.len(), 3);
}

#[test]
fn an_update_keeps_what_the_file_already_says() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = temp.path().join("fixture.spdx.json");
    std::fs::write(&sbom, UPDATE_SBOM).expect("failed to write fixture SBOM");
    let definitions = temp.path().join("clearlydefined.json");
    // A correction someone made by hand, and a placeholder someone has since filled in.
    std::fs::write(
        &definitions,
        r#"{
          "crate/cratesio/-/feluda-fixture-answered/1.0.0": "BSD-3-Clause",
          "crate/cratesio/-/feluda-fixture-unknown/2.0.0": "ISC",
          "npm/npmjs/-/unrelated/1.0.0": "MIT"
        }"#,
    )
    .expect("failed to write definitions file");
    let stub = Stub::start(UPDATE_RESPONSE);

    let output = feluda_with_definitions(
        temp.path(),
        &sbom.to_string_lossy(),
        &stub.endpoint,
        &definitions,
        &["--update-definitions"],
    );
    let entries = report(&output);
    assert_eq!(
        entry(&entries, "feluda-fixture-answered")["license"],
        "BSD-3-Clause"
    );
    assert_eq!(entry(&entries, "feluda-fixture-unknown")["license"], "ISC");

    // Everything unresolved was answered by the file, so the service had nothing to be asked.
    assert!(
        stub.requests
            .recv_timeout(std::time::Duration::from_millis(500))
            .is_err(),
        "the service was asked about coordinates the file already answers"
    );

    let recorded = read_definitions(&definitions);
    assert_eq!(
        recorded["crate/cratesio/-/feluda-fixture-answered/1.0.0"],
        "BSD-3-Clause"
    );
    assert_eq!(
        recorded["crate/cratesio/-/feluda-fixture-unknown/2.0.0"],
        "ISC"
    );
    assert_eq!(recorded["npm/npmjs/-/unrelated/1.0.0"], "MIT");
    assert_eq!(
        recorded["crate/cratesio/-/feluda-fixture-stated/3.0.0"],
        "MIT"
    );
}

#[test]
fn an_updated_file_answers_a_run_without_the_service() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = temp.path().join("fixture.spdx.json");
    std::fs::write(&sbom, UPDATE_SBOM).expect("failed to write fixture SBOM");
    let definitions = temp.path().join("clearlydefined.json");
    let stub = Stub::start(UPDATE_RESPONSE);
    let sbom = sbom.to_string_lossy().to_string();

    report(&feluda_with_definitions(
        temp.path(),
        &sbom,
        &stub.endpoint,
        &definitions,
        &["--update-definitions"],
    ));
    stub.request_body();

    // The air gapped half: a fresh cache and an endpoint nothing listens on.
    let offline = temp.path().join("offline");
    std::fs::create_dir(&offline).expect("failed to create offline home");
    let entries = report(&feluda_with_definitions(
        &offline,
        &sbom,
        "http://127.0.0.1:9/definitions",
        &definitions,
        &[],
    ));
    assert_eq!(
        entry(&entries, "feluda-fixture-answered")["license"],
        "Apache-2.0"
    );
    assert!(entry(&entries, "feluda-fixture-unknown")["license"].is_null());
}

#[test]
fn an_unreadable_file_is_not_overwritten() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = temp.path().join("fixture.spdx.json");
    std::fs::write(&sbom, UPDATE_SBOM).expect("failed to write fixture SBOM");
    let definitions = temp.path().join("clearlydefined.json");
    std::fs::write(&definitions, "{ half written").expect("failed to write definitions file");
    let stub = Stub::start(UPDATE_RESPONSE);

    let output = feluda_with_definitions(
        temp.path(),
        &sbom.to_string_lossy(),
        &stub.endpoint,
        &definitions,
        &["--update-definitions"],
    );
    let entries = report(&output);
    // The scan itself still gets the service's answers.
    assert_eq!(
        entry(&entries, "feluda-fixture-answered")["license"],
        "Apache-2.0"
    );
    assert_eq!(
        std::fs::read_to_string(&definitions).unwrap(),
        "{ half written"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("was not updated"));
}

#[test]
fn an_update_without_a_file_to_write_is_an_error() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = write_sbom(temp.path());

    let output = Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(["--sbom-input", &sbom, "--update-definitions"])
        .current_dir(temp.path())
        .env("HOME", temp.path())
        .env("XDG_CACHE_HOME", temp.path().join("cache"))
        .env(
            "FELUDA_CLEARLYDEFINED_ENDPOINT",
            "http://127.0.0.1:9/definitions",
        )
        .output()
        .expect("failed to run feluda binary");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--update-definitions"));
}
