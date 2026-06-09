use rayon::prelude::*;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

use crate::config::FeludaConfig;
use crate::debug::{log, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

#[derive(Debug, Clone)]
struct RubyDependency {
    name: String,
    version: String,
}

pub fn analyze_ruby_licenses(file_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
    log(
        LogLevel::Info,
        &format!("Analyzing Ruby dependencies from: {file_path}"),
    );

    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("Failed to read Ruby file: {file_path}"), &e);
            return Vec::new();
        }
    };

    // `Gemfile.lock` is the resolved lockfile: it already contains the full
    // transitive dependency set with exact versions, so no registry walk is
    // needed (unlike Java). A bare `Gemfile` only lists direct, often
    // constraint-versioned deps, so it is a best-effort fallback.
    let deps = if file_path.ends_with("Gemfile.lock") {
        parse_gemfile_lock(&content)
    } else {
        parse_gemfile(&content)
    };

    if deps.is_empty() {
        log(LogLevel::Warn, "No Ruby dependencies found");
        return Vec::new();
    }

    log(
        LogLevel::Info,
        &format!("Found {} Ruby dependencies", deps.len()),
    );

    let known_licenses = match fetch_licenses_from_github() {
        Ok(licenses) => licenses,
        Err(err) => {
            log_error("Failed to fetch licenses from GitHub", &err);
            HashMap::new()
        }
    };

    deps.par_iter()
        .map(|dep| {
            let license = fetch_ruby_license(&dep.name, &dep.version);
            let is_restrictive =
                is_license_restrictive(&Some(license.clone()), &known_licenses, config.strict);

            LicenseInfo {
                name: dep.name.clone(),
                version: dep.version.clone(),
                license: Some(license.clone()),
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::get_osi_status(&license),
                sub_project: None,
            }
        })
        .collect()
}

// =============================================================================
// GEMFILE.LOCK PARSING
// =============================================================================

/// Parse the resolved gems from a `Gemfile.lock`.
///
/// Every `specs:` block (under `GEM`, and any `GIT`/`PATH` sources) lists its
/// resolved gems at 4-space indentation as `name (version)`. Lines indented
/// deeper are that gem's own constraints and are skipped, since each such gem
/// also appears as its own top-level spec.
fn parse_gemfile_lock(content: &str) -> Vec<RubyDependency> {
    let spec_re = Regex::new(r"^    ([A-Za-z0-9._-]+) \(([^)]+)\)$").unwrap();
    let mut deps: Vec<RubyDependency> = Vec::new();
    let mut in_specs = false;

    for line in content.lines() {
        if line.trim() == "specs:" {
            in_specs = true;
            continue;
        }

        if !in_specs {
            continue;
        }

        if line.trim().is_empty() {
            in_specs = false;
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        if indent < 4 {
            // Dedented out of the specs block (e.g. a new top-level section).
            in_specs = false;
            continue;
        }
        if indent > 4 {
            // A gem's own dependency constraint, not a resolved spec.
            continue;
        }

        if let Some(cap) = spec_re.captures(line) {
            deps.push(RubyDependency {
                name: cap[1].to_string(),
                version: strip_platform(&cap[2]),
            });
        }
    }

    deps.sort_by(|a, b| a.name.cmp(&b.name));
    deps.dedup_by(|a, b| a.name == b.name);
    deps
}

/// Drop a platform suffix from a locked gem version
/// (e.g. `1.13.10-x86_64-linux` -> `1.13.10`). Ruby gem versions are
/// dot-separated and never contain `-`, so the first `-` begins the platform.
fn strip_platform(version: &str) -> String {
    version
        .split_once('-')
        .map(|(v, _)| v)
        .unwrap_or(version)
        .to_string()
}

// =============================================================================
// GEMFILE PARSING
// =============================================================================

/// Best-effort parse of direct dependencies declared in a `Gemfile`.
/// Versions are optional and frequently constraints; an unresolvable version
/// is left empty so the license lookup falls back to the latest release.
fn parse_gemfile(content: &str) -> Vec<RubyDependency> {
    let gem_re =
        Regex::new(r#"(?m)^\s*gem\s+['"]([^'"]+)['"]\s*(?:,\s*['"]([^'"]+)['"])?"#).unwrap();

    let mut deps: Vec<RubyDependency> = Vec::new();
    for cap in gem_re.captures_iter(content) {
        let name = cap[1].to_string();
        let version = cap
            .get(2)
            .map(|m| clean_gem_version(m.as_str()))
            .unwrap_or_default();
        deps.push(RubyDependency { name, version });
    }

    deps.sort_by(|a, b| a.name.cmp(&b.name));
    deps.dedup_by(|a, b| a.name == b.name);
    deps
}

/// Extract a concrete version from a Gemfile constraint, dropping operators
/// like `~>`, `>=`, `=`. Returns an empty string when no version token is found.
fn clean_gem_version(constraint: &str) -> String {
    let ver_re = Regex::new(r"[0-9][0-9A-Za-z.]*").unwrap();
    ver_re
        .find(constraint)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

// =============================================================================
// RUBYGEMS LICENSE LOOKUP
// =============================================================================

fn fetch_ruby_license(name: &str, version: &str) -> String {
    if !version.is_empty() {
        if let Some(license) = fetch_license_for_version(name, version) {
            return license;
        }
    }

    fetch_license_latest(name).unwrap_or_else(|| "Unknown".to_string())
}

fn fetch_license_for_version(name: &str, version: &str) -> Option<String> {
    let url = format!("https://rubygems.org/api/v2/rubygems/{name}/versions/{version}.json");
    log(
        LogLevel::Info,
        &format!("Fetching RubyGems metadata: {url}"),
    );
    fetch_licenses_field(&url)
}

fn fetch_license_latest(name: &str) -> Option<String> {
    let url = format!("https://rubygems.org/api/v1/gems/{name}.json");
    log(
        LogLevel::Info,
        &format!("Fetching latest RubyGems metadata: {url}"),
    );
    fetch_licenses_field(&url)
}

/// Fetch a RubyGems JSON document and join its `licenses` array into a single
/// SPDX string. Multiple licenses become an `A OR B` expression, which the
/// compound-expression handling in `is_license_restrictive` understands.
fn fetch_licenses_field(url: &str) -> Option<String> {
    let response = reqwest::blocking::get(url).ok()?;
    if !response.status().is_success() {
        return None;
    }

    let json: Value = response.json().ok()?;
    let licenses = json["licenses"].as_array()?;

    let names: Vec<String> = licenses
        .iter()
        .filter_map(|l| l.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if names.is_empty() {
        None
    } else {
        Some(names.join(" OR "))
    }
}

// TESTS
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gemfile_lock_basic() {
        let content = r#"GEM
  remote: https://rubygems.org/
  specs:
    actioncable (7.0.4)
      actionpack (= 7.0.4)
      nio4r (~> 2.0)
    nokogiri (1.13.10)
      racc (~> 1.4)
    racc (1.6.2)

PLATFORMS
  ruby

DEPENDENCIES
  rails (~> 7.0)

BUNDLED WITH
   2.3.7
"#;
        let deps = parse_gemfile_lock(content);
        let names: Vec<&str> = deps.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["actioncable", "nokogiri", "racc"]);
        let actioncable = deps.iter().find(|d| d.name == "actioncable").unwrap();
        assert_eq!(actioncable.version, "7.0.4");
    }

    #[test]
    fn test_parse_gemfile_lock_strips_platform() {
        let content = r#"GEM
  specs:
    nokogiri (1.13.10-x86_64-linux)
    sqlite3 (1.5.4-arm64-darwin)
"#;
        let deps = parse_gemfile_lock(content);
        let nokogiri = deps.iter().find(|d| d.name == "nokogiri").unwrap();
        assert_eq!(nokogiri.version, "1.13.10");
        let sqlite3 = deps.iter().find(|d| d.name == "sqlite3").unwrap();
        assert_eq!(sqlite3.version, "1.5.4");
    }

    #[test]
    fn test_parse_gemfile_lock_dedups() {
        let content = r#"GIT
  remote: https://github.com/example/foo.git
  specs:
    foo (1.0.0)

GEM
  specs:
    foo (2.0.0)
    bar (3.1.0)
"#;
        let deps = parse_gemfile_lock(content);
        assert_eq!(deps.len(), 2);
        let names: Vec<&str> = deps.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["bar", "foo"]);
    }

    #[test]
    fn test_strip_platform() {
        assert_eq!(strip_platform("1.13.10-x86_64-linux"), "1.13.10");
        assert_eq!(strip_platform("1.0.0"), "1.0.0");
        assert_eq!(strip_platform("2.0.0.beta1"), "2.0.0.beta1");
    }

    #[test]
    fn test_parse_gemfile() {
        let content = r#"source "https://rubygems.org"

gem "rails", "~> 7.0.4"
gem 'pg', '>= 0.18', '< 2.0'
gem "puma"
gem "redis", require: false
"#;
        let deps = parse_gemfile(content);
        let names: Vec<&str> = deps.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["pg", "puma", "rails", "redis"]);

        let rails = deps.iter().find(|d| d.name == "rails").unwrap();
        assert_eq!(rails.version, "7.0.4");
        let puma = deps.iter().find(|d| d.name == "puma").unwrap();
        assert_eq!(puma.version, "");
    }

    #[test]
    fn test_clean_gem_version() {
        assert_eq!(clean_gem_version("~> 7.0.4"), "7.0.4");
        assert_eq!(clean_gem_version(">= 0.18"), "0.18");
        assert_eq!(clean_gem_version("= 1.2.3"), "1.2.3");
        assert_eq!(clean_gem_version(">= 0"), "0");
        assert_eq!(clean_gem_version(""), "");
    }

    #[test]
    fn test_parse_gemfile_lock_empty() {
        assert!(parse_gemfile_lock("").is_empty());
        assert!(parse_gemfile("").is_empty());
    }
}
