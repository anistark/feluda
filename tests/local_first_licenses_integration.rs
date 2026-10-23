//! A Maven dependency that is already in the local repository must be read from there, not from
//! Maven Central.
//!
//! `AGENTS.md` makes local-first resolution a rule: local files before network requests, with
//! `--no-local` as the only way to skip them. The fixture makes the two sources disagree — the
//! cached jar says MIT, while the cached POM and the stub registry both say Apache-2.0 — so the
//! reported license says which one answered, and the request tally says whether the registry was
//! asked at all.
//!
//! The registry is pointed at a stub on localhost (`FELUDA_MAVEN_ENDPOINT`) so the test never
//! depends on Maven Central, and `FELUDA_REQUEST_STATS` counts what was actually asked: the default
//! run has to report MIT and make **no** request to the registry, `--no-local` has to report
//! Apache-2.0 and reach the stub.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;

use serde_json::Value;

const POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.stub</groupId>
  <artifactId>widget</artifactId>
  <version>1.0</version>
  <licenses>
    <license>
      <name>Apache-2.0</name>
    </license>
  </licenses>
  <dependencies>
    <dependency>
      <groupId>com.stub</groupId>
      <artifactId>child</artifactId>
      <version>1.0</version>
    </dependency>
  </dependencies>
</project>
"#;

/// What the local repository holds: the same license, no dependencies, so a fully local tree.
const LOCAL_POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.stub</groupId>
  <artifactId>widget</artifactId>
  <version>1.0</version>
  <licenses>
    <license>
      <name>Apache-2.0</name>
    </license>
  </licenses>
</project>
"#;

const CHILD_POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.stub</groupId>
  <artifactId>child</artifactId>
  <version>1.0</version>
  <licenses>
    <license>
      <name>MIT</name>
    </license>
  </licenses>
</project>
"#;

const PROJECT_POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>demo</artifactId>
  <version>1.0</version>
  <dependencies>
    <dependency>
      <groupId>com.stub</groupId>
      <artifactId>widget</artifactId>
      <version>1.0</version>
    </dependency>
  </dependencies>
</project>
"#;

/// A registry stub that answers one POM and 404s anything else.
struct Stub {
    base: String,
}

impl Stub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind registry stub");
        let base = format!(
            "http://{}",
            listener.local_addr().expect("stub has no address")
        );

        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve(stream);
            }
        });

        Self { base }
    }
}

/// Answer one request by path, with `Connection: close` so the client never waits on more.
fn serve(mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("failed to clone stub stream"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    // Drain the headers; these lookups never send a body.
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line == "\r\n" || line == "\n" => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }

    let path = request_line.split_whitespace().nth(1).unwrap_or("");
    let (status, body) = if path.ends_with("widget-1.0.pom") {
        ("200 OK", POM)
    } else if path.ends_with("child-1.0.pom") {
        ("200 OK", CHILD_POM)
    } else {
        ("404 Not Found", "")
    };

    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn write_jar(path: &Path, license: &str) {
    use zip::write::SimpleFileOptions;

    fs::create_dir_all(path.parent().expect("jar has no parent")).expect("failed to create m2");
    let file = fs::File::create(path).expect("failed to create jar");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("META-INF/LICENSE", SimpleFileOptions::default())
        .expect("failed to start jar entry");
    zip.write_all(license.as_bytes())
        .expect("failed to write jar entry");
    zip.finish().expect("failed to finish jar");
}

/// A Maven project whose only dependency is `com.stub:widget:1.0`, already in a local repository
/// (jar and POM, as a real build leaves them).
fn maven_fixture(root: &Path) -> (PathBuf, PathBuf) {
    let project = root.join("java-project");
    fs::create_dir_all(&project).expect("failed to create project dir");
    fs::write(project.join("pom.xml"), PROJECT_POM).expect("failed to write pom.xml");

    let m2 = root.join("m2");
    let artifact = m2.join("com/stub/widget/1.0");
    write_jar(
        &artifact.join("widget-1.0.jar"),
        "MIT License\n\nCopyright (c) 2024 Stub",
    );
    fs::write(artifact.join("widget-1.0.pom"), LOCAL_POM).expect("failed to write cached pom");

    (project, m2)
}

fn feluda(home: &Path, project: &Path, stub: &Stub, m2: &Path, extra_args: &[&str]) -> Output {
    let stats = home.join("requests.json");
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args([
            "--path",
            project.to_str().expect("project path is not UTF-8"),
        ])
        .arg("--json")
        .args(extra_args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .env("FELUDA_MAVEN_ENDPOINT", format!("{}/maven2", stub.base))
        .env("FELUDA_REQUEST_STATS", &stats)
        .env("MAVEN_REPO_LOCAL", m2)
        .output()
        .expect("failed to run feluda binary")
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

fn license_of(entries: &[Value], name: &str) -> String {
    let entry = entries
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("no entry named {name:?} in report: {entries:#?}"));
    entry["license"]
        .as_str()
        .unwrap_or_else(|| panic!("entry {name:?} has no license: {entry:#?}"))
        .to_string()
}

/// Registry requests the run made, read from the tally `FELUDA_REQUEST_STATS` writes.
fn registry_requests(home: &Path) -> u64 {
    let stats = home.join("requests.json");
    let raw = fs::read_to_string(&stats).expect("feluda wrote no request tally");
    let tally: Value = serde_json::from_str(&raw).expect("request tally is not JSON");
    tally["hosts"]
        .get("127.0.0.1")
        .map(|host| host["requests"].as_u64().unwrap_or(0))
        .unwrap_or(0)
}

#[test]
fn maven_license_and_pom_come_from_the_local_repository_first() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let (project, m2) = maven_fixture(temp.path());
    let stub = Stub::start();

    let local_first = feluda(temp.path(), &project, &stub, &m2, &[]);
    let entries = report(&local_first);
    assert_eq!(
        license_of(&entries, "com.stub:widget"),
        "MIT",
        "the jar in the local repository should answer first"
    );
    assert_eq!(
        registry_requests(temp.path()),
        0,
        "a dependency that is fully local must not be asked about at all"
    );

    // `--no-local` is the only way to skip the local files, and it has to reach the Java analyzer.
    let remote = feluda(temp.path(), &project, &stub, &m2, &["--no-local"]);
    let entries = report(&remote);
    assert_eq!(
        license_of(&entries, "com.stub:widget"),
        "Apache-2.0",
        "--no-local should fall back to the registry"
    );
    assert!(
        registry_requests(temp.path()) > 0,
        "--no-local should have asked the registry"
    );
    // The transitive walk has to honour the flag too: `child` exists only in the registry's copy
    // of the POM, never in the local one.
    assert!(
        entries
            .iter()
            .any(|entry| entry["name"] == "com.stub:child"),
        "the transitive walk should have read the registry POM: {entries:#?}"
    );
}

#[test]
fn an_unreadable_local_pom_falls_back_to_the_registry() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let (project, m2) = maven_fixture(temp.path());
    let stub = Stub::start();

    // With the jar gone the POM is the only local source. A proxy's error page saved under its
    // name is readable but is not a POM; treating it as the answer would report Unknown for a
    // dependency the registry can describe.
    let artifact = m2.join("com/stub/widget/1.0");
    fs::remove_file(artifact.join("widget-1.0.jar")).expect("failed to remove cached jar");
    fs::write(
        artifact.join("widget-1.0.pom"),
        "<html>502 Bad Gateway</html>",
    )
    .expect("failed to overwrite cached pom");

    let output = feluda(temp.path(), &project, &stub, &m2, &[]);
    let entries = report(&output);
    assert_eq!(
        license_of(&entries, "com.stub:widget"),
        "Apache-2.0",
        "a corrupt local POM must not silence the registry"
    );
    assert!(
        registry_requests(temp.path()) > 0,
        "the registry should have been asked"
    );
}
