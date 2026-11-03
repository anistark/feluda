//! A Node project's licenses are asked of the registry over HTTP, not of `npm view`.
//!
//! `npm view <spec> license --json` costs one Node process per package (0.35–0.5 s each, 78 s of
//! CPU in 17 s of wall time at 1,000 dependencies, #288), so the registry request is the first
//! network step and `npm view` is kept for what only it can do. `npm view` honours `.npmrc`, so the
//! fixture points the project's `.npmrc` at a stub registry on localhost and puts a fake `npm`
//! first on `PATH` that logs its arguments and answers `view` with a license the stub never states:
//!
//! - a public project has to report the stub's license, log no `view`, and have the one request it
//!   made counted by `FELUDA_REQUEST_STATS`;
//! - a registry that answers `401` has to go through `npm view`, which sends the credentials feluda
//!   does not, after exactly one request from the stub — not zero, as a pre-check from the `.npmrc`
//!   would do, since a token for the public registry is on every machine that has run `npm login`;
//! - a version the registry no longer lists has to fall back, not read as the neighbouring version
//!   (`semver` reads a bare `1.3.0` as `^1.3.0`);
//! - a version document with no `license` field has to fall back, not report no license.
//!
//! The fixture's versions disagree with the real registry (`left-pad@1.3.0` is WTFPL there and ISC
//! here), so a report of ISC can only come from the stub.

#![cfg(unix)]

use std::collections::HashMap;
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

/// The packument the stub answers for `left-pad`, for the range path. The lockfile pins 1.3.0, so
/// the license reported is the ISC of that version's own document.
const PACKUMENT: &str = r#"{
  "name": "left-pad",
  "dist-tags": {"latest": "1.3.1"},
  "versions": {
    "1.3.0": {"name": "left-pad", "version": "1.3.0", "license": "ISC"},
    "1.3.1": {"name": "left-pad", "version": "1.3.1", "license": "MIT"}
  }
}"#;

/// The same package with 1.3.0 gone, as a registry that no longer serves a pinned version answers.
const PACKUMENT_WITHOUT_1_3_0: &str = r#"{
  "name": "left-pad",
  "dist-tags": {"latest": "1.3.1"},
  "versions": {
    "1.3.1": {"name": "left-pad", "version": "1.3.1", "license": "MIT"}
  }
}"#;

/// `GET /left-pad/1.3.0`: the per-version document, 4.8 KB instead of an 11 MB packument.
const VERSION: &str = r#"{"name": "left-pad", "version": "1.3.0", "license": "ISC"}"#;

const VERSION_WITHOUT_LICENSE: &str = r#"{"name": "left-pad", "version": "1.3.0"}"#;

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

/// A registry stub on localhost that answers the routes it was given and records what it was asked.
struct Stub {
    url: String,
    request: Arc<Mutex<Vec<Request>>>,
    routes: Arc<Mutex<HashMap<String, (u16, String)>>>,
}

/// One request the stub answered: the path it asked for and the head it sent.
struct Request {
    path: String,
    head: String,
}

impl Stub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind the registry stub");
        let url = format!("http://{}", listener.local_addr().expect("no stub address"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let routes = Arc::new(Mutex::new(HashMap::from([
            ("/left-pad".to_string(), (200, PACKUMENT.to_string())),
            ("/left-pad/1.3.0".to_string(), (200, VERSION.to_string())),
        ])));

        let logged = Arc::clone(&seen);
        let answers = Arc::clone(&routes);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let head = read_head(stream.try_clone().expect("failed to clone the stub stream"));
                let path = path_of(&head);
                let (status, body) = answers
                    .lock()
                    .expect("stub routes are poisoned")
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, "{}".to_string()));
                logged
                    .lock()
                    .expect("stub request log is poisoned")
                    .push(Request { path, head });
                respond(stream, status, &body);
            }
        });

        Self {
            url,
            request: seen,
            routes,
        }
    }

    /// What the stub answers for `path` from now on.
    fn answer(&self, path: &str, status: u16, body: &str) {
        self.routes
            .lock()
            .expect("stub routes are poisoned")
            .insert(path.to_string(), (status, body.to_string()));
    }

    fn requests(&self) -> Vec<String> {
        self.request
            .lock()
            .expect("stub request log is poisoned")
            .iter()
            .map(|request| request.path.clone())
            .collect()
    }

    /// How many requests asked for `accept` by name.
    fn accepted(&self, accept: &str) -> usize {
        self.request
            .lock()
            .expect("stub request log is poisoned")
            .iter()
            .filter(|request| request.head.to_lowercase().contains(accept))
            .count()
    }
}

fn read_head(mut stream: TcpStream) -> String {
    let mut buffer = [0u8; 4096];
    let read = stream.read(&mut buffer).unwrap_or(0);
    String::from_utf8_lossy(&buffer[..read]).to_string()
}

fn path_of(head: &str) -> String {
    head.split_whitespace().nth(1).unwrap_or("/").to_string()
}

fn respond(mut stream: TcpStream, status: u16, body: &str) {
    let reason = if status == 200 {
        "OK"
    } else if status == 401 {
        "Unauthorized"
    } else {
        "Not Found"
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("failed to write the stub response");
    stream.flush().expect("failed to flush the stub response");
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

/// A fake `npm` first on `PATH`: it logs its arguments and its working directory, and answers
/// `view` with a license the stub never states, so a report of WTFPL means a Node process ran.
fn fake_npm(bin: &Path) -> PathBuf {
    fs::create_dir_all(bin).expect("failed to create the bin directory");
    let log = bin.join("npm-arguments.log");
    let script = bin.join("npm");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\necho \"$PWD\" >> '{}'\ncase \"$1\" in\n  view) echo '\"WTFPL\"' ;;\nesac\n",
            log.display(),
            npm_cwd_log(log.parent().expect("the log has no directory")).display()
        ),
    )
    .expect("failed to write the fake npm");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
        .expect("failed to make the fake npm executable");
    log
}

fn npm_cwd_log(bin: &Path) -> PathBuf {
    bin.join("npm-cwd.log")
}

/// The directory the last `npm view` ran in.
fn npm_cwd(log: &Path) -> String {
    let bin = log.parent().expect("the log has no directory");
    fs::read_to_string(npm_cwd_log(bin))
        .unwrap_or_default()
        .lines()
        .last()
        .unwrap_or_default()
        .to_string()
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
        // A CI runner's proxy must not swallow the loopback request the stub answers.
        .env("NO_PROXY", "127.0.0.1")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
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

    // The pinned version has its own document, so the packument is never downloaded, and the
    // full document is named: abbreviated metadata carries no license on any version.
    assert_eq!(stub.requests(), vec!["/left-pad/1.3.0".to_string()]);
    assert_eq!(stub.accepted("accept: application/json"), 1);
    let tally = tally(root);
    assert_eq!(tally["total_requests"], 1, "tally: {tally:#?}");
    assert_eq!(
        tally["hosts"]["127.0.0.1"]["requests"], 1,
        "tally: {tally:#?}"
    );
}

#[test]
fn a_registry_that_answers_401_goes_through_npm_view() {
    let temp = tempfile::tempdir().expect("failed to create the temp directory");
    let root = temp.path();
    seed_caches(root);
    let stub = Stub::start();
    // The credentials exist, and this is what a registry that wants them answers an anonymous GET.
    stub.answer("/left-pad/1.3.0", 401, "{}");
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

    // The token in `.npmrc` is not a reason to skip the request: the refusal is what decides, and
    // it costs exactly one request.
    assert_eq!(stub.requests(), vec!["/left-pad/1.3.0".to_string()]);

    // `npm view` is the authoritative answer for those credentials only if it reads the same
    // `.npmrc`, so it has to run in the project and not in feluda's cwd.
    assert_eq!(
        fs::canonicalize(npm_cwd(&log)).expect("npm view ran nowhere"),
        fs::canonicalize(&project).expect("the project directory is gone")
    );
}

#[test]
fn an_exact_version_the_registry_no_longer_lists_does_not_read_as_its_neighbour() {
    let temp = tempfile::tempdir().expect("failed to create the temp directory");
    let root = temp.path();
    seed_caches(root);
    let stub = Stub::start();
    // 1.3.0 is gone from the packument, and its own document is a 404.
    stub.answer("/left-pad/1.3.0", 404, "{}");
    stub.answer("/left-pad", 200, PACKUMENT_WITHOUT_1_3_0);
    let log = fake_npm(&root.join("bin"));
    let project = project(root, &format!("registry={}/\n", stub.url));

    let output = scan(root, &project, &log);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {stderr}",
        output.status.code()
    );

    // 1.3.1's MIT never stands in for 1.3.0: the answer is `npm view`'s, or nothing.
    assert_eq!(license_of(&report(&output), "left-pad"), "WTFPL");
    // And the pinned version is asked for on its own endpoint, never read out of the packument.
    assert_eq!(stub.requests(), vec!["/left-pad/1.3.0".to_string()]);
    assert!(
        npm_arguments(&log)
            .iter()
            .any(|args| args == "view left-pad@1.3.0 license --json"),
        "npm view was not asked"
    );
}

#[test]
fn a_version_document_without_a_license_field_falls_back_to_npm_view() {
    let temp = tempfile::tempdir().expect("failed to create the temp directory");
    let root = temp.path();
    seed_caches(root);
    let stub = Stub::start();
    // Abbreviated metadata: the version is there, the license key is not.
    stub.answer("/left-pad/1.3.0", 200, VERSION_WITHOUT_LICENSE);
    let log = fake_npm(&root.join("bin"));
    let project = project(root, &format!("registry={}/\n", stub.url));

    let output = scan(root, &project, &log);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {stderr}",
        output.status.code()
    );

    // A missing key is not the package saying it has no license.
    assert_eq!(license_of(&report(&output), "left-pad"), "WTFPL");
}
