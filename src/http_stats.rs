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
//! A host that answers 429 or 503 is asked again, up to [`ATTEMPTS`] times in all. The pause is
//! what its `Retry-After` header asks for, else a backoff that starts at one second, doubles and
//! is capped at eight, with jitter so lookups running side by side do not ask again in step. A
//! request pauses [`RETRY_BUDGET`] at most in total: a `Retry-After` that would take it past that
//! is not waited out, and the response goes back to the caller as it came. The first time a host
//! is still answering 429 or 503 when its retries run out, one line on stderr says so.
//!
//! Set `FELUDA_REQUEST_STATS` to a file path and every request feluda sends is counted per host,
//! with its status and how long it took to answer; the tally is written to that path as JSON when
//! the run ends. Skipped requests are counted apart, under `skipped`. Unset, nothing is kept.
//!
//! The time recorded is from sending to the response headers, so a slow body is not included.
//! Requests run concurrently, so a host's `total_ms` can exceed the wall time of the run.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use reqwest::header::{HeaderMap, RETRY_AFTER};

use crate::config::timeouts;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};

const STATS_ENV: &str = "FELUDA_REQUEST_STATS";

const USER_AGENT: &str = "feluda-license-checker/1.0";

/// Transport errors in a row after which a host is skipped for the rest of the run.
pub const GIVE_UP_AFTER: u32 = 3;

/// Attempts per request, the first included, while the host answers 429 or 503.
pub const ATTEMPTS: u32 = 4;

/// The first pause when the response names no `Retry-After`. It doubles with each retry.
const BACKOFF: Duration = Duration::from_secs(1);

/// The longest pause the backoff reaches on its own.
const MAX_BACKOFF: Duration = Duration::from_secs(8);

/// The most one request pauses between its attempts, `Retry-After` included.
pub const RETRY_BUDGET: Duration = Duration::from_secs(30);

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
    /// Requests sent again after a 429 or 503, and the time spent pausing before them.
    retries: u64,
    retry_wait_ms: f64,
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

fn record_retry(stats: &Stats, url: &str, pause: Duration) {
    if let Ok(mut hosts) = stats.hosts.lock() {
        let host = hosts.entry(host_of(url)).or_default();
        host.retries += 1;
        host.retry_wait_ms += pause.as_secs_f64() * 1000.0;
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

/// Whether a status means the host is busy rather than that the request is wrong: 429 Too Many
/// Requests and 503 Service Unavailable. These are the statuses [`send`] asks again on, so a caller
/// that sees one has already had its retries.
pub fn is_busy(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 503)
}

/// How long `Retry-After` asks to wait: delay seconds, or an HTTP date (a date already past is no
/// wait). A value that is neither is ignored, and the backoff applies.
fn retry_after(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let at = SystemTime::from(date);
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

/// The pause before retry number `retry` (1 for the first) when the host named none: [`BACKOFF`]
/// doubled per retry up to [`MAX_BACKOFF`], of which the upper half is random.
fn backoff(retry: u32) -> Duration {
    let full = BACKOFF
        .saturating_mul(1 << retry.saturating_sub(1).min(16))
        .min(MAX_BACKOFF);
    full / 2 + jitter(full / 2)
}

/// A random duration from zero to `max`. `RandomState` is seeded per instance, which is all the
/// randomness spreading out retries needs.
fn jitter(max: Duration) -> Duration {
    use std::hash::BuildHasher;
    let millis = max.as_millis() as u64;
    let random = std::collections::hash_map::RandomState::new().hash_one(Instant::now());
    Duration::from_millis(random % (millis + 1))
}

/// The pause before asking again, or `None` to hand this response back.
///
/// `attempts` is how many have been sent so far and `paused` how long this request has already
/// waited between them.
fn pause_before_retry(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    attempts: u32,
    paused: Duration,
) -> Option<Duration> {
    if !is_busy(status) || attempts >= ATTEMPTS {
        return None;
    }
    let pause = retry_after(headers, SystemTime::now()).unwrap_or_else(|| backoff(attempts));
    (paused + pause <= RETRY_BUDGET).then_some(pause)
}

/// Note that `url` is about to be asked again after `pause`.
fn note_retry(url: &str, status: reqwest::StatusCode, attempts: u32, pause: Duration) {
    log(
        LogLevel::Warn,
        &format!(
            "{} answered {status}; asking again in {:.1}s (attempt {}/{ATTEMPTS})",
            host_of(url),
            pause.as_secs_f64(),
            attempts + 1
        ),
    );
    if let Some(stats) = stats() {
        record_retry(stats, url, pause);
    }
}

/// Hosts that have been reported on stderr as still busy once their retries ran out.
static REPORTED_BUSY: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// Say once per host, on stderr, that a request is going back to its caller still busy.
fn note_still_busy(url: &str, status: reqwest::StatusCode) {
    if !is_busy(status) {
        return;
    }
    let host = host_of(url);
    let first = REPORTED_BUSY
        .lock()
        .map(|mut reported| reported.insert(host.clone()))
        .unwrap_or(false);
    if first {
        eprintln!(
            "⚠️ {host} is still answering {status} after retrying; licenses it would have answered may read Unknown."
        );
    }
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
    request(url, None)
}

/// [`get`] with an explicit `Accept: application/json`.
///
/// The npm registry answers an abbreviated document, with no `license` on any version, to the
/// `vnd.npm.install-v1+json` npm asks for by default, so the full one has to be named.
pub fn get_json(url: &str) -> FeludaResult<reqwest::blocking::Response> {
    request(url, Some("application/json"))
}

/// A blocking GET from the shared client, which is built once so lookups running side by side reuse
/// its connections.
fn request(url: &str, accept: Option<&str>) -> FeludaResult<reqwest::blocking::Response> {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    send(url, || {
        let client = match CLIENT.get() {
            Some(client) => client,
            None => {
                let client = client_builder().build()?;
                CLIENT.get_or_init(|| client)
            }
        };
        let builder = client.get(url);
        match accept {
            Some(accept) => builder.header(reqwest::header::ACCEPT, accept).send(),
            None => builder.send(),
        }
    })
}

/// Run a blocking request for `url` and count it, unless its host has been given up on.
///
/// `request` is called again while the host answers 429 or 503, as the module docs describe, so
/// it has to build the request afresh each time. What comes back is the last response, which can
/// still be one of those statuses.
pub fn send(
    url: &str,
    request: impl Fn() -> reqwest::Result<reqwest::blocking::Response>,
) -> FeludaResult<reqwest::blocking::Response> {
    let authority = authority_of(url);
    let mut attempts = 0;
    let mut paused = Duration::ZERO;
    loop {
        let response = send_once(url, &authority, &request)?;
        attempts += 1;
        let status = response.status();
        let Some(pause) = pause_before_retry(status, response.headers(), attempts, paused) else {
            note_still_busy(url, status);
            return Ok(response);
        };
        drop(response);
        note_retry(url, status, attempts, pause);
        std::thread::sleep(pause);
        paused += pause;
    }
}

fn send_once(
    url: &str,
    authority: &str,
    request: impl FnOnce() -> reqwest::Result<reqwest::blocking::Response>,
) -> FeludaResult<reqwest::blocking::Response> {
    if given_up(authority) {
        return Err(skip(url));
    }
    let started = Instant::now();
    let result = request();
    let status = result.as_ref().map(|r| r.status().as_u16());
    note_outcome(url, authority, &status);
    if let Some(stats) = stats() {
        record(stats, url, started.elapsed(), status.ok());
    }
    Ok(result?)
}

/// [`send`] for the async client.
pub async fn send_async<F>(url: &str, request: impl Fn() -> F) -> FeludaResult<reqwest::Response>
where
    F: Future<Output = reqwest::Result<reqwest::Response>>,
{
    let authority = authority_of(url);
    let mut attempts = 0;
    let mut paused = Duration::ZERO;
    loop {
        let response = send_once_async(url, &authority, &request).await?;
        attempts += 1;
        let status = response.status();
        let Some(pause) = pause_before_retry(status, response.headers(), attempts, paused) else {
            note_still_busy(url, status);
            return Ok(response);
        };
        drop(response);
        note_retry(url, status, attempts, pause);
        tokio::time::sleep(pause).await;
        paused += pause;
    }
}

async fn send_once_async<F>(
    url: &str,
    authority: &str,
    request: impl FnOnce() -> F,
) -> FeludaResult<reqwest::Response>
where
    F: Future<Output = reqwest::Result<reqwest::Response>>,
{
    if given_up(authority) {
        return Err(skip(url));
    }
    let started = Instant::now();
    let result = request().await;
    let status = result.as_ref().map(|r| r.status().as_u16());
    note_outcome(url, authority, &status);
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

    /// A server on localhost that answers each connection with the next of `responses`, then
    /// stops. Returns its base URL and a handle that yields how many requests it served.
    fn stub(responses: Vec<&'static str>) -> (String, std::thread::JoinHandle<usize>) {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut served = 0;
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).unwrap();
                stream.write_all(response.as_bytes()).unwrap();
                served += 1;
            }
            served
        });
        (format!("http://127.0.0.1:{port}/"), server)
    }

    const BUSY_NOW: &str = "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const UNAVAILABLE_NOW: &str = "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    #[test]
    fn a_busy_answer_is_asked_again() {
        let (url, server) = stub(vec![BUSY_NOW, UNAVAILABLE_NOW, OK]);
        let response = get(&url).unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(server.join().unwrap(), 3);
    }

    #[test]
    fn retries_stop_after_the_last_attempt() {
        let (url, server) = stub(vec![UNAVAILABLE_NOW; ATTEMPTS as usize]);
        let response = get(&url).unwrap();
        assert_eq!(response.status(), 503);
        assert_eq!(server.join().unwrap(), ATTEMPTS as usize);
    }

    #[test]
    fn a_retry_after_past_the_budget_is_not_waited_out() {
        let (url, server) = stub(vec![
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 3600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);
        let started = Instant::now();
        let response = get(&url).unwrap();
        assert_eq!(response.status(), 429);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(server.join().unwrap(), 1);
    }

    #[test]
    fn other_errors_are_not_retried() {
        let (url, server) = stub(vec![
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);
        assert_eq!(get(&url).unwrap().status(), 500);
        assert_eq!(server.join().unwrap(), 1);
    }

    fn retry_after_header(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, value.parse().unwrap());
        headers
    }

    #[test]
    fn retry_after_reads_seconds_and_dates() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_445_412_480); // 2015-10-21 07:28:00
        assert_eq!(
            retry_after(&retry_after_header("120"), now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            retry_after(&retry_after_header("Wed, 21 Oct 2015 07:28:30 GMT"), now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            retry_after(&retry_after_header("Wed, 21 Oct 2015 07:00:00 GMT"), now),
            Some(Duration::ZERO)
        );
        assert_eq!(retry_after(&retry_after_header("soon"), now), None);
        assert_eq!(retry_after(&HeaderMap::new(), now), None);
    }

    #[test]
    fn backoff_doubles_up_to_its_cap() {
        for _ in 0..50 {
            let first = backoff(1);
            assert!(first >= BACKOFF / 2 && first <= BACKOFF, "{first:?}");
            let third = backoff(3);
            assert!(third >= BACKOFF * 2 && third <= BACKOFF * 4, "{third:?}");
            let late = backoff(40);
            assert!(late >= MAX_BACKOFF / 2 && late <= MAX_BACKOFF, "{late:?}");
        }
    }

    #[test]
    fn only_busy_statuses_within_budget_are_retried() {
        use reqwest::StatusCode;

        let none = HeaderMap::new();
        let busy = StatusCode::TOO_MANY_REQUESTS;
        assert!(pause_before_retry(StatusCode::NOT_FOUND, &none, 1, Duration::ZERO).is_none());
        assert!(pause_before_retry(StatusCode::OK, &none, 1, Duration::ZERO).is_none());
        assert!(
            pause_before_retry(StatusCode::SERVICE_UNAVAILABLE, &none, 1, Duration::ZERO).is_some()
        );
        assert!(pause_before_retry(busy, &none, ATTEMPTS, Duration::ZERO).is_none());

        let ten = retry_after_header("10");
        assert_eq!(
            pause_before_retry(busy, &ten, 1, Duration::ZERO),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            pause_before_retry(busy, &ten, 2, RETRY_BUDGET - Duration::from_secs(10)),
            Some(Duration::from_secs(10))
        );
        assert!(pause_before_retry(busy, &ten, 2, RETRY_BUDGET - Duration::from_secs(9)).is_none());
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
