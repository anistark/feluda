//! ClearlyDefined as a last-resort license source.
//!
//! Every resolution path feluda has ends somewhere. A manifest may state no license, a registry
//! may not carry one, an installed wheel may ship no metadata, and the GitHub fallback only helps
//! when the analyzer knows which repository to ask about. Those findings report as Unknown, which
//! in a compliance report is the least useful answer available.
//!
//! [ClearlyDefined](https://clearlydefined.io) curates exactly this gap: one definition per package
//! coordinate, harvested by scancode, licensee and reuse, then corrected by human curation. It is
//! free, unauthenticated, and has a batch endpoint, so a whole scan's unknowns cost one request.
//!
//! This runs as a pass over finished findings rather than inside the analyzers. All three scan
//! sources produce `Vec<LicenseInfo>` carrying an ecosystem, a name and a version, which is the
//! coordinate ClearlyDefined keys on, so one pass covers manifests, ingested SBOMs and cataloged
//! filesystems alike and nothing has to be threaded through eight language modules.
//!
//! Only `licensed.declared` is read. The same document carries per-file scan results under
//! `facets.core.discovered`, but those include the licenses of test fixtures and vendored code
//! inside the package, and reporting one of those as the package's license would be worse than
//! reporting Unknown.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use crate::cache;
use crate::cli::with_spinner;
use crate::config;
use crate::debug::{log, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, get_osi_status, is_license_restrictive, is_unresolved_license,
    LicenseInfo,
};
use crate::purl::Ecosystem;

/// Trims the per-file scan results out of the response. They are not read, and they are the bulk
/// of a definition: a batch of eight packages is 190KB with them and 12KB without.
const NO_FILES: &str = "?expand=-files";

/// Coordinates per request. The service accepts far more, but a bounded batch keeps one slow
/// response from stalling a whole scan and keeps the retry cost small when one fails.
const BATCH_SIZE: usize = 100;

/// A successful batch answers in a second or two. The service does occasionally accept a request
/// and never answer it, though, so the timeout is what bounds the cost of that rather than what
/// a slow answer needs.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Attempts per batch, and the pause between them. The failure this retries is that silent hang,
/// which a second attempt on a fresh connection usually does not hit.
const ATTEMPTS: usize = 2;
const RETRY_BACKOFF: Duration = Duration::from_secs(1);

/// Values `licensed.declared` uses to say it has no answer.
const NO_ANSWER: &[&str] = &["NOASSERTION", "NONE", "OTHER", "UNKNOWN"];

static DISABLED: OnceLock<bool> = OnceLock::new();

/// Turn the lookup off for this process, from `--no-clearlydefined`.
pub fn set_disabled(disabled: bool) {
    let _ = DISABLED.set(disabled);
}

/// The endpoint to ask, or `None` when this run must not ask at all.
fn endpoint() -> Option<String> {
    if *DISABLED.get().unwrap_or(&false) {
        log(LogLevel::Info, "ClearlyDefined disabled by flag");
        return None;
    }
    let settings = config::load_config().ok()?.clearlydefined;
    if !settings.enabled {
        log(LogLevel::Info, "ClearlyDefined disabled by configuration");
        return None;
    }
    Some(format!("{}{NO_FILES}", settings.endpoint))
}

/// Fill in licenses ClearlyDefined knows and feluda could not resolve.
///
/// Findings that already carry a license are untouched, and so are the ones whose ecosystem has no
/// ClearlyDefined coordinate. A finding this does resolve is reclassified, since its restrictiveness
/// and OSI status were decided against the Unknown it used to hold.
///
/// Returns the indices it filled in, which is what an ingested SBOM needs in order to write those
/// licenses back into the enriched copy.
///
/// Never fails: a network error, a bad response or a coordinate the service has never seen all
/// leave the finding exactly where it already was.
pub fn resolve_unknown_licenses(findings: &mut [LicenseInfo], strict: bool) -> Vec<usize> {
    let Some(endpoint) = endpoint() else {
        return Vec::new();
    };

    let pending: Vec<(usize, String)> = findings
        .iter()
        .enumerate()
        .filter(|(_, info)| is_unresolved_license(info.license.as_deref()))
        .filter_map(|(index, info)| Some((index, coordinates(info)?)))
        .collect();

    if pending.is_empty() {
        return Vec::new();
    }

    log(
        LogLevel::Info,
        &format!(
            "Asking ClearlyDefined about {} unresolved package(s)",
            pending.len()
        ),
    );

    let definitions = with_spinner("🔍: ClearlyDefined", |indicator| {
        let mut definitions = lookup(&pending, &endpoint);
        definitions.retain(|_, license| license.is_some());
        indicator.update_progress(&format!("{} resolved", definitions.len()));
        definitions
    });

    if definitions.is_empty() {
        return Vec::new();
    }

    let mut resolved = Vec::new();
    let known_licenses = fetch_licenses_from_github().unwrap_or_default();
    for (index, coordinate) in pending {
        let Some(Some(license)) = definitions.get(&coordinate) else {
            continue;
        };
        let info = &mut findings[index];
        log(
            LogLevel::Info,
            &format!("ClearlyDefined resolved {coordinate} as {license}"),
        );
        info.license = Some(license.clone());
        info.is_restrictive = is_license_restrictive(&info.license, &known_licenses, strict);
        info.osi_status = get_osi_status(license);
        resolved.push(index);
    }

    resolved
}

/// Answer every coordinate, from the cache where possible and the API for the rest.
///
/// Misses are cached as well as hits: a package ClearlyDefined has never heard of should not cost
/// a request on every subsequent run.
fn lookup(pending: &[(usize, String)], endpoint: &str) -> HashMap<String, Option<String>> {
    let cached = cache::load_clearlydefined_from_cache()
        .unwrap_or_default()
        .unwrap_or_default();

    let mut answers: HashMap<String, Option<String>> = HashMap::new();
    let mut missing: HashSet<String> = HashSet::new();
    for (_, coordinate) in pending {
        match cached.get(coordinate) {
            Some(license) => {
                answers.insert(coordinate.clone(), license.clone());
            }
            None => {
                missing.insert(coordinate.clone());
            }
        }
    }
    let missing: Vec<String> = missing.into_iter().collect();

    if missing.is_empty() {
        return answers;
    }

    let mut fetched = 0;
    for batch in missing.chunks(BATCH_SIZE) {
        let Some(definitions) = fetch_batch(batch, endpoint) else {
            // Stop rather than work through the remaining batches: whatever is wrong with the
            // service or the network is not specific to this batch, and this is a fallback that
            // must not turn a scan into a wait.
            log(
                LogLevel::Warn,
                "Giving up on ClearlyDefined for this run; unresolved licenses stay unresolved",
            );
            break;
        };
        fetched += batch.len();
        for coordinate in batch {
            let license = definitions.get(coordinate).and_then(declared_license);
            answers.insert(coordinate.clone(), license);
        }
    }

    if fetched > 0 {
        let mut to_cache = cached;
        to_cache.extend(answers.clone());
        if let Err(e) = cache::save_clearlydefined_to_cache(&to_cache) {
            log(
                LogLevel::Warn,
                &format!("Failed to save ClearlyDefined cache: {e}"),
            );
        }
    }

    answers
}

/// One batch, retried once on a fresh connection.
///
/// The service intermittently accepts a request and never answers it, and a pooled connection it
/// has done that on tends to do it again, so each attempt gets its own client rather than sharing
/// one across the batch.
fn fetch_batch(batch: &[String], endpoint: &str) -> Option<HashMap<String, Definition>> {
    (0..ATTEMPTS).find_map(|attempt| {
        if attempt > 0 {
            std::thread::sleep(RETRY_BACKOFF);
        }
        fetch_batch_once(batch, endpoint)
    })
}

fn fetch_batch_once(batch: &[String], endpoint: &str) -> Option<HashMap<String, Definition>> {
    // HTTP/1.1 by negotiation, not by preference: the service accepts an HTTP/2 POST and then
    // never answers it at all.
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("feluda/", env!("CARGO_PKG_VERSION")))
        .timeout(REQUEST_TIMEOUT)
        .http1_only()
        .build()
        .inspect_err(|e| {
            log(
                LogLevel::Warn,
                &format!("Could not build ClearlyDefined client: {e}"),
            )
        })
        .ok()?;

    let response = client
        .post(endpoint)
        .json(&batch)
        .send()
        .inspect_err(|e| {
            log(
                LogLevel::Warn,
                &format!("ClearlyDefined request failed: {e}"),
            )
        })
        .ok()?;

    if !response.status().is_success() {
        log(
            LogLevel::Warn,
            &format!("ClearlyDefined returned {}", response.status()),
        );
        return None;
    }

    response
        .json::<HashMap<String, Definition>>()
        .inspect_err(|e| {
            log(
                LogLevel::Warn,
                &format!("Could not read ClearlyDefined response: {e}"),
            )
        })
        .ok()
}

#[derive(Debug, Deserialize)]
struct Definition {
    #[serde(default)]
    licensed: Option<Licensed>,
}

#[derive(Debug, Deserialize)]
struct Licensed {
    #[serde(default)]
    declared: Option<String>,
}

/// The declared license of a definition, or `None` when it declares nothing usable.
fn declared_license(definition: &Definition) -> Option<String> {
    let declared = definition.licensed.as_ref()?.declared.as_ref()?.trim();
    if declared.is_empty() || NO_ANSWER.contains(&declared.to_ascii_uppercase().as_str()) {
        return None;
    }
    Some(declared.to_string())
}

/// The ClearlyDefined coordinate for a finding: `type/provider/namespace/name/revision`.
///
/// `None` for anything the service does not index. OS packages are out because a deb revision
/// carries an architecture suffix feluda does not record and rpm and apk are not harvested at all;
/// CRAN and Conan are not supported; and a `generic` finding is a path, not a package.
fn coordinates(info: &LicenseInfo) -> Option<String> {
    let (kind, provider) = match info.ecosystem {
        Ecosystem::Cargo => ("crate", "cratesio"),
        Ecosystem::Npm => ("npm", "npmjs"),
        Ecosystem::Pypi => ("pypi", "pypi"),
        Ecosystem::Maven => ("maven", "mavencentral"),
        Ecosystem::Gem => ("gem", "rubygems"),
        Ecosystem::Nuget => ("nuget", "nuget"),
        Ecosystem::Golang => ("go", "golang"),
        Ecosystem::Cran
        | Ecosystem::Conan
        | Ecosystem::Deb
        | Ecosystem::Rpm
        | Ecosystem::Apk
        | Ecosystem::Generic => return None,
    };

    let (namespace, name) = split_name(info.ecosystem, info.name.trim())?;
    let revision = revision(info.ecosystem, info.version.trim())?;
    Some(format!("{kind}/{provider}/{namespace}/{name}/{revision}"))
}

/// Split a display name into ClearlyDefined's namespace and name, `-` when there is no namespace.
fn split_name(ecosystem: Ecosystem, name: &str) -> Option<(String, String)> {
    if name.is_empty() {
        return None;
    }
    // Maven's namespace is the group id, and an artifact reported without one cannot be located:
    // asking with an empty namespace would be asking about a different package.
    if ecosystem == Ecosystem::Maven && !name.contains(':') {
        return None;
    }

    let split = match ecosystem {
        // An npm scope is the namespace and keeps its `@`.
        Ecosystem::Npm => name
            .strip_prefix('@')
            .and_then(|rest| rest.split_once('/'))
            .map(|(scope, package)| (format!("@{scope}"), package.to_string())),
        Ecosystem::Maven => name
            .split_once(':')
            .map(|(group, artifact)| (group.to_string(), artifact.to_string())),
        // A Go module path is a namespace of path segments plus a final name, and the separators
        // inside the namespace are escaped so the coordinate keeps its five parts.
        Ecosystem::Golang => name
            .rsplit_once('/')
            .map(|(namespace, package)| (namespace.replace('/', "%2f"), package.to_string())),
        _ => None,
    };

    let (namespace, name) = split.unwrap_or_else(|| ("-".to_string(), name.to_string()));
    if name.is_empty() || namespace.is_empty() {
        return None;
    }
    // A name carrying a separator would push the coordinate out of shape.
    if name.contains('/') {
        return None;
    }
    Some((namespace, name))
}

/// The revision to ask about, or `None` when the version is not a single concrete release.
///
/// Manifests hold ranges (`^1.2.3`, `>=2,<3`) as often as they hold versions, and a range is not
/// something ClearlyDefined can answer. Go module versions carry their `v` prefix.
fn revision(ecosystem: Ecosystem, version: &str) -> Option<String> {
    if version.is_empty() || version.len() > 64 {
        return None;
    }
    if version
        .chars()
        .any(|c| c.is_whitespace() || "^~*|,<>=()[]{}/\\".contains(c))
    {
        return None;
    }
    if !version
        .strip_prefix('v')
        .unwrap_or(version)
        .starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    if ecosystem == Ecosystem::Golang && !version.starts_with('v') {
        return Some(format!("v{version}"));
    }
    Some(version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::licenses::{LicenseCompatibility, OsiStatus};

    fn finding(ecosystem: Ecosystem, name: &str, version: &str) -> LicenseInfo {
        LicenseInfo {
            name: name.to_string(),
            version: version.to_string(),
            license: None,
            is_restrictive: false,
            compatibility: LicenseCompatibility::Unknown,
            osi_status: OsiStatus::Unknown,
            ecosystem,
            sub_project: None,
        }
    }

    #[test]
    fn test_coordinates_per_ecosystem() {
        let cases = [
            (
                finding(Ecosystem::Cargo, "serde", "1.0.219"),
                "crate/cratesio/-/serde/1.0.219",
            ),
            (
                finding(Ecosystem::Npm, "lodash", "4.17.21"),
                "npm/npmjs/-/lodash/4.17.21",
            ),
            (
                finding(Ecosystem::Npm, "@babel/core", "7.24.0"),
                "npm/npmjs/@babel/core/7.24.0",
            ),
            (
                finding(Ecosystem::Pypi, "requests", "2.32.3"),
                "pypi/pypi/-/requests/2.32.3",
            ),
            (
                finding(
                    Ecosystem::Maven,
                    "com.fasterxml.jackson.core:jackson-databind",
                    "2.15.2",
                ),
                "maven/mavencentral/com.fasterxml.jackson.core/jackson-databind/2.15.2",
            ),
            (
                finding(Ecosystem::Gem, "rails", "7.1.0"),
                "gem/rubygems/-/rails/7.1.0",
            ),
            (
                finding(Ecosystem::Nuget, "Newtonsoft.Json", "13.0.3"),
                "nuget/nuget/-/Newtonsoft.Json/13.0.3",
            ),
            (
                finding(Ecosystem::Golang, "github.com/gorilla/mux", "v1.8.1"),
                "go/golang/github.com%2fgorilla/mux/v1.8.1",
            ),
        ];

        for (info, expected) in cases {
            assert_eq!(coordinates(&info).as_deref(), Some(expected));
        }
    }

    #[test]
    fn test_go_version_gains_its_v_prefix() {
        let info = finding(Ecosystem::Golang, "github.com/gorilla/mux", "1.8.1");
        assert_eq!(
            coordinates(&info).as_deref(),
            Some("go/golang/github.com%2fgorilla/mux/v1.8.1")
        );
    }

    #[test]
    fn test_unindexed_ecosystems_have_no_coordinate() {
        for ecosystem in [
            Ecosystem::Deb,
            Ecosystem::Rpm,
            Ecosystem::Apk,
            Ecosystem::Cran,
            Ecosystem::Conan,
            Ecosystem::Generic,
        ] {
            let info = finding(ecosystem, "debian/libssl3", "3.0.15");
            assert_eq!(coordinates(&info), None, "{ecosystem:?}");
        }
    }

    #[test]
    fn test_version_ranges_are_not_asked_about() {
        for version in ["^1.2.3", ">=2.0", "1.0 - 2.0", "*", "latest", "", "~4.17"] {
            let info = finding(Ecosystem::Npm, "lodash", version);
            assert_eq!(coordinates(&info), None, "{version}");
        }
    }

    #[test]
    fn test_empty_name_has_no_coordinate() {
        assert_eq!(coordinates(&finding(Ecosystem::Npm, "  ", "1.0.0")), None);
    }

    #[test]
    fn test_maven_artifact_without_a_group_is_not_asked_about() {
        let info = finding(Ecosystem::Maven, "jackson-databind", "2.15.2");
        assert_eq!(coordinates(&info), None);
    }

    fn definition(declared: Option<&str>) -> Definition {
        Definition {
            licensed: Some(Licensed {
                declared: declared.map(str::to_string),
            }),
        }
    }

    #[test]
    fn test_declared_license_is_read() {
        assert_eq!(
            declared_license(&definition(Some("MIT OR Apache-2.0"))).as_deref(),
            Some("MIT OR Apache-2.0")
        );
    }

    #[test]
    fn test_non_answers_are_not_licenses() {
        for declared in ["NOASSERTION", "NONE", "OTHER", "unknown", "  ", ""] {
            assert_eq!(
                declared_license(&definition(Some(declared))),
                None,
                "{declared}"
            );
        }
        assert_eq!(declared_license(&definition(None)), None);
        assert_eq!(declared_license(&Definition { licensed: None }), None);
    }

    #[test]
    fn test_response_parses_into_definitions() {
        let body = r#"{
            "npm/npmjs/-/lodash/4.17.21": {
                "described": {"releaseDate": "2021-02-20"},
                "licensed": {"declared": "CC0-1.0 AND MIT"},
                "scores": {"effective": 87}
            },
            "npm/npmjs/-/nope/1.0.0": {
                "licensed": {"toolScore": {"total": 0}},
                "scores": {"effective": 0}
            }
        }"#;
        let definitions: HashMap<String, Definition> = serde_json::from_str(body).unwrap();
        assert_eq!(
            definitions
                .get("npm/npmjs/-/lodash/4.17.21")
                .and_then(declared_license)
                .as_deref(),
            Some("CC0-1.0 AND MIT")
        );
        assert_eq!(
            definitions
                .get("npm/npmjs/-/nope/1.0.0")
                .and_then(declared_license),
            None
        );
    }
}

/// Live checks against the real service, skipped by default: `cargo test -- --ignored clearlydefined`.
///
/// They are the only way to catch the service changing its coordinate scheme or its response shape
/// out from under the parsing above, which no fixture can tell us.
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore = "needs network"]
    fn the_batch_endpoint_answers_every_coordinate_shape() {
        let batch: Vec<String> = [
            "crate/cratesio/-/serde/1.0.219",
            "npm/npmjs/@babel/core/7.24.0",
            "pypi/pypi/-/requests/2.32.3",
            "maven/mavencentral/com.fasterxml.jackson.core/jackson-databind/2.15.2",
            "gem/rubygems/-/rails/7.1.0",
            "nuget/nuget/-/Newtonsoft.Json/13.0.3",
            "go/golang/github.com%2fgorilla/mux/v1.8.1",
        ]
        .iter()
        .map(|c| c.to_string())
        .collect();

        let definitions = fetch_batch(&batch, &endpoint().expect("enabled by default"))
            .expect("batch request failed");
        for coordinate in &batch {
            let declared = definitions
                .get(coordinate)
                .and_then(declared_license)
                .unwrap_or_else(|| panic!("no declared license for {coordinate}"));
            assert!(!declared.is_empty());
        }
    }

    #[test]
    #[ignore = "needs network"]
    fn an_unknown_coordinate_answers_with_no_license() {
        let batch = vec!["npm/npmjs/-/feluda-not-a-real-package/9.9.9".to_string()];
        let definitions = fetch_batch(&batch, &endpoint().expect("enabled by default"))
            .expect("batch request failed");
        assert_eq!(definitions.get(&batch[0]).and_then(declared_license), None);
    }
}
