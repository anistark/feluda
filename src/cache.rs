//! Caching functionality for license data
//!
//! Future considerations:
//! - Per-package license cache (language:package:version keys)
//! - Dependency manifest cache with mtime tracking for incremental analysis

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::debug::{log, log_error, FeludaResult, LogLevel};
use crate::licenses::License;

const CACHE_DIR: &str = ".feluda/cache";
const GITHUB_LICENSES_CACHE_FILE: &str = "github_licenses.json";
const CACHE_TTL_SECS: u64 = 30 * 24 * 60 * 60; // 30 days

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct CacheEntry {
    data: HashMap<String, License>,
    timestamp: u64,
}

fn get_cache_dir() -> FeludaResult<PathBuf> {
    let cache_dir = PathBuf::from(CACHE_DIR);

    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir).map_err(|e| {
            log_error("Failed to create cache directory", &e);
            e
        })?;
    }

    Ok(cache_dir)
}

fn get_github_cache_path() -> FeludaResult<PathBuf> {
    let cache_dir = get_cache_dir()?;
    Ok(cache_dir.join(GITHUB_LICENSES_CACHE_FILE))
}

fn is_cache_fresh(path: &Path) -> bool {
    match path.metadata() {
        Ok(metadata) => match metadata.modified() {
            Ok(modified_time) => match SystemTime::now().duration_since(modified_time) {
                Ok(age) => {
                    let is_fresh = age < Duration::from_secs(CACHE_TTL_SECS);
                    log(
                        LogLevel::Info,
                        &format!(
                            "Cache age: {:?} seconds (fresh: {})",
                            age.as_secs(),
                            is_fresh
                        ),
                    );
                    is_fresh
                }
                Err(_) => {
                    log(
                        LogLevel::Warn,
                        "Could not determine cache age, treating as stale",
                    );
                    false
                }
            },
            Err(_) => {
                log(
                    LogLevel::Warn,
                    "Could not read cache modification time, treating as stale",
                );
                false
            }
        },
        Err(_) => false,
    }
}

pub fn load_github_licenses_from_cache() -> FeludaResult<Option<HashMap<String, License>>> {
    let cache_path = get_github_cache_path()?;

    if !cache_path.exists() {
        log(LogLevel::Info, "No GitHub licenses cache found");
        return Ok(None);
    }

    if !is_cache_fresh(&cache_path) {
        log(
            LogLevel::Info,
            "GitHub licenses cache is stale, will re-fetch",
        );
        return Ok(None);
    }

    log(LogLevel::Info, "Loading GitHub licenses from cache");

    match fs::read_to_string(&cache_path) {
        Ok(content) => match serde_json::from_str::<CacheEntry>(&content) {
            Ok(entry) => {
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
                log_error("Failed to parse cache file", &e);
                log(LogLevel::Info, "Will re-fetch licenses from GitHub");
                Ok(None)
            }
        },
        Err(e) => {
            log_error("Failed to read cache file", &e);
            log(LogLevel::Info, "Will re-fetch licenses from GitHub");
            Ok(None)
        }
    }
}

pub fn save_github_licenses_to_cache(licenses: &HashMap<String, License>) -> FeludaResult<()> {
    let cache_path = get_github_cache_path()?;

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let entry = CacheEntry {
        data: licenses.clone(),
        timestamp,
    };

    let json = match serde_json::to_string_pretty(&entry) {
        Ok(json) => json,
        Err(e) => {
            log_error("Failed to serialize cache", &e);
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()).into());
        }
    };

    fs::write(&cache_path, json).map_err(|e| {
        log_error("Failed to write cache file", &e);
        e
    })?;

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
    let cache_path = get_github_cache_path()?;

    if cache_path.exists() {
        fs::remove_file(&cache_path).map_err(|e| {
            log_error("Failed to clear cache", &e);
            e
        })?;
        log(LogLevel::Info, "Cleared GitHub licenses cache");
    } else {
        log(LogLevel::Info, "No cache to clear");
    }

    Ok(())
}

#[derive(Debug)]
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

        if secs < HOUR {
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

pub fn get_cache_status() -> FeludaResult<CacheStatus> {
    let cache_path = get_github_cache_path()?;

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

    let metadata = fs::metadata(&cache_path)?;
    let size_bytes = metadata.len();
    let is_fresh = is_cache_fresh(&cache_path);

    let age_secs = SystemTime::now()
        .duration_since(metadata.modified()?)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let license_count = match fs::read_to_string(&cache_path) {
        Ok(content) => match serde_json::from_str::<CacheEntry>(&content) {
            Ok(entry) => entry.data.len(),
            Err(_) => 0,
        },
        Err(_) => 0,
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
