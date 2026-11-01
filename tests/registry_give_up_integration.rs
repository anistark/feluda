//! A registry that stops answering is given up on for the rest of the run.
//!
//! Every outgoing request goes through `http_stats`, which counts transport errors per host and,
//! after three in a row, skips that host instead of waiting on it again. The fixture is a Maven
//! project with several dependencies, an empty local repository and `FELUDA_MAVEN_ENDPOINT`
//! pointed at a port that refuses connections, so every POM lookup fails at once. The tally that
//! `FELUDA_REQUEST_STATS` writes has to show exactly three requests that went out and the rest
//! skipped, the scan has to finish, and stderr has to say why the licenses read Unknown.
//!
//! `RAYON_NUM_THREADS=1` keeps the lookups in sequence: with lookups side by side, more than three
//! can be in flight before the third failure lands, and the count of requests sent would vary.

use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const DEPENDENCIES: usize = 6;

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

/// A base URL on localhost whose port refuses connections: bound to find a free port, then closed.
fn refused_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind");
    let port = listener.local_addr().expect("no address").port();
    drop(listener);
    format!("http://127.0.0.1:{port}/maven2")
}

fn tally(home: &Path) -> Value {
    let raw = fs::read_to_string(home.join("requests.json")).expect("feluda wrote no tally");
    serde_json::from_str(&raw).expect("request tally is not JSON")
}

#[test]
fn a_refusing_registry_is_skipped_after_three_failures() {
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
        .env("FELUDA_MAVEN_ENDPOINT", refused_endpoint())
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
    assert!(
        stderr.contains("127.0.0.1 did not answer 3 requests in a row"),
        "no give-up warning on stderr: {stderr}"
    );
    assert_eq!(
        stderr.matches("did not answer").count(),
        1,
        "the warning is printed once per host: {stderr}"
    );

    let tally = tally(home.path());
    let host = &tally["hosts"]["127.0.0.1"];
    assert_eq!(host["requests"], 3, "tally: {tally:#}");
    assert_eq!(host["transport_errors"], 3, "tally: {tally:#}");
    assert!(
        host["skipped"].as_u64().unwrap_or(0) >= (DEPENDENCIES - 3) as u64,
        "tally: {tally:#}"
    );

    let report: Vec<Value> =
        serde_json::from_slice(&output.stdout).expect("feluda emitted invalid JSON");
    assert_eq!(report.len(), DEPENDENCIES, "report: {report:#?}");
}
