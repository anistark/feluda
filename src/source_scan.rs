//! Own-source license header scanning.
//!
//! Dependency analysis answers "what do my declared dependencies license as?"; this module
//! answers "does my own code carry someone else's license?". AI coding assistants and plain
//! copy-paste routinely introduce source files bearing an `SPDX-License-Identifier:` tag or a
//! GNU license banner without any manifest entry, so the default scan walks the project's own
//! source files and flags every file whose header declares a license different from the
//! project's own.
//!
//! Findings are reported as [`LicenseInfo`] entries alongside dependency results — the file's
//! relative path is the entry name and the version column carries [`OWN_SOURCE_MARKER`] — so
//! every output mode (table, JSON, YAML, CI formats) and every filter (`--restrictive`,
//! `--incompatible`, `--fail-on-restrictive`) applies to them unchanged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::debug::{log, LogLevel};
use crate::licenses::{
    detect_license_from_source_header, fetch_licenses_from_github, get_osi_status,
    is_license_ignored, is_license_restrictive, read_header_region, LicenseCompatibility,
    LicenseInfo, SOURCE_HEADER_EXTENSIONS,
};

/// Marker placed in the version column of an own-source finding, distinguishing it from a
/// dependency entry (files have no version).
pub const OWN_SOURCE_MARKER: &str = "own source";

/// Directory names never treated as own source. These hold third-party code that either the
/// dependency analyzers already cover (`node_modules`, `target`) or that the planned vendored
/// dependency detection will handle at directory granularity — flagging every file inside them
/// individually would drown the report.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "third_party",
    "venv",
    ".venv",
    "__pycache__",
    "site-packages",
    "bower_components",
    "Pods",
    "dist",
    "build",
];

/// Flatten a comment banner into lowercase prose so phrase matching works across line breaks.
///
/// Banners wrap at ~70 columns with a comment prefix on every line (`// `, ` * `, `# `), so a
/// phrase like "GNU General Public License" is routinely split mid-phrase. Stripping the
/// leading comment punctuation from each line and joining on single spaces restores the
/// contiguous prose.
fn normalize_header_prose(header: &str) -> String {
    let joined: String = header
        .lines()
        .map(|line| {
            line.trim_start()
                .trim_start_matches(['/', '*', '#', '-', ';', '!', '<'])
        })
        .collect::<Vec<_>>()
        .join(" ");
    joined
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Detect a GNU license from the conventional grant banner ("This program is free software:
/// you can redistribute it ... under the terms of the GNU General Public License ...").
///
/// This complements [`detect_license_from_source_header`]: older or hand-copied GPL-family
/// files carry the prose banner rather than an SPDX tag. Matching is anchored on the grant
/// phrase "under the terms of the GNU" so a passing prose mention of a license name does not
/// count. Returns a canonical SPDX id with an `-only`/`-or-later` suffix derived from the
/// banner's own wording ("either version N" / "any later version").
fn detect_license_from_header_boilerplate(header: &str) -> Option<String> {
    let lower = normalize_header_prose(header);
    if !lower.contains("under the terms of the gnu") {
        return None;
    }

    let (base, default_version) = if lower.contains("affero general public license") {
        ("AGPL", "3.0")
    } else if lower.contains("lesser general public license")
        || lower.contains("library general public license")
    {
        ("LGPL", "2.1")
    } else if lower.contains("general public license") {
        ("GPL", "2.0")
    } else {
        return None;
    };

    // "version 2.1" must be probed before "version 2" — the latter is a substring of it.
    let version = if lower.contains("version 2.1") {
        "2.1"
    } else if lower.contains("version 3") {
        "3.0"
    } else if lower.contains("version 2") {
        "2.0"
    } else {
        default_version
    };

    let suffix = if lower.contains("any later version") {
        "or-later"
    } else {
        "only"
    };

    Some(format!("{base}-{version}-{suffix}"))
}

/// Whether `path` has a source extension worth scanning for a license header.
fn has_source_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SOURCE_HEADER_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        })
}

/// Walk the project's own source files and return every file whose leading comment region
/// declares a license, as `(relative path, license expression)` pairs.
///
/// The walk honours `.gitignore`, skips hidden entries, and never descends into [`SKIP_DIRS`]
/// (third-party code is the dependency analyzers' job). Files whose header license equals
/// `project_license` are not findings — that is the normal shape of a project that stamps its
/// own headers. Entries are visited in a stable order so results are deterministic.
fn collect_header_findings(root: &Path, project_license: Option<&str>) -> Vec<(PathBuf, String)> {
    let walker = WalkBuilder::new(root)
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|entry| {
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            !(is_dir
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| SKIP_DIRS.contains(&name)))
        })
        .build();

    let mut findings = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        if !has_source_extension(path) {
            continue;
        }
        let Some(header) = read_header_region(path) else {
            continue;
        };
        let Some(found) = detect_license_from_source_header(&header)
            .or_else(|| detect_license_from_header_boilerplate(&header))
        else {
            continue;
        };

        if project_license.is_some_and(|proj| proj.trim().eq_ignore_ascii_case(found.trim())) {
            continue;
        }
        if is_license_ignored(Some(&found)) {
            continue;
        }

        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        log(
            LogLevel::Warn,
            &format!(
                "Own source file {} declares license {found} (project: {})",
                rel.display(),
                project_license.unwrap_or("unknown")
            ),
        );
        findings.push((rel, found));
    }
    findings
}

/// Scan the project's own source files for foreign license headers and return them as
/// [`LicenseInfo`] entries ready to be appended to the dependency report.
///
/// Compatibility is left [`LicenseCompatibility::Unknown`]; the caller's compatibility
/// annotation pass fills it in exactly as it does for dependencies. The license registry is
/// fetched only when at least one finding exists, so clean projects pay nothing.
pub fn scan_own_source_headers(
    root: &Path,
    project_license: Option<&str>,
    strict: bool,
) -> Vec<LicenseInfo> {
    let findings = collect_header_findings(root, project_license);
    if findings.is_empty() {
        return Vec::new();
    }

    let known_licenses = fetch_licenses_from_github().unwrap_or_else(|e| {
        log(
            LogLevel::Warn,
            &format!("Failed to fetch license registry for own-source scan: {e}"),
        );
        HashMap::new()
    });

    findings
        .into_iter()
        .map(|(rel, found)| {
            let osi_status = get_osi_status(&found);
            let license = Some(found);
            let is_restrictive = is_license_restrictive(&license, &known_licenses, strict);
            LicenseInfo {
                name: rel.display().to_string(),
                version: OWN_SOURCE_MARKER.to_string(),
                license,
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
                osi_status,
                sub_project: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const GPL2_BANNER: &str = "\
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
int main(void) { return 0; }
";

    #[test]
    fn test_boilerplate_gpl2_or_later() {
        assert_eq!(
            detect_license_from_header_boilerplate(GPL2_BANNER),
            Some("GPL-2.0-or-later".to_string())
        );
    }

    #[test]
    fn test_boilerplate_gpl3_or_later() {
        let header = "\
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
";
        assert_eq!(
            detect_license_from_header_boilerplate(header),
            Some("GPL-3.0-or-later".to_string())
        );
    }

    #[test]
    fn test_boilerplate_lgpl21() {
        let header = "\
/* This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version. */
";
        assert_eq!(
            detect_license_from_header_boilerplate(header),
            Some("LGPL-2.1-or-later".to_string())
        );
    }

    #[test]
    fn test_boilerplate_agpl3() {
        let header = "\
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
";
        assert_eq!(
            detect_license_from_header_boilerplate(header),
            Some("AGPL-3.0-or-later".to_string())
        );
    }

    #[test]
    fn test_boilerplate_only_variant_without_later_clause() {
        let header = "\
// You may redistribute this file under the terms of the GNU General
// Public License version 2 as published by the Free Software Foundation.
";
        assert_eq!(
            detect_license_from_header_boilerplate(header),
            Some("GPL-2.0-only".to_string())
        );
    }

    #[test]
    fn test_boilerplate_rejects_prose_mention() {
        // Mentions a license by name but carries no grant phrase.
        let header =
            "// Unlike tools covered by the GNU General Public License, this one is MIT.\n";
        assert_eq!(detect_license_from_header_boilerplate(header), None);
    }

    #[test]
    fn test_boilerplate_rejects_plain_code() {
        assert_eq!(
            detect_license_from_header_boilerplate("fn main() { println!(\"hi\"); }\n"),
            None
        );
    }

    #[test]
    fn test_collect_flags_foreign_spdx_header() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(
            dir.path().join("pasted.py"),
            "# SPDX-License-Identifier: GPL-3.0-only\nprint('hi')\n",
        )
        .unwrap();

        let findings = collect_header_findings(dir.path(), Some("MIT"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].0, PathBuf::from("pasted.py"));
        assert_eq!(findings[0].1, "GPL-3.0-only");
    }

    #[test]
    fn test_collect_flags_gnu_banner() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("borrowed.c"), GPL2_BANNER).unwrap();

        let findings = collect_header_findings(dir.path(), Some("Apache-2.0"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].1, "GPL-2.0-or-later");
    }

    #[test]
    fn test_collect_skips_header_matching_project_license() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "// SPDX-License-Identifier: MIT\npub fn f() {}\n",
        )
        .unwrap();

        assert!(collect_header_findings(dir.path(), Some("MIT")).is_empty());
        // Case differences in the header must not defeat the match.
        assert!(collect_header_findings(dir.path(), Some("mit")).is_empty());
    }

    #[test]
    fn test_collect_reports_header_when_project_license_unknown() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(
            dir.path().join("mystery.go"),
            "// SPDX-License-Identifier: MPL-2.0\npackage main\n",
        )
        .unwrap();

        let findings = collect_header_findings(dir.path(), None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].1, "MPL-2.0");
    }

    #[test]
    fn test_collect_skips_dependency_directories() {
        let dir = tempfile::TempDir::new().unwrap();
        let dep_dir = dir.path().join("node_modules").join("leftpad");
        fs::create_dir_all(&dep_dir).unwrap();
        fs::write(
            dep_dir.join("index.js"),
            "// SPDX-License-Identifier: GPL-3.0-only\nmodule.exports = {};\n",
        )
        .unwrap();
        let vendor_dir = dir.path().join("vendor");
        fs::create_dir_all(&vendor_dir).unwrap();
        fs::write(
            vendor_dir.join("lib.c"),
            "/* SPDX-License-Identifier: AGPL-3.0-only */\n",
        )
        .unwrap();

        assert!(collect_header_findings(dir.path(), Some("MIT")).is_empty());
    }

    #[test]
    fn test_collect_ignores_non_source_files() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(
            dir.path().join("NOTES.md"),
            "SPDX-License-Identifier: GPL-3.0-only\n",
        )
        .unwrap();

        assert!(collect_header_findings(dir.path(), Some("MIT")).is_empty());
    }

    #[test]
    fn test_collect_headerless_files_are_clean() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

        assert!(collect_header_findings(dir.path(), Some("MIT")).is_empty());
    }

    #[test]
    fn test_collect_is_deterministic() {
        let dir = tempfile::TempDir::new().unwrap();
        for name in ["b.py", "a.py", "c.py"] {
            fs::write(
                dir.path().join(name),
                "# SPDX-License-Identifier: GPL-3.0-only\n",
            )
            .unwrap();
        }

        let names: Vec<String> = collect_header_findings(dir.path(), Some("MIT"))
            .into_iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        assert_eq!(names, vec!["a.py", "b.py", "c.py"]);
    }
}
