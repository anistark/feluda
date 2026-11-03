//! A Node project's licenses are asked of the registry over HTTP, not of `npm view`.
//!
//! `npm view <spec> license --json` costs one Node process per package (0.35–0.5 s each, 78 s of
//! CPU in 17 s of wall time at 1,000 dependencies, #288), so the packument request is the first
//! network step and `npm view` is kept for what only it can do. `npm view` honours `.npmrc`, so the
//! fixture points the project's `.npmrc` at a stub registry on localhost and puts a fake `npm`
//! first on `PATH` that logs its arguments and answers `view` with a license the stub never states:
//!
//! - a public project has to report the stub's license, log no `view`, and have the one request it
//!   made counted by `FELUDA_REQUEST_STATS`;
//! - a `.npmrc` that holds a token for that registry has to go through `npm view`, since feluda
//!   does not send credentials, and the stub must not be asked at all.
//!
//! The fixture's versions disagree with the real registry (`left-pad@1.3.0` is WTFPL there and ISC
//! here), so a report of ISC can only come from the stub.

#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// The packument the stub answers for `left-pad`. The lockfile pins 1.3.0, so the license reported
/// is the ISC of that version.
const PACKUMENT: &str = r#"{
  "name": "left-pad",
  "dist-tags": {"latest": "1.3.1"},
  "versions": {
    "1.3.0": {"name": "left-pad", "version": "1.3.0", "license": "ISC"},
    "1.3.1": {"name": "left-pad", "version": "1.3.1", "license": "MIT"}
  }
}"#;

const PACKAGE_JSON: &str = r#"{
  "name": "fixture-node-project",
  "version": "1.0.0",
  "license": "MIT",
  "dependencies": {}
}
"#;

/// The dependency list comes from the lockfile, so `npm ls` is never the only source and the
/// recursive resolver — which still asks registry.npmjs.org directly — has nothing to resolve.
const PACKAGE_LOCK: &str = r#"{
  "name": "fixture-node-project",
  "version": "1.0.0",
  "lockfileVersion": 3,
  "packages": {
    "node_modules/left-pad": {"name": "left-pad", "version": "1.3.0"}
  }
}
"#;

/// A registry stub on localhost that answers the packument and records the paths it was asked for.
struct Stub {
    url: String,
    paths: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind the registry stub");
        let url = format!("http://{}", listener.local_addr().expect("no stub address"));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&paths);

        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                seen.lock()
                    .expect("stub path log is poisoned")
                    .push(path_of(stream));
            }
        });

        Self { url, paths }
    }

    fn requests(&self) -> Vec<String> {
        self.paths
            .lock()
            .expect("stub path log is poisoned")
            .clone()
    }
}

fn path_of(mut stream: TcpStream) -> String {
    let mut buffer = [0u8; 4096];
    let read = stream.read(&mut buffer).unwrap_or(0);
    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PACKUMENT}",
        PACKUMENT.len()
    )
    .expect("failed to write the stub response");
    stream.flush().expect("failed to flush the stub response");
    path
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock predates Unix epoch")
        .as_secs()
}

fn cache_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Caches/feluda")
    } else {
        home.join("cache/feluda")
    }
}

/// Seed both caches so the run makes no request of its own: classification reads the GitHub
/// license table and the OSI approval list, and an empty table is enough for these fixtures.
fn seed_caches(home: &Path) {
    let dir = cache_dir(home);
    fs::create_dir_all(&dir).expect("failed to create the fixture cache directory");
    for name in ["github_licenses.json", "osi_licenses.json"] {
        fs::write(
            dir.join(name),
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "data": {},
                "timestamp": now_secs(),
            }))
            .expect("failed to serialize the fixture cache"),
        )
        .expect("failed to write the fixture cache");
    }
}

/// A fake `npm` first on `PATH`: it logs its arguments and answers `view` with a license the stub
/// never states, so a report of WTFPL means a Node process ran.
fn fake_npm(bin: &Path) -> PathBuf {
    fs::create_dir_all(bin).expect("failed to create the bin directory");
    let log = bin.join("npm-arguments.log");
    let script = bin.join("npm");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\ncase \"$1\" in\n  view) echo '\"WTFPL\"' ;;\nesac\n",
            log.display()
        ),
    )
    .expect("failed to write the fake npm");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
        .expect("failed to make the fake npm executable");
    log
}

fn project(root: &Path, npmrc: &str) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(&project).expect("failed to create the project directory");
    fs::write(project.join("package.json"), PACKAGE_JSON).expect("failed to write package.json");
    fs::write(project.join("package-lock.json"), PACKAGE_LOCK)
        .expect("failed to write package-lock.json");
    fs::write(project.join(".npmrc"), npmrc).expect("failed to write .npmrc");
    project
}

fn scan(root: &Path, project: &Path, log: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args([
            "--path",
            project.to_str().expect("path is not UTF-8"),
            "--json",
        ])
        .current_dir(root)
        .env(
            "PATH",
            format!(
                "{}:{}",
                log.parent().unwrap().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("HOME", root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("LOCALAPPDATA", root.join("cache"))
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .env("FELUDA_REQUEST_STATS", root.join("requests.json"))
        .env_remove("npm_config_registry")
        .output()
        .expect("failed to run feluda")
}

fn report(output: &Output) -> Vec<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "feluda emitted invalid JSON: {e}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn license_of(entries: &[Value], name: &str) -> String {
    entries
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("no entry named {name:?} in the report: {entries:#?}"))["license"]
        .as_str()
        .expect("the entry has no license string")
        .to_string()
}

fn npm_arguments(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn tally(root: &Path) -> Value {
    let raw =
        fs::read_to_string(root.join("requests.json")).expect("feluda wrote no request tally");
    serde_json::from_str(&raw).expect("the request tally is not JSON")
}

#[test]
fn a_public_project_asks_the_registry_and_spawns_no_npm_view() {
    let temp = tempfile::tempdir().expect("failed to create the temp directory");
    let root = temp.path();
    seed_caches(root);
    let stub = Stub::start();
    let log = fake_npm(&root.join("bin"));
    let project = project(root, &format!("registry={}/\n", stub.url));

    let output = scan(root, &project, &log);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {stderr}",
        output.status.code()
    );

    let entries = report(&output);
    assert_eq!(license_of(&entries, "left-pad"), "ISC");

    // The license came from the stub, so no license lookup may have run a Node process.
    let arguments = npm_arguments(&log);
    assert!(
        !arguments.iter().any(|args| args.starts_with("view")),
        "a license lookup spawned npm view: {arguments:?}"
    );

    assert_eq!(stub.requests(), vec!["/left-pad".to_string()]);
    let tally = tally(root);
    assert_eq!(tally["total_requests"], 1, "tally: {tally:#?}");
    assert_eq!(
        tally["hosts"]["127.0.0.1"]["requests"], 1,
        "tally: {tally:#?}"
    );
}

#[test]
fn a_registry_that_needs_credentials_goes_through_npm_view() {
    let temp = tempfile::tempdir().expect("failed to create the temp directory");
    let root = temp.path();
    seed_caches(root);
    let stub = Stub::start();
    let log = fake_npm(&root.join("bin"));
    let nerf_dart = stub.url.trim_start_matches("http:");
    let project = project(
        root,
        &format!(
            "registry={}/\n{nerf_dart}/:_authToken=fixture-token\n",
            stub.url
        ),
    );

    let output = scan(root, &project, &log);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {stderr}",
        output.status.code()
    );

    // Only `npm view` can state WTFPL here, so it is what answered.
    let entries = report(&output);
    assert_eq!(license_of(&entries, "left-pad"), "WTFPL");
    let arguments = npm_arguments(&log);
    assert!(
        arguments
            .iter()
            .any(|args| args == "view left-pad@1.3.0 license --json"),
        "npm view was not asked: {arguments:?}"
    );

    // Feluda sends no credentials, so it must not have asked the stub at all.
    assert_eq!(stub.requests(), Vec::<String>::new());
}
