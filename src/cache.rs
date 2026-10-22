//! Caching functionality for license data
//!
//! Future considerations:
//! - Per-package license cache (language:package:version keys)
//! - Dependency manifest cache with mtime tracking for incremental analysis

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::debug::{log, log_error, FeludaResult, LogLevel};
use crate::licenses::{License, OsiStatus};

const CACHE_SUBDIR: &str = "feluda";
const GITHUB_LICENSES_CACHE_FILE: &str = "github_licenses.json";
const OSI_LICENSES_CACHE_FILE: &str = "osi_licenses.json";
const CACHE_TTL_SECS: u64 = 30 * 24 * 60 * 60; // 30 days
const OSI_REFRESH_RETRY_SECS: u64 = 6 * 60 * 60; // 6 hours

/// ClearlyDefined answers are cached separately and expire sooner: the GitHub table is the SPDX
/// license list, which barely moves, while ClearlyDefined definitions gain curations continuously.
const CLEARLYDEFINED_CACHE_FILE: &str = "clearlydefined.json";
const CLEARLYDEFINED_TTL_SECS: u64 = 7 * 24 * 60 * 60; // 7 days

const CACHE_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct CacheEntry<T> {
    #[serde(default)]
    version: u32,
    data: HashMap<String, T>,
    timestamp: u64,
    /// Until when a stale entry is used without asking again, after a refresh that failed. Only
    /// the OSI table writes it; `timestamp` keeps saying when the data was last fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_after: Option<u64>,
}

fn cache_dir_path() -> FeludaResult<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine user cache directory",
        )
    })?;
    Ok(base.join(CACHE_SUBDIR))
}

fn ensure_cache_dir() -> FeludaResult<PathBuf> {
    let cache_dir = cache_dir_path()?;
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir)
            .inspect_err(|e| log_error("Failed to create cache directory", e))?;
    }
    Ok(cache_dir)
}

fn github_cache_path() -> FeludaResult<PathBuf> {
    Ok(cache_dir_path()?.join(GITHUB_LICENSES_CACHE_FILE))
}

fn clearlydefined_cache_path() -> FeludaResult<PathBuf> {
    Ok(cache_dir_path()?.join(CLEARLYDEFINED_CACHE_FILE))
}

fn osi_cache_path() -> FeludaResult<PathBuf> {
    Ok(cache_dir_path()?.join(OSI_LICENSES_CACHE_FILE))
}

fn is_entry_fresh_for(timestamp: u64, ttl_secs: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age = now.saturating_sub(timestamp);
    let is_fresh = age < ttl_secs;
    log(
        LogLevel::Info,
        &format!("Cache age: {age} seconds (fresh: {is_fresh})"),
    );
    is_fresh
}

fn entry_age_secs(timestamp: u64) -> u64 {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(timestamp)
}

/// A cache entry, and whether it is due a refresh.
#[derive(Debug)]
pub(crate) enum CacheLookup<T> {
    /// Use it as is: within its TTL, or inside the retry window after a failed refresh.
    Fresh(HashMap<String, T>),
    /// Past its TTL; `timestamp` is when it was last fetched, kept so a deferral can keep it.
    Stale {
        data: HashMap<String, T>,
        timestamp: u64,
    },
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read a cache file and report whether its entry is fresh or stale.
///
/// A missing, older-layout, or unreadable cache returns `None`: the caller's fallback is to fetch,
/// which is what it would have done anyway.
fn load_cache_with_state<T: serde::de::DeserializeOwned>(
    path: &Path,
    ttl_secs: u64,
) -> FeludaResult<Option<CacheLookup<T>>> {
    if !path.exists() {
        log(
            LogLevel::Info,
            &format!("No cache found at {}", path.display()),
        );
        return Ok(None);
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            log(
                LogLevel::Warn,
                &format!("Failed to read cache file, will re-fetch: {e}"),
            );
            return Ok(None);
        }
    };

    let entry = match serde_json::from_str::<CacheEntry<T>>(&content) {
        Ok(entry) => entry,
        Err(e) => {
            log(
                LogLevel::Warn,
                &format!("Corrupt cache file, will re-fetch: {e}"),
            );
            return Ok(None);
        }
    };

    if entry.version != CACHE_VERSION {
        log(
            LogLevel::Info,
            &format!(
                "Cache version mismatch (got {}, expected {CACHE_VERSION}), will re-fetch",
                entry.version
            ),
        );
        return Ok(None);
    }

    if !is_entry_fresh_for(entry.timestamp, ttl_secs) {
        if entry
            .retry_after
            .is_some_and(|retry_after| retry_after > now_secs())
        {
            log(
                LogLevel::Info,
                "Cache is stale, but the last refresh failed recently; not asking again yet",
            );
            return Ok(Some(CacheLookup::Fresh(entry.data)));
        }
        log(LogLevel::Info, "Cache is stale, will re-fetch");
        return Ok(Some(CacheLookup::Stale {
            data: entry.data,
            timestamp: entry.timestamp,
        }));
    }

    log(
        LogLevel::Info,
        &format!(
            "Loaded {} cached entries from {}",
            entry.data.len(),
            path.display()
        ),
    );
    Ok(Some(CacheLookup::Fresh(entry.data)))
}

fn load_cache<T: serde::de::DeserializeOwned>(
    path: &Path,
    ttl_secs: u64,
) -> FeludaResult<Option<HashMap<String, T>>> {
    match load_cache_with_state(path, ttl_secs)? {
        Some(CacheLookup::Fresh(data)) => Ok(Some(data)),
        Some(CacheLookup::Stale { .. }) | None => Ok(None),
    }
}

fn save_cache<T: serde::Serialize>(path: &Path, data: &HashMap<String, T>) -> FeludaResult<()> {
    save_cache_entry(path, data, now_secs(), None)
}

fn save_cache_entry<T: serde::Serialize>(
    path: &Path,
    data: &HashMap<String, T>,
    timestamp: u64,
    retry_after: Option<u64>,
) -> FeludaResult<()> {
    ensure_cache_dir()?;

    let mut entry = serde_json::json!({
        "version": CACHE_VERSION,
        "data": data,
        "timestamp": timestamp,
    });
    if let Some(retry_after) = retry_after {
        entry["retry_after"] = retry_after.into();
    }
    let json = match serde_json::to_string_pretty(&entry) {
        Ok(json) => json,
        Err(e) => {
            log_error("Failed to serialize cache", &e);
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()).into());
        }
    };

    fs::write(path, json).inspect_err(|e| log_error("Failed to write cache file", e))?;

    log(
        LogLevel::Info,
        &format!(
            "Saved {} entries to cache at {}",
            data.len(),
            path.display()
        ),
    );

    Ok(())
}

pub fn load_github_licenses_from_cache() -> FeludaResult<Option<HashMap<String, License>>> {
    load_cache(&github_cache_path()?, CACHE_TTL_SECS)
}

pub fn save_github_licenses_to_cache(licenses: &HashMap<String, License>) -> FeludaResult<()> {
    save_cache(&github_cache_path()?, licenses)
}

/// Load the OSI table and preserve a stale entry so callers can use it if refresh fails.
pub(crate) fn load_osi_licenses_from_cache() -> FeludaResult<Option<CacheLookup<OsiStatus>>> {
    load_cache_with_state(&osi_cache_path()?, CACHE_TTL_SECS)
}

pub fn save_osi_licenses_to_cache(licenses: &HashMap<String, OsiStatus>) -> FeludaResult<()> {
    save_cache(&osi_cache_path()?, licenses)
}

/// After a failed refresh, keep using `licenses` for six hours before asking again.
///
/// `timestamp` is when the table was last fetched, and is written back unchanged, so `feluda
/// cache` keeps reporting the table as stale with its real age. Without any table to keep, the
/// caller passes the built-in list with a timestamp of 0, so it reads as never fetched.
pub(crate) fn defer_osi_refresh(
    licenses: &HashMap<String, OsiStatus>,
    timestamp: u64,
) -> FeludaResult<()> {
    save_cache_entry(
        &osi_cache_path()?,
        licenses,
        timestamp,
        Some(now_secs() + OSI_REFRESH_RETRY_SECS),
    )
}

/// Declared licenses ClearlyDefined answered with, keyed by coordinate.
///
/// The value is `None` for a coordinate ClearlyDefined has no answer for. Those are cached too, so
/// a package it has never heard of is not asked about again on every run.
pub fn load_clearlydefined_from_cache() -> FeludaResult<Option<HashMap<String, Option<String>>>> {
    load_cache(&clearlydefined_cache_path()?, CLEARLYDEFINED_TTL_SECS)
}

pub fn save_clearlydefined_to_cache(
    definitions: &HashMap<String, Option<String>>,
) -> FeludaResult<()> {
    save_cache(&clearlydefined_cache_path()?, definitions)
}

/// Clear every cache feluda keeps.
pub fn clear_all_caches() -> FeludaResult<()> {
    let mut cleared = false;
    for path in [
        github_cache_path()?,
        osi_cache_path()?,
        clearlydefined_cache_path()?,
    ] {
        if path.exists() {
            fs::remove_file(&path).inspect_err(|e| log_error("Failed to clear cache", e))?;
            log(
                LogLevel::Info,
                &format!("Cleared cache at {}", path.display()),
            );
            cleared = true;
        }
    }
    if !cleared {
        log(LogLevel::Info, "No cache to clear");
    }

    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct CacheStatus {
    pub exists: bool,
    /// What this cache holds, for the status line.
    pub label: &'static str,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub is_fresh: bool,
    /// Seconds since the data was fetched, or `None` when it never was (the OSI built-in list).
    pub age_secs: Option<u64>,
    pub license_count: usize,
    /// Seconds until the next attempt, when the last refresh failed and Feluda is holding off.
    pub retry_in_secs: Option<u64>,
}

impl CacheStatus {
    fn format_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;

        if bytes < KB {
            format!("{bytes} B")
        } else if bytes < MB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        }
    }

    fn format_age(secs: u64) -> String {
        const HOUR: u64 = 3600;
        const DAY: u64 = 24 * HOUR;

        if secs < 60 {
            "just now".to_string()
        } else if secs < HOUR {
            format!("{} minutes ago", secs / 60)
        } else if secs < DAY {
            format!("{} hours ago", secs / HOUR)
        } else {
            format!("{} days ago", secs / DAY)
        }
    }

    fn format_duration(secs: u64) -> String {
        if secs < 3600 {
            format!("{} minutes", secs.div_ceil(60))
        } else {
            format!("{} hours", secs.div_ceil(3600))
        }
    }

    pub fn print_status(&self) {
        if !self.exists {
            println!("\n📦 {} Cache: EMPTY", self.label);
            println!("   No cache found at: {}", self.path.display());
            println!("   Cache will be created on next license analysis.\n");
            return;
        }

        let health = if self.is_fresh {
            "✓ FRESH"
        } else {
            "✗ STALE"
        };

        println!("\n📦 {} Cache: {health}", self.label);
        println!("   Location: {}", self.path.display());
        println!("   Size: {}", Self::format_size(self.size_bytes));
        match self.age_secs {
            Some(age) => println!("   Age: {}", Self::format_age(age)),
            None => println!("   Age: never fetched (built-in list)"),
        }
        println!("   Entries cached: {}", self.license_count);
        if let Some(retry_in) = self.retry_in_secs {
            println!(
                "   Last refresh failed; next attempt in {}",
                Self::format_duration(retry_in)
            );
        }
        println!();
    }
}

/// Visible for testing: parse a cache entry from JSON content and check freshness.
#[cfg(test)]
fn load_from_content(content: &str) -> Option<HashMap<String, License>> {
    match serde_json::from_str::<CacheEntry<License>>(content) {
        Ok(entry)
            if entry.version == CACHE_VERSION
                && is_entry_fresh_for(entry.timestamp, CACHE_TTL_SECS) =>
        {
            Some(entry.data)
        }
        _ => None,
    }
}

pub fn get_cache_status() -> FeludaResult<CacheStatus> {
    status_of::<License>(github_cache_path()?, "GitHub Licenses", CACHE_TTL_SECS)
}

/// The OSI approval table cache, refreshed on the same 30-day cadence as GitHub's license table.
pub fn get_osi_cache_status() -> FeludaResult<CacheStatus> {
    status_of::<OsiStatus>(osi_cache_path()?, "OSI Licenses", CACHE_TTL_SECS)
}

/// The ClearlyDefined answers cache, reported next to the GitHub one so `feluda cache` accounts
/// for everything `--clear` would remove.
pub fn get_clearlydefined_cache_status() -> FeludaResult<CacheStatus> {
    status_of::<Option<String>>(
        clearlydefined_cache_path()?,
        "ClearlyDefined Answers",
        CLEARLYDEFINED_TTL_SECS,
    )
}

fn status_of<T: serde::de::DeserializeOwned>(
    cache_path: PathBuf,
    label: &'static str,
    ttl_secs: u64,
) -> FeludaResult<CacheStatus> {
    if !cache_path.exists() {
        return Ok(CacheStatus {
            exists: false,
            label,
            path: cache_path,
            size_bytes: 0,
            is_fresh: false,
            age_secs: None,
            license_count: 0,
            retry_in_secs: None,
        });
    }

    let size_bytes = fs::metadata(&cache_path)?.len();

    let entry = fs::read_to_string(&cache_path)
        .ok()
        .and_then(|content| serde_json::from_str::<CacheEntry<T>>(&content).ok());
    let (is_fresh, age_secs, license_count, retry_in_secs) = match entry {
        Some(entry) => (
            is_entry_fresh_for(entry.timestamp, ttl_secs),
            (entry.timestamp != 0).then(|| entry_age_secs(entry.timestamp)),
            entry.data.len(),
            entry
                .retry_after
                .and_then(|retry_after| retry_after.checked_sub(now_secs()))
                .filter(|&secs| secs > 0),
        ),
        None => (false, Some(0), 0, None),
    };

    Ok(CacheStatus {
        exists: true,
        label,
        path: cache_path,
        size_bytes,
        is_fresh,
        age_secs,
        license_count,
        retry_in_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_license(id: &str) -> License {
        License {
            title: format!("{id} License"),
            spdx_id: id.to_string(),
            permissions: vec!["commercial-use".into()],
            conditions: vec!["include-copyright".into()],
            limitations: vec!["liability".into()],
        }
    }

    #[test]
    fn fresh_entry_is_fresh() {
        assert!(is_entry_fresh_for(now_secs(), CACHE_TTL_SECS));
    }

    #[test]
    fn stale_entry_is_not_fresh() {
        let old = now_secs() - CACHE_TTL_SECS - 1;
        assert!(!is_entry_fresh_for(old, CACHE_TTL_SECS));
    }

    #[test]
    fn entry_age_is_correct() {
        let ts = now_secs() - 120;
        let age = entry_age_secs(ts);
        assert!((120..=122).contains(&age));
    }

    #[test]
    fn serde_round_trip() {
        let mut data = HashMap::new();
        data.insert("MIT".to_string(), make_license("MIT"));
        let entry = CacheEntry {
            version: CACHE_VERSION,
            data,
            timestamp: now_secs(),
            retry_after: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let decoded: CacheEntry<License> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.data.len(), 1);
        assert_eq!(decoded.data["MIT"].spdx_id, "MIT");
        assert_eq!(decoded.timestamp, entry.timestamp);
        assert_eq!(decoded.version, CACHE_VERSION);
    }

    #[test]
    fn load_from_content_fresh() {
        let mut data = HashMap::new();
        data.insert("MIT".to_string(), make_license("MIT"));
        let entry = CacheEntry {
            version: CACHE_VERSION,
            data,
            timestamp: now_secs(),
            retry_after: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let result = load_from_content(&json);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn load_from_content_stale() {
        let mut data = HashMap::new();
        data.insert("MIT".to_string(), make_license("MIT"));
        let entry = CacheEntry {
            version: CACHE_VERSION,
            data,
            timestamp: now_secs() - CACHE_TTL_SECS - 1,
            retry_after: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(load_from_content(&json).is_none());
    }

    #[test]
    fn a_stale_entry_inside_its_retry_window_is_used_without_a_refresh() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("osi_licenses.json");
        let data = HashMap::from([("MIT".to_string(), OsiStatus::Approved)]);
        let stale = now_secs() - CACHE_TTL_SECS - 1;

        fs::write(
            &path,
            serde_json::to_string(&CacheEntry {
                version: CACHE_VERSION,
                data: data.clone(),
                timestamp: stale,
                retry_after: Some(now_secs() + 60),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            load_cache_with_state::<OsiStatus>(&path, CACHE_TTL_SECS).unwrap(),
            Some(CacheLookup::Fresh(_))
        ));
        let status = status_of::<OsiStatus>(path.clone(), "OSI Licenses", CACHE_TTL_SECS).unwrap();
        assert!(!status.is_fresh);
        assert!(status.retry_in_secs.is_some());

        fs::write(
            &path,
            serde_json::to_string(&CacheEntry {
                version: CACHE_VERSION,
                data,
                timestamp: stale,
                retry_after: Some(now_secs() - 60),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            load_cache_with_state::<OsiStatus>(&path, CACHE_TTL_SECS).unwrap(),
            Some(CacheLookup::Stale { timestamp, .. }) if timestamp == stale
        ));
        let status = status_of::<OsiStatus>(path, "OSI Licenses", CACHE_TTL_SECS).unwrap();
        assert_eq!(status.retry_in_secs, None);
    }

    #[test]
    fn load_from_content_corrupt() {
        assert!(load_from_content("not valid json {{{").is_none());
        assert!(load_from_content("").is_none());
        assert!(load_from_content("{}").is_none());
    }

    #[test]
    fn format_size_bytes() {
        assert_eq!(CacheStatus::format_size(500), "500 B");
    }

    #[test]
    fn format_size_kilobytes() {
        assert_eq!(CacheStatus::format_size(2048), "2.00 KB");
    }

    #[test]
    fn format_size_megabytes() {
        assert_eq!(CacheStatus::format_size(1_048_576), "1.00 MB");
    }

    #[test]
    fn format_age_just_now() {
        assert_eq!(CacheStatus::format_age(30), "just now");
        assert_eq!(CacheStatus::format_age(0), "just now");
    }

    #[test]
    fn format_age_minutes() {
        assert_eq!(CacheStatus::format_age(300), "5 minutes ago");
    }

    #[test]
    fn format_age_hours() {
        assert_eq!(CacheStatus::format_age(7200), "2 hours ago");
    }

    #[test]
    fn format_age_days() {
        assert_eq!(CacheStatus::format_age(172_800), "2 days ago");
    }
}
