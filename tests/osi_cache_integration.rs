//! Integration coverage for the OSI approval-list cache and its offline fallbacks.
//!
//! Unix only: the harness isolates the cache through `HOME` and `XDG_CACHE_HOME`, and on Windows
//! `dirs::cache_dir()` asks the OS for the Local AppData folder instead, so a run there would read
//! and clear the developer's real cache.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const SBOM: &str = r#"{
  "spdxVersion": "SPDX-2.3",
  "SPDXID": "SPDXRef-DOCUMENT",
  "name": "fixture",
  "documentNamespace": "https://example.com/fixture",
  "creationInfo": {"created": "2026-01-01T00:00:00Z", "creators": ["Tool: fixture"]},
  "packages": [{
    "SPDXID": "SPDXRef-1",
    "name": "epl-project",
    "versionInfo": "1.0.0",
    "licenseConcluded": "EPL-2.0",
    "licenseDeclared": "EPL-2.0"
  }]
}"#;

const OSI_RESPONSE: &str = r#"[{"id":"epl-2-0","spdx_id":"EPL-2.0","approved":true}]"#;
const THIRTY_ONE_DAYS: u64 = 31 * 24 * 60 * 60;

struct Stub {
    endpoint: String,
    requests: Arc<AtomicUsize>,
}

impl Stub {
    fn start(status: &'static str, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind OSI stub");
        let endpoint = format!(
            "http://{}/api/licenses",
            listener.local_addr().expect("stub has no address")
        );
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = Arc::clone(&requests);

        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                request_count.fetch_add(1, Ordering::SeqCst);
                serve(stream, status, body);
            }
        });

        Self { endpoint, requests }
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn serve(mut stream: TcpStream, status: &str, body: &str) {
    let mut request = [0u8; 2048];
    let _ = stream.read(&mut request);
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("failed to write OSI stub response");
    stream.flush().expect("failed to flush OSI stub response");
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock predates Unix epoch")
        .as_secs()
}

fn cache_dir(home: &std::path::Path) -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Caches/feluda")
    } else {
        home.join("cache/feluda")
    }
}

fn write_cache(home: &std::path::Path, name: &str, data: Value, timestamp: u64) {
    let dir = cache_dir(home);
    std::fs::create_dir_all(&dir).expect("failed to create fixture cache directory");
    std::fs::write(
        dir.join(name),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "data": data,
            "timestamp": timestamp,
        }))
        .expect("failed to serialize fixture cache"),
    )
    .expect("failed to write fixture cache");
}

fn prepare(home: &std::path::Path) -> String {
    let sbom = home.join("fixture.spdx.json");
    std::fs::write(&sbom, SBOM).expect("failed to write fixture SBOM");
    // Classification also consults the GitHub license table. A fresh empty table keeps these tests
    // focused on the OSI request and prevents any external network access.
    write_cache(home, "github_licenses.json", json!({}), now_secs());
    sbom.to_string_lossy().into_owned()
}

fn feluda(home: &std::path::Path, sbom: &str, endpoint: &str, stats: &str) -> Output {
    std::fs::write(
        home.join(".feluda.toml"),
        format!("[osi]\nendpoint = {endpoint:?}\n"),
    )
    .expect("failed to write OSI endpoint config");
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(["--sbom-input", sbom, "--json"])
        .current_dir(home)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("LOCALAPPDATA", home.join("cache"))
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .env("FELUDA_REQUEST_STATS", stats)
        .output()
        .expect("failed to run feluda")
}

fn report(output: &Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "feluda emitted invalid JSON: {error}\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// How many OSI warnings a run printed. Counted by message rather than by line, since the
/// spinner writes its frames to stderr too.
fn osi_warnings(output: &Output) -> usize {
    String::from_utf8_lossy(&output.stderr)
        .matches("OSI license data (")
        .count()
}

fn read_osi_cache(home: &std::path::Path) -> Value {
    serde_json::from_slice(
        &std::fs::read(cache_dir(home).join("osi_licenses.json")).expect("OSI cache is missing"),
    )
    .expect("OSI cache is invalid JSON")
}

fn cache_status(home: &std::path::Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_feluda"))
        .arg("cache")
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("LOCALAPPDATA", home.join("cache"))
        .output()
        .expect("failed to inspect cache");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn total_requests(path: &std::path::Path) -> u64 {
    let stats: Value = serde_json::from_slice(
        &std::fs::read(path).expect("feluda did not write request statistics"),
    )
    .expect("request statistics are not valid JSON");
    stats["total_requests"]
        .as_u64()
        .expect("request statistics have no total")
}

#[test]
fn a_fresh_osi_cache_prevents_a_second_request() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = prepare(temp.path());
    let stub = Stub::start("200 OK", OSI_RESPONSE);
    let first_stats = temp.path().join("first-stats.json");
    let second_stats = temp.path().join("second-stats.json");

    let first = feluda(
        temp.path(),
        &sbom,
        &stub.endpoint,
        &first_stats.to_string_lossy(),
    );
    assert_eq!(report(&first)[0]["osi_status"], "Approved");
    assert_eq!(total_requests(&first_stats), 1);
    assert_eq!(stub.request_count(), 1);
    assert!(cache_dir(temp.path()).join("osi_licenses.json").exists());

    let second = feluda(
        temp.path(),
        &sbom,
        &stub.endpoint,
        &second_stats.to_string_lossy(),
    );
    assert_eq!(report(&second)[0]["osi_status"], "Approved");
    assert_eq!(total_requests(&second_stats), 0);
    assert_eq!(stub.request_count(), 1);
}

#[test]
fn a_failed_fetch_without_cache_warns_and_keeps_json_output_valid() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = prepare(temp.path());
    let stub = Stub::start("503 Service Unavailable", "unavailable");
    let stats = temp.path().join("stats.json");

    let output = feluda(temp.path(), &sbom, &stub.endpoint, &stats.to_string_lossy());
    assert_eq!(report(&output)[0]["osi_status"], "Unknown");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Could not load OSI license data"));
    assert!(stderr.contains("503 Service Unavailable"));
    assert!(stderr.contains("may read Unknown"));
    assert_eq!(osi_warnings(&output), 1);
    assert_eq!(stub.request_count(), 1);

    // The built-in list is recorded as never fetched, so the next run inside the retry window
    // does not ask again, and `feluda cache` does not call it fresh.
    let retry_stats = temp.path().join("retry-stats.json");
    let retry = feluda(
        temp.path(),
        &sbom,
        &stub.endpoint,
        &retry_stats.to_string_lossy(),
    );
    assert_eq!(report(&retry)[0]["osi_status"], "Unknown");
    assert_eq!(total_requests(&retry_stats), 0);
    assert_eq!(stub.request_count(), 1);

    let status = cache_status(temp.path());
    assert!(status.contains("OSI Licenses Cache: ✗ STALE"), "{status}");
    assert!(status.contains("never fetched"), "{status}");
    assert!(status.contains("Last refresh failed"), "{status}");
}

#[test]
fn a_failed_refresh_uses_the_stale_cache() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = prepare(temp.path());
    let stale_timestamp = now_secs() - THIRTY_ONE_DAYS;
    write_cache(
        temp.path(),
        "osi_licenses.json",
        json!({"EPL-2.0": "Approved"}),
        stale_timestamp,
    );
    let stub = Stub::start("503 Service Unavailable", "unavailable");
    let stats = temp.path().join("stats.json");
    let retry_stats = temp.path().join("retry-stats.json");

    let output = feluda(temp.path(), &sbom, &stub.endpoint, &stats.to_string_lossy());
    assert_eq!(report(&output)[0]["osi_status"], "Approved");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("using the stale cache"));
    assert_eq!(osi_warnings(&output), 1);
    assert_eq!(stub.request_count(), 1);

    // The table keeps its real age; only the retry is put off.
    let cache = read_osi_cache(temp.path());
    assert_eq!(cache["timestamp"], stale_timestamp);
    assert!(cache["retry_after"].as_u64().unwrap() > now_secs());
    let status = cache_status(temp.path());
    assert!(status.contains("OSI Licenses Cache: ✗ STALE"), "{status}");
    assert!(status.contains("31 days ago"), "{status}");
    assert!(status.contains("Last refresh failed"), "{status}");

    let retry = feluda(
        temp.path(),
        &sbom,
        &stub.endpoint,
        &retry_stats.to_string_lossy(),
    );
    assert_eq!(report(&retry)[0]["osi_status"], "Approved");
    assert_eq!(total_requests(&retry_stats), 0);
    assert_eq!(stub.request_count(), 1);
}

#[test]
fn an_empty_successful_response_is_never_cached() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = prepare(temp.path());
    let stub = Stub::start("200 OK", "[]");
    let stats = temp.path().join("stats.json");

    let output = feluda(temp.path(), &sbom, &stub.endpoint, &stats.to_string_lossy());
    assert_eq!(report(&output)[0]["osi_status"], "Unknown");
    assert!(String::from_utf8_lossy(&output.stderr).contains("no approved licenses"));
    assert_eq!(stub.request_count(), 1);

    // What is kept for the retry window is the built-in list, never the empty answer.
    let cache = read_osi_cache(temp.path());
    assert_eq!(cache["timestamp"], 0);
    assert_eq!(cache["data"]["MIT"], "Approved");
}

#[test]
fn a_successful_refresh_replaces_the_stale_cache() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let sbom = prepare(temp.path());
    let stale_timestamp = now_secs() - THIRTY_ONE_DAYS;
    write_cache(
        temp.path(),
        "osi_licenses.json",
        json!({"EPL-2.0": "NotApproved"}),
        stale_timestamp,
    );
    let stub = Stub::start("200 OK", OSI_RESPONSE);
    let stats = temp.path().join("stats.json");

    let output = feluda(temp.path(), &sbom, &stub.endpoint, &stats.to_string_lossy());
    assert_eq!(report(&output)[0]["osi_status"], "Approved");
    assert_eq!(stub.request_count(), 1);

    let cache: Value = serde_json::from_slice(
        &std::fs::read(cache_dir(temp.path()).join("osi_licenses.json"))
            .expect("refreshed OSI cache is missing"),
    )
    .expect("refreshed OSI cache is invalid JSON");
    assert_eq!(cache["data"]["EPL-2.0"], "Approved");
    assert!(cache["timestamp"].as_u64().unwrap() > stale_timestamp);
}

#[test]
fn cache_command_lists_and_clears_the_osi_cache() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    write_cache(
        temp.path(),
        "osi_licenses.json",
        json!({"EPL-2.0": "Approved"}),
        now_secs(),
    );

    assert!(cache_status(temp.path()).contains("OSI Licenses Cache: ✓ FRESH"));

    let clear = Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(["cache", "--clear"])
        .env("HOME", temp.path())
        .env("XDG_CACHE_HOME", temp.path().join("cache"))
        .env("LOCALAPPDATA", temp.path().join("cache"))
        .output()
        .expect("failed to clear cache");
    assert!(clear.status.success());
    assert!(!cache_dir(temp.path()).join("osi_licenses.json").exists());
}
