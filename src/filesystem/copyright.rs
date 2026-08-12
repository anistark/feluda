//! Reading a license out of a Debian `copyright` file.
//!
//! dpkg records no license anywhere in its database, so unlike apk and rpm there is no field to
//! read. What Debian Policy does guarantee is `/usr/share/doc/<package>/copyright`, and since
//! DEP-5 that file is often machine readable. This module gets a license out of it three ways, in
//! descending order of confidence:
//!
//! 1. the DEP-5 `License` field of the `Files: *` stanza, which is the package's own statement;
//! 2. the first `License` short name anywhere in a DEP-5 file, for packages that never state a
//!    catch-all stanza;
//! 3. text matching, the same path the vendor scan takes on a plain `LICENSE` file.
//!
//! Anything else leaves the license unset. An OS package with an unreadable copyright file is
//! reported as unknown, never guessed: a wrong license in a compliance report is worse than an
//! absent one.

use crate::licenses::detect_license_from_content;

use super::deb822::{parse_stanzas, Stanza};

/// Extract a license from the contents of a `copyright` file.
pub fn license_from_copyright(content: &str) -> Option<String> {
    if let Some(license) = dep5_license(content) {
        return Some(license);
    }
    detect_license_from_content(content)
}

/// The license a DEP-5 document states, or `None` when the file is not DEP-5.
fn dep5_license(content: &str) -> Option<String> {
    let stanzas = parse_stanzas(content);
    if !is_dep5(&stanzas) {
        return None;
    }

    // `Files: *` is the package as a whole. Anything narrower covers part of it, and on a package
    // whose debian/ directory is licensed differently from its upstream source (common) the
    // catch-all is the one that describes what is installed.
    let catch_all = stanzas
        .iter()
        .find(|stanza| stanza.first_line("Files") == Some("*"));

    let short_name = catch_all
        .and_then(|stanza| stanza.first_line("License"))
        .or_else(|| {
            stanzas
                .iter()
                .find_map(|stanza| stanza.first_line("License"))
        })?;

    normalize_debian_license(short_name)
}

/// Whether the stanzas came from a machine readable copyright file.
///
/// The `Format` field is the header DEP-5 defines for exactly this purpose. Requiring it keeps a
/// free text file that happens to contain the word `License:` from being read as structured.
fn is_dep5(stanzas: &[Stanza]) -> bool {
    stanzas
        .first()
        .and_then(|stanza| stanza.get("Format"))
        .is_some_and(|format| format.contains("copyright-format") || format.contains("dep5"))
}

/// Translate a Debian license short name into an SPDX identifier.
///
/// Debian's short names predate SPDX and do not match it: `GPL-2+`, `Expat`, `BSD-3-clause`. Left
/// untranslated, the most consequential of them fail to classify at all, and a GPL package would
/// report as unknown rather than restrictive, which is the exact case `--fail-on-restrictive`
/// exists to catch.
///
/// Returns `None` for the names that state the absence of a license rather than a license.
fn normalize_debian_license(short_name: &str) -> Option<String> {
    let name = short_name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("unknown") {
        return None;
    }

    // DEP-5 spells alternatives and combinations in prose. SPDX spells them in operators, which is
    // what `is_license_restrictive` already understands: `OR` is permissive if any branch is,
    // `AND` is restrictive if any part is.
    for (separator, operator) in [(" or ", " OR "), (" and ", " AND ")] {
        if let Some(parts) = split_expression(name, separator) {
            let mapped: Vec<String> = parts
                .iter()
                .filter_map(|part| normalize_debian_license(part))
                .collect();
            if mapped.is_empty() {
                return None;
            }
            return Some(mapped.join(operator));
        }
    }

    if let Some(spdx) = gnu_family_license(name) {
        return Some(spdx);
    }

    let spdx = match name.to_ascii_lowercase().as_str() {
        "expat" => "MIT",
        "bsd-2-clause" => "BSD-2-Clause",
        "bsd-3-clause" => "BSD-3-Clause",
        "bsd-4-clause" => "BSD-4-Clause",
        "artistic" => "Artistic-1.0",
        "artistic-2.0" => "Artistic-2.0",
        "apache-2.0" => "Apache-2.0",
        "mpl-1.1" => "MPL-1.1",
        "mpl-2.0" => "MPL-2.0",
        "cc0-1.0" | "cc0" => "CC0-1.0",
        "cc-by-sa-4.0" => "CC-BY-SA-4.0",
        "zlib" | "zlib/libpng" => "Zlib",
        "isc" => "ISC",
        // Debian writes the Expat license three ways, and all three are SPDX's MIT.
        "mit" | "mit/x11" | "x11" => "MIT",
        "python" | "psf-2" => "Python-2.0",
        "openssl" => "OpenSSL",
        "boost" | "bsl-1.0" => "BSL-1.0",
        "epl-1.0" => "EPL-1.0",
        "epl-2.0" => "EPL-2.0",
        "unlicense" => "Unlicense",
        "wtfpl" | "wtfpl-2" => "WTFPL",
        // Not an SPDX identifier and not a mistake: Debian says a fair number of files are in the
        // public domain, and passing it through unchanged reports it honestly.
        _ => return Some(name.to_string()),
    };
    Some(spdx.to_string())
}

/// Split a license expression on a separator, honouring the parentheses DEP-5 permits.
///
/// Returns `None` when the separator does not appear at the top level, so `GPL-2+` and
/// `(GPL-2+ or Artistic)` are told apart from a name that merely contains the word.
fn split_expression(name: &str, separator: &str) -> Option<Vec<String>> {
    // ASCII lowering preserves byte lengths, so an index into one string indexes the other.
    let lowered = name.to_ascii_lowercase();
    let mut parts: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut positions = lowered.char_indices().peekable();

    while let Some((index, character)) = positions.next() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth > 0 || !lowered[index..].starts_with(separator) {
            continue;
        }

        parts.push(trim_part(&name[start..index]));
        start = index + separator.len();
        while positions.peek().is_some_and(|(next, _)| *next < start) {
            positions.next();
        }
    }

    if parts.is_empty() {
        return None;
    }
    parts.push(trim_part(&name[start..]));
    let parts: Vec<String> = parts.into_iter().filter(|part| !part.is_empty()).collect();
    (!parts.is_empty()).then_some(parts)
}

/// Strip the whitespace and grouping parentheses around one branch of an expression.
fn trim_part(part: &str) -> String {
    part.trim().trim_matches(['(', ')']).trim().to_string()
}

/// Map the GNU license short names, where Debian's spelling differs from SPDX's most often and
/// matters most.
///
/// Debian writes `GPL-2` for the exact version and `GPL-2+` for "or any later version"; SPDX
/// writes `GPL-2.0-only` and `GPL-2.0-or-later`. The bare major versions also need a `.0` that
/// Debian omits.
fn gnu_family_license(name: &str) -> Option<String> {
    let lowered = name.to_ascii_lowercase();
    let (family, spdx_family) = ["agpl", "lgpl", "gfdl", "gpl"]
        .iter()
        .find(|family| lowered.starts_with(*family))
        .map(|family| {
            (
                *family,
                match *family {
                    "agpl" => "AGPL",
                    "lgpl" => "LGPL",
                    "gfdl" => "GFDL",
                    _ => "GPL",
                },
            )
        })?;

    let remainder = lowered[family.len()..].trim_start_matches('-');
    let (version, or_later) = match remainder.strip_suffix('+') {
        Some(version) => (version, true),
        None => (remainder, false),
    };
    // `GPL` with no version at all says which family but not which license, and guessing a version
    // would put words in the package's mouth.
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }

    let version = if version.contains('.') {
        version.to_string()
    } else {
        format!("{version}.0")
    };
    let suffix = if or_later { "or-later" } else { "only" };
    Some(format!("{spdx_family}-{version}-{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEP5: &str = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: hello
Source: https://www.gnu.org/software/hello/

Files: *
Copyright: 1992-2022 Free Software Foundation, Inc.
License: GPL-3+

Files: debian/*
Copyright: 2005-2022 Santiago Vila
License: GPL-3+

License: GPL-3+
 This program is free software: you can redistribute it and/or modify
 it under the terms of the GNU General Public License.
"#;

    #[test]
    fn test_reads_the_catch_all_stanza() {
        assert_eq!(
            license_from_copyright(DEP5),
            Some("GPL-3.0-or-later".to_string())
        );
    }

    #[test]
    fn test_catch_all_wins_over_a_narrower_stanza() {
        // The upstream source is BSD; only the packaging is GPL. What is installed is the former.
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: debian/*
Copyright: 2020 A Maintainer
License: GPL-2+

Files: *
Copyright: 2015 Upstream
License: BSD-3-clause
"#;
        assert_eq!(
            license_from_copyright(content),
            Some("BSD-3-Clause".to_string())
        );
    }

    #[test]
    fn test_falls_back_to_the_first_license_stanza() {
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2015 Upstream
License: Expat
"#;
        assert_eq!(license_from_copyright(content), Some("MIT".to_string()));
    }

    #[test]
    fn test_free_text_copyright_falls_back_to_content_detection() {
        // No Format header, so nothing structured to read. Debian ships plenty of these.
        let content = "This package was debianized by someone.\n\n\
             Permission is hereby granted, free of charge, to any person obtaining a copy \
             of this software and associated documentation files (the \"Software\"), to deal \
             in the Software without restriction, including without limitation the rights \
             to use, copy, modify, merge, publish, distribute, sublicense, and/or sell \
             copies of the Software";
        assert_eq!(license_from_copyright(content), Some("MIT".to_string()));
    }

    #[test]
    fn test_unreadable_copyright_yields_nothing() {
        assert_eq!(
            license_from_copyright("Copyright 2020 Someone. All rights reserved."),
            None
        );
    }

    #[test]
    fn test_a_license_field_without_the_format_header_is_not_dep5() {
        // A free text file mentioning `License:` is not a machine readable one, and reading it as
        // structured would take a line out of prose as the whole package's license.
        let content = "Upstream authors said: License: whatever you like\n";
        assert_eq!(license_from_copyright(content), None);
    }

    #[test]
    fn test_gnu_short_names_become_spdx() {
        let cases = [
            ("GPL-2", "GPL-2.0-only"),
            ("GPL-2+", "GPL-2.0-or-later"),
            ("GPL-3+", "GPL-3.0-or-later"),
            ("LGPL-2.1+", "LGPL-2.1-or-later"),
            ("LGPL-3", "LGPL-3.0-only"),
            ("AGPL-3+", "AGPL-3.0-or-later"),
            ("GFDL-1.3+", "GFDL-1.3-or-later"),
        ];
        for (debian, spdx) in cases {
            assert_eq!(
                normalize_debian_license(debian),
                Some(spdx.to_string()),
                "{debian} should map to {spdx}"
            );
        }
    }

    #[test]
    fn test_gpl_without_a_version_is_not_guessed() {
        assert_eq!(normalize_debian_license("GPL"), Some("GPL".to_string()));
    }

    #[test]
    fn test_alternatives_become_an_spdx_expression() {
        assert_eq!(
            normalize_debian_license("GPL-2+ or Artistic"),
            Some("GPL-2.0-or-later OR Artistic-1.0".to_string())
        );
        assert_eq!(
            normalize_debian_license("Apache-2.0 and Expat"),
            Some("Apache-2.0 AND MIT".to_string())
        );
    }

    #[test]
    fn test_debian_spellings_of_one_license_agree() {
        // All three turn up in a stock debian:12-slim image.
        for name in ["Expat", "MIT/X11", "X11"] {
            assert_eq!(normalize_debian_license(name), Some("MIT".to_string()));
        }
    }

    #[test]
    fn test_unmapped_names_pass_through() {
        // Not SPDX, but it is what the package said, and saying so beats reporting unknown.
        assert_eq!(
            normalize_debian_license("public-domain"),
            Some("public-domain".to_string())
        );
    }

    #[test]
    fn test_nothing_stated_stays_nothing() {
        assert_eq!(normalize_debian_license(""), None);
        assert_eq!(normalize_debian_license("unknown"), None);
    }
}
