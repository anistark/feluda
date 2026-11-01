//! Every outgoing HTTP request: the shared client, per host give-up, and an opt in tally.
//!
//! Clients start from [`client_builder`] (or [`async_client_builder`]), which carries feluda's
//! User-Agent and the `[timeouts]` connect and request limits, and every request goes through
//! [`send`], [`send_async`] or [`get`].
//!
//! A host that fails [`GIVE_UP_AFTER`] requests in a row without answering (connection refused,
//! TLS, timeout) is skipped for the rest of the run: later requests to it return
//! [`FeludaError::HostSkipped`] at once, and one line on stderr says so. Any response, an error
//! status included, resets the count.
//!
//! Set `FELUDA_REQUEST_STATS` to a file path and every request feluda sends is counted per host,
//! with its status and how long it took to answer; the tally is written to that path as JSON when
//! the run ends. Skipped requests are counted apart, under `skipped`. Unset, nothing is kept.
//!
//! The time recorded is from sending to the response headers, so a slow body is not included.
//! Requests run concurrently, so a host's `total_ms` can exceed the wall time of the run.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::config::TimeoutConfig;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};

const STATS_ENV: &str = "FELUDA_REQUEST_STATS";

const USER_AGENT: &str = "feluda-license-checker/1.0";

/// Transport errors in a row after which a host is skipped for the rest of the run.
pub const GIVE_UP_AFTER: u32 = 3;

#[derive(Default, serde::Serialize)]
struct HostStats {
    requests: u64,
    /// Requests that failed before any status came back (connection, TLS, timeout).
    transport_errors: u64,
    /// Requests never sent, because the host had already been given up on.
    skipped: u64,
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

/// Host and port, so two services on one machine are given up on separately.
fn authority_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            Some(match url.port_or_known_default() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            })
        })
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

fn record_skipped(stats: &Stats, url: &str) {
    if let Ok(mut hosts) = stats.hosts.lock() {
        hosts.entry(host_of(url)).or_default().skipped += 1;
    }
}

/// Transport errors in a row, per authority. Reaching [`GIVE_UP_AFTER`] is final for the run.
static FAILURES: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(Default::default);

fn given_up(authority: &str) -> bool {
    FAILURES
        .lock()
        .map(|failures| failures.get(authority).copied().unwrap_or(0) >= GIVE_UP_AFTER)
        .unwrap_or(false)
}

/// Count a transport error against the host, or clear its count when it answered.
///
/// An error from building the request (a malformed URL) says nothing about the host, so it is not
/// counted. A response arriving after the host was given up on does not bring it back: requests
/// in flight when it went quiet can still land, and the stderr line has already been printed.
fn note_outcome(url: &str, authority: &str, result: &Result<u16, &reqwest::Error>) {
    let Ok(mut failures) = FAILURES.lock() else {
        return;
    };
    let count = failures.entry(authority.to_owned()).or_default();
    match result {
        Ok(_) if *count < GIVE_UP_AFTER => *count = 0,
        Ok(_) => {}
        Err(err) if err.is_builder() => {}
        Err(_) => {
            *count += 1;
            if *count == GIVE_UP_AFTER {
                eprintln!(
                    "⚠️ {} did not answer {GIVE_UP_AFTER} requests in a row; skipping it for the rest of this run, so licenses it would have answered may read Unknown.",
                    host_of(url)
                );
            }
        }
    }
}

fn skip(url: &str) -> FeludaError {
    log(
        LogLevel::Warn,
        &format!("Skipping {url}: its host stopped answering earlier in this run"),
    );
    if let Some(stats) = stats() {
        record_skipped(stats, url);
    }
    FeludaError::HostSkipped(host_of(url))
}

/// The `[timeouts]` section, read once for the process.
fn timeouts() -> &'static TimeoutConfig {
    static TIMEOUTS: OnceLock<TimeoutConfig> = OnceLock::new();
    TIMEOUTS.get_or_init(|| {
        crate::config::load_config()
            .map(|config| config.timeouts)
            .unwrap_or_default()
    })
}

/// A blocking client builder with feluda's User-Agent and the configured connect and request
/// timeouts. A client that needs a shorter total sets its own `timeout` on top.
pub fn client_builder() -> reqwest::blocking::ClientBuilder {
    let timeouts = timeouts();
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(timeouts.connect))
        .timeout(Duration::from_secs(timeouts.request))
}

/// [`client_builder`] for the async client.
pub fn async_client_builder() -> reqwest::ClientBuilder {
    let timeouts = timeouts();
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(timeouts.connect))
        .timeout(Duration::from_secs(timeouts.request))
}

/// `reqwest::blocking::get`, counted, with feluda's User-Agent and timeouts.
///
/// A bare `reqwest::blocking::get` sends no User-Agent, and Maven Central's search API answers
/// that with a 403.
///
/// The client is built once and shared, so lookups running side by side reuse its connections.
pub fn get(url: &str) -> FeludaResult<reqwest::blocking::Response> {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    send(url, || {
        let client = match CLIENT.get() {
            Some(client) => client,
            None => {
                let client = client_builder().build()?;
                CLIENT.get_or_init(|| client)
            }
        };
        client.get(url).send()
    })
}

/// Run a blocking request for `url` and count it, unless its host has been given up on.
pub fn send(
    url: &str,
    request: impl FnOnce() -> reqwest::Result<reqwest::blocking::Response>,
) -> FeludaResult<reqwest::blocking::Response> {
    let authority = authority_of(url);
    if given_up(&authority) {
        return Err(skip(url));
    }
    let started = Instant::now();
    let result = request();
    let status = result.as_ref().map(|r| r.status().as_u16());
    note_outcome(url, &authority, &status);
    if let Some(stats) = stats() {
        record(stats, url, started.elapsed(), status.ok());
    }
    Ok(result?)
}

/// Await an async request for `url` and count it, unless its host has been given up on.
pub async fn send_async(
    url: &str,
    request: impl Future<Output = reqwest::Result<reqwest::Response>>,
) -> FeludaResult<reqwest::Response> {
    let authority = authority_of(url);
    if given_up(&authority) {
        return Err(skip(url));
    }
    let started = Instant::now();
    let result = request.await;
    let status = result.as_ref().map(|r| r.status().as_u16());
    note_outcome(url, &authority, &status);
    if let Some(stats) = stats() {
        record(stats, url, started.elapsed(), status.ok());
    }
    Ok(result?)
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

    /// A port on localhost that refuses connections: bound once to get a free port, then closed.
    fn refused_url(path: &str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        format!("http://127.0.0.1:{port}/{path}")
    }

    #[test]
    fn a_host_is_skipped_after_failing_in_a_row() {
        let url = refused_url("pkg");
        for _ in 0..GIVE_UP_AFTER {
            assert!(matches!(get(&url), Err(FeludaError::Http(_))));
        }
        match get(&url) {
            Err(FeludaError::HostSkipped(host)) => assert_eq!(host, "127.0.0.1"),
            other => panic!("expected the host to be skipped, got {other:?}"),
        }
    }

    #[test]
    fn a_response_resets_the_failure_count() {
        let err = get(&refused_url("probe")).unwrap_err();
        let FeludaError::Http(err) = err else {
            panic!("expected a transport error, got {err:?}");
        };
        let url = "http://reset.invalid/";
        let authority = authority_of(url);

        for _ in 0..GIVE_UP_AFTER - 1 {
            note_outcome(url, &authority, &Err(&err));
        }
        note_outcome(url, &authority, &Ok(503));
        for _ in 0..GIVE_UP_AFTER - 1 {
            note_outcome(url, &authority, &Err(&err));
        }
        assert!(!given_up(&authority));

        note_outcome(url, &authority, &Err(&err));
        assert!(given_up(&authority));
        // A late answer from a request already in flight does not bring the host back.
        note_outcome(url, &authority, &Ok(200));
        assert!(given_up(&authority));
    }

    #[test]
    fn ports_are_given_up_on_separately() {
        assert_eq!(authority_of("https://pypi.org/pypi/a/json"), "pypi.org:443");
        assert_eq!(authority_of("http://127.0.0.1:8080/x"), "127.0.0.1:8080");
        assert_ne!(
            authority_of("http://127.0.0.1:8080/x"),
            authority_of("http://127.0.0.1:8081/x")
        );
    }

    /// A connect timeout bounds an address that never answers. Needs a network where
    /// 10.255.255.1 is unroutable, so it is not part of the default run.
    #[test]
    #[ignore]
    fn an_unroutable_address_fails_within_the_connect_timeout() {
        let client = client_builder().build().unwrap();
        let started = Instant::now();
        assert!(client.get("http://10.255.255.1/").send().is_err());
        assert!(started.elapsed() < Duration::from_secs(timeouts().connect + 2));
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
