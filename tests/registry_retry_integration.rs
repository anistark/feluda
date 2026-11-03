//! A registry that answers 429 is asked again a bounded number of times, then the lookup moves on.
//!
//! Every outgoing request goes through `http_stats`, which retries 429 and 503 honouring
//! `Retry-After`. The fixture is a Maven project with an empty local repository and
//! `FELUDA_MAVEN_ENDPOINT` pointed at a stub that answers every request with
//! `429 Too Many Requests` and `Retry-After: 0`. The tally that `FELUDA_REQUEST_STATS` writes has
//! to show each POM lookup sent four times (three of them retries), the scan has to finish, and
//! stderr has to say once why the licenses read Unknown.
//!
//! `RAYON_NUM_THREADS=1` keeps the lookups in sequence, so the counts do not depend on timing.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const DEPENDENCIES: u64 = 2;

/// Attempts per request in `http_stats`, the first included.
const ATTEMPTS: u64 = 4;

fn project_pom() -> String {
    let dependencies: String = (0..DEPENDENCIES)
        .map(|i| {
            format!(
                "    <dependency>\n      <groupId>com.stub</groupId>\n      \
                 <artifactId>dep{i}</artifactId>\n      <version>1.0</version>\n    </dependency>\n"
            )
        })
        .collect();
    format!(
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>demo</artifactId>
  <version>1.0</version>
  <dependencies>
{dependencies}  </dependencies>
</project>
"#
    )
}

/// A base URL on localhost that answers every request with 429 and `Retry-After: 0`. The server
/// thread runs until the test process exits.
fn busy_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind");
    let port = listener.local_addr().expect("no address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\n\
                  Content-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    format!("http://127.0.0.1:{port}/maven2")
}

fn tally(home: &Path) -> Value {
    let raw = fs::read_to_string(home.join("requests.json")).expect("feluda wrote no tally");
    serde_json::from_str(&raw).expect("request tally is not JSON")
}

#[test]
fn a_rate_limited_registry_is_asked_a_bounded_number_of_times() {
    let home = tempfile::tempdir().expect("failed to create temp home");
    let project = home.path().join("java-project");
    fs::create_dir_all(&project).expect("failed to create project dir");
    fs::write(project.join("pom.xml"), project_pom()).expect("failed to write pom.xml");
    let m2 = home.path().join("m2");
    fs::create_dir_all(&m2).expect("failed to create m2");

    let output = Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args([
            "--path",
            project.to_str().expect("path is not UTF-8"),
            "--json",
        ])
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .env("FELUDA_MAVEN_ENDPOINT", busy_endpoint())
        .env("FELUDA_REQUEST_STATS", home.path().join("requests.json"))
        .env("MAVEN_REPO_LOCAL", &m2)
        .env("RAYON_NUM_THREADS", "1")
        .output()
        .expect("failed to run feluda binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {stderr}",
        output.status.code()
    );
    assert_eq!(
        stderr.matches("127.0.0.1 is still answering 429").count(),
        1,
        "the warning is printed once per host: {stderr}"
    );

    let tally = tally(home.path());
    let host = &tally["hosts"]["127.0.0.1"];
    assert_eq!(
        host["requests"],
        DEPENDENCIES * ATTEMPTS,
        "tally: {tally:#}"
    );
    assert_eq!(
        host["statuses"]["429"],
        DEPENDENCIES * ATTEMPTS,
        "tally: {tally:#}"
    );
    assert_eq!(
        host["retries"],
        DEPENDENCIES * (ATTEMPTS - 1),
        "tally: {tally:#}"
    );
    assert_eq!(host["transport_errors"], 0, "tally: {tally:#}");

    let report: Vec<Value> =
        serde_json::from_slice(&output.stdout).expect("feluda emitted invalid JSON");
    assert_eq!(report.len() as u64, DEPENDENCIES, "report: {report:#?}");
}
