//! Opt in tally of outgoing HTTP requests, for benchmarking.
//!
//! Set `FELUDA_REQUEST_STATS` to a file path and every request feluda sends is counted per host,
//! with its status and how long it took to answer; the tally is written to that path as JSON when
//! the run ends. Unset, recording costs one check of an initialised `OnceLock` and nothing is kept.
//!
//! The time recorded is from sending to the response headers, so a slow body is not included.
//! Requests run concurrently, so a host's `total_ms` can exceed the wall time of the run.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::debug::{log, LogLevel};

const STATS_ENV: &str = "FELUDA_REQUEST_STATS";

#[derive(Default, serde::Serialize)]
struct HostStats {
    requests: u64,
    /// Requests that failed before any status came back (connection, TLS, timeout).
    transport_errors: u64,
    statuses: BTreeMap<u16, u64>,
    total_ms: f64,
    max_ms: f64,
}

#[derive(serde::Serialize)]
struct Report<'a> {
    total_requests: u64,
    hosts: &'a BTreeMap<String, HostStats>,
}

struct Stats {
    path: PathBuf,
    hosts: Mutex<BTreeMap<String, HostStats>>,
}

static STATS: OnceLock<Option<Stats>> = OnceLock::new();

fn stats() -> Option<&'static Stats> {
    STATS
        .get_or_init(|| {
            std::env::var_os(STATS_ENV)
                .filter(|path| !path.is_empty())
                .map(|path| Stats {
                    path: PathBuf::from(path),
                    hosts: Mutex::new(BTreeMap::new()),
                })
        })
        .as_ref()
}

fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn record(stats: &Stats, url: &str, elapsed: Duration, status: Option<u16>) {
    let ms = elapsed.as_secs_f64() * 1000.0;
    let Ok(mut hosts) = stats.hosts.lock() else {
        return;
    };
    let host = hosts.entry(host_of(url)).or_default();
    host.requests += 1;
    match status {
        Some(code) => *host.statuses.entry(code).or_default() += 1,
        None => host.transport_errors += 1,
    }
    host.total_ms += ms;
    host.max_ms = host.max_ms.max(ms);
}

/// `reqwest::blocking::get`, counted, with feluda's User-Agent.
///
/// A bare `reqwest::blocking::get` sends no User-Agent, and Maven Central's search API answers
/// that with a 403.
pub fn get(url: &str) -> reqwest::Result<reqwest::blocking::Response> {
    send(url, || {
        reqwest::blocking::Client::builder()
            .user_agent("feluda-license-checker/1.0")
            .build()?
            .get(url)
            .send()
    })
}

/// Run a blocking request for `url` and count it.
pub fn send(
    url: &str,
    request: impl FnOnce() -> reqwest::Result<reqwest::blocking::Response>,
) -> reqwest::Result<reqwest::blocking::Response> {
    let Some(stats) = stats() else {
        return request();
    };
    let started = Instant::now();
    let result = request();
    let status = result.as_ref().ok().map(|r| r.status().as_u16());
    record(stats, url, started.elapsed(), status);
    result
}

/// Await an async request for `url` and count it.
pub async fn send_async(
    url: &str,
    request: impl Future<Output = reqwest::Result<reqwest::Response>>,
) -> reqwest::Result<reqwest::Response> {
    let Some(stats) = stats() else {
        return request.await;
    };
    let started = Instant::now();
    let result = request.await;
    let status = result.as_ref().ok().map(|r| r.status().as_u16());
    record(stats, url, started.elapsed(), status);
    result
}

/// Write the tally to the path in `FELUDA_REQUEST_STATS`, if it is set.
pub fn write() {
    let Some(stats) = stats() else {
        return;
    };
    let Ok(hosts) = stats.hosts.lock() else {
        return;
    };
    let report = Report {
        total_requests: hosts.values().map(|h| h.requests).sum(),
        hosts: &hosts,
    };
    let written = serde_json::to_vec_pretty(&report)
        .map_err(|e| e.to_string())
        .and_then(|json| std::fs::write(&stats.path, json).map_err(|e| e.to_string()));
    if let Err(e) = written {
        log(
            LogLevel::Warn,
            &format!(
                "Could not write request stats to {}: {e}",
                stats.path.display()
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_taken_from_the_url() {
        assert_eq!(
            host_of("https://registry.npmjs.org/left-pad"),
            "registry.npmjs.org"
        );
        assert_eq!(host_of("not a url"), "unknown");
    }

    #[test]
    fn get_sends_a_user_agent() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            String::from_utf8_lossy(&buf[..n]).to_lowercase()
        });

        let response = get(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert!(response.status().is_success());
        let request = server.join().unwrap();
        assert!(request.contains("user-agent: feluda-license-checker/1.0"));
    }

    #[test]
    fn record_tallies_per_host() {
        let stats = Stats {
            path: PathBuf::new(),
            hosts: Mutex::new(BTreeMap::new()),
        };
        record(
            &stats,
            "https://pypi.org/pypi/a/json",
            Duration::from_millis(30),
            Some(200),
        );
        record(
            &stats,
            "https://pypi.org/pypi/b/json",
            Duration::from_millis(10),
            Some(404),
        );
        record(
            &stats,
            "https://pypi.org/pypi/c/json",
            Duration::from_millis(5),
            None,
        );

        let hosts = stats.hosts.lock().unwrap();
        let pypi = &hosts["pypi.org"];
        assert_eq!(pypi.requests, 3);
        assert_eq!(pypi.transport_errors, 1);
        assert_eq!(pypi.statuses[&200], 1);
        assert_eq!(pypi.statuses[&404], 1);
        assert!((pypi.total_ms - 45.0).abs() < 1.0);
        assert!((pypi.max_ms - 30.0).abs() < 1.0);
    }
}
