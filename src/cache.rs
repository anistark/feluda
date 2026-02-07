//! Caching functionality for license data
//!
//! Future considerations:
//! - Per-package license cache (language:package:version keys)
//! - Dependency manifest cache with mtime tracking for incremental analysis

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::debug::{log, log_error, FeludaResult, LogLevel};
use crate::licenses::License;

const CACHE_SUBDIR: &str = "feluda";
const GITHUB_LICENSES_CACHE_FILE: &str = "github_licenses.json";
const CACHE_TTL_SECS: u64 = 30 * 24 * 60 * 60; // 30 days

const CACHE_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct CacheEntry {
    #[serde(default)]
    version: u32,
    data: HashMap<String, License>,
    timestamp: u64,
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

fn is_entry_fresh(timestamp: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age = now.saturating_sub(timestamp);
    let is_fresh = age < CACHE_TTL_SECS;
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

pub fn load_github_licenses_from_cache() -> FeludaResult<Option<HashMap<String, License>>> {
    let cache_path = github_cache_path()?;

    if !cache_path.exists() {
        log(LogLevel::Info, "No GitHub licenses cache found");
        return Ok(None);
    }

    log(LogLevel::Info, "Loading GitHub licenses from cache");

    match fs::read_to_string(&cache_path) {
        Ok(content) => match serde_json::from_str::<CacheEntry>(&content) {
            Ok(entry) => {
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
                if !is_entry_fresh(entry.timestamp) {
                    log(
                        LogLevel::Info,
                        "GitHub licenses cache is stale, will re-fetch",
                    );
                    return Ok(None);
                }
                log(
                    LogLevel::Info,
                    &format!(
                        "Successfully loaded {} licenses from cache",
                        entry.data.len()
                    ),
                );
                Ok(Some(entry.data))
            }
            Err(e) => {
                log(
                    LogLevel::Warn,
                    &format!("Corrupt cache file, will re-fetch: {e}"),
                );
                Ok(None)
            }
        },
        Err(e) => {
            log(
                LogLevel::Warn,
                &format!("Failed to read cache file, will re-fetch: {e}"),
            );
            Ok(None)
        }
    }
}

pub fn save_github_licenses_to_cache(licenses: &HashMap<String, License>) -> FeludaResult<()> {
    let cache_dir = ensure_cache_dir()?;
    let cache_path = cache_dir.join(GITHUB_LICENSES_CACHE_FILE);

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let json = match serde_json::to_string_pretty(&serde_json::json!({
        "version": CACHE_VERSION,
        "data": licenses,
        "timestamp": timestamp,
    })) {
        Ok(json) => json,
        Err(e) => {
            log_error("Failed to serialize cache", &e);
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()).into());
        }
    };

    fs::write(&cache_path, json).inspect_err(|e| log_error("Failed to write cache file", e))?;

    log(
        LogLevel::Info,
        &format!(
            "Saved {} licenses to cache at {}",
            licenses.len(),
            cache_path.display()
        ),
    );

    Ok(())
}

pub fn clear_github_licenses_cache() -> FeludaResult<()> {
    let cache_path = github_cache_path()?;

    if cache_path.exists() {
        fs::remove_file(&cache_path).inspect_err(|e| log_error("Failed to clear cache", e))?;
        log(LogLevel::Info, "Cleared GitHub licenses cache");
    } else {
        log(LogLevel::Info, "No cache to clear");
    }

    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct CacheStatus {
    pub exists: bool,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub is_fresh: bool,
    pub age_secs: u64,
    pub license_count: usize,
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

    pub fn print_status(&self) {
        if !self.exists {
            println!("\n📦 Cache Status: EMPTY");
            println!("   No cache found at: {}", self.path.display());
            println!("   Cache will be created on next license analysis.\n");
            return;
        }

        let health = if self.is_fresh {
            "✓ FRESH"
        } else {
            "✗ STALE"
        };

        println!("\n📦 Cache Status: {health}");
        println!("   Location: {}", self.path.display());
        println!("   Size: {}", Self::format_size(self.size_bytes));
        println!("   Age: {}", Self::format_age(self.age_secs));
        println!("   Licenses cached: {}", self.license_count);
        println!();
    }
}

/// Visible for testing: parse a cache entry from JSON content and check freshness.
#[cfg(test)]
fn load_from_content(content: &str) -> Option<HashMap<String, License>> {
    match serde_json::from_str::<CacheEntry>(content) {
        Ok(entry) if entry.version == CACHE_VERSION && is_entry_fresh(entry.timestamp) => {
            Some(entry.data)
        }
        _ => None,
    }
}

pub fn get_cache_status() -> FeludaResult<CacheStatus> {
    let cache_path = github_cache_path()?;

    if !cache_path.exists() {
        return Ok(CacheStatus {
            exists: false,
            path: cache_path,
            size_bytes: 0,
            is_fresh: false,
            age_secs: 0,
            license_count: 0,
        });
    }

    let size_bytes = fs::metadata(&cache_path)?.len();

    let (is_fresh, age_secs, license_count) = match fs::read_to_string(&cache_path) {
        Ok(content) => match serde_json::from_str::<CacheEntry>(&content) {
            Ok(entry) => (
                is_entry_fresh(entry.timestamp),
                entry_age_secs(entry.timestamp),
                entry.data.len(),
            ),
            Err(_) => (false, 0, 0),
        },
        Err(_) => (false, 0, 0),
    };

    Ok(CacheStatus {
        exists: true,
        path: cache_path,
        size_bytes,
        is_fresh,
        age_secs,
        license_count,
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

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn fresh_entry_is_fresh() {
        assert!(is_entry_fresh(now_secs()));
    }

    #[test]
    fn stale_entry_is_not_fresh() {
        let old = now_secs() - CACHE_TTL_SECS - 1;
        assert!(!is_entry_fresh(old));
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
        };
        let json = serde_json::to_string(&entry).unwrap();
        let decoded: CacheEntry = serde_json::from_str(&json).unwrap();
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
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(load_from_content(&json).is_none());
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
