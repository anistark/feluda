//! Translating an rpm `License` tag into an SPDX expression.
//!
//! Fedora migrated its packages to SPDX identifiers, but package by package and release by release,
//! so both spellings are live. A `fedora:41` image states `GPL-2.0-or-later AND BSD-2-Clause`; a
//! `rockylinux:9` image states `(GPLv2+ or LGPLv3+) and GPLv3+` for the same kind of thing.
//!
//! Shaped like the Debian mapping in [`super::super::copyright`]: split on the top level `and` and
//! `or` while honouring parentheses, map each atom, rejoin with SPDX's uppercase operators. Because
//! the split is on space delimited words, an expression that is already SPDX passes through
//! unharmed, since `GPL-2.0-or-later` contains no space delimited `or`.

/// Normalize a `License` tag, or `None` when it states nothing.
pub fn normalize(license: &str) -> Option<String> {
    let license = license.trim();
    if license.is_empty() || license.eq_ignore_ascii_case("unknown") {
        return None;
    }

    // `or` binds tighter than `and` in Fedora's spelling, so splitting on `and` first leaves the
    // alternatives grouped together on one side.
    for (separator, operator) in [(" and ", " AND "), (" or ", " OR ")] {
        if let Some(parts) = split(license, separator) {
            let mapped: Vec<String> = parts.iter().filter_map(|part| branch(part)).collect();
            if mapped.is_empty() {
                return None;
            }
            return Some(mapped.join(operator));
        }
    }

    Some(atom(license))
}

/// Map one branch of an expression, keeping it grouped if it needs to be.
///
/// Parentheses come off before mapping so the contents can be read, and go back on in two cases:
/// the branch arrived parenthesized, or mapping turned it into an expression of its own. The first
/// keeps an already correct SPDX string byte for byte identical, since `A AND (B WITH C)` is how
/// Fedora writes it. The second is what stops `(GPLv2+ or LGPLv3+) and GPLv3+` from flattening into
/// `A OR B AND C`, which offers different licenses than the package does.
fn branch(part: &str) -> Option<String> {
    let trimmed = part.trim();
    let inner = ungroup(trimmed);
    let mapped = normalize(inner)?;

    let compound = split(&mapped, " and ").is_some() || split(&mapped, " or ").is_some();
    if compound || inner != trimmed {
        return Some(format!("({mapped})"));
    }
    Some(mapped)
}

/// Map one license name, leaving anything unrecognized as the package stated it.
fn atom(name: &str) -> String {
    if let Some(spdx) = gnu_family(name) {
        return spdx;
    }

    let spdx = match name.to_ascii_lowercase().as_str() {
        "asl 1.0" => "Apache-1.0",
        "asl 1.1" => "Apache-1.1",
        "asl 2.0" => "Apache-2.0",
        "mit" | "mit/x11" => "MIT",
        "bsd with advertising" => "BSD-4-Clause",
        "modified bsd" => "BSD-3-Clause",
        "mplv1.0" => "MPL-1.0",
        "mplv1.1" => "MPL-1.1",
        "mplv2.0" => "MPL-2.0",
        "artistic 2.0" => "Artistic-2.0",
        "artistic clarified" => "Artistic-2.0",
        "boost" => "BSL-1.0",
        "epl-1.0" => "EPL-1.0",
        "epl-2.0" => "EPL-2.0",
        "cddl" => "CDDL-1.0",
        "cc0" => "CC0-1.0",
        "python" => "Python-2.0",
        "psf" | "psf-2.0" => "PSF-2.0",
        "openssl" => "OpenSSL",
        "sleepycat" => "Sleepycat",
        "zlib" | "zlib with acknowledgement" => "Zlib",
        "isc" => "ISC",
        "ofl" => "OFL-1.1",
        "ijg" => "IJG",
        "vim" => "Vim",
        "unlicense" => "Unlicense",
        // Deliberately unmapped, following the refusal `gnu_family` makes for a bare `GPL`:
        //
        // - `BSD`: Fedora used it for both the two and three clause licenses, and a compliance
        //   report should not pick one.
        // - `Public Domain`: SPDX has no identifier for it.
        //
        // Both come back as the string the package stated, which reports honestly.
        _ => return name.to_string(),
    };
    spdx.to_string()
}

/// Map Fedora's GNU short names, where the spelling differs from SPDX and matters most: an
/// unmapped `GPLv2+` reports as unknown rather than restrictive, and the gate `--fail-on-restrictive`
/// exists for stays green.
///
/// Fedora writes `GPLv2` for the exact version and `GPLv2+` for "or any later version", against
/// SPDX's `GPL-2.0-only` and `GPL-2.0-or-later`. `GPL+` is the one without a version, which Fedora
/// documented as "any version", so it maps to the earliest.
fn gnu_family(name: &str) -> Option<String> {
    let lowered = name.to_ascii_lowercase();
    let (family, spdx) = ["agpl", "lgpl", "gfdl", "gpl"]
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

    let remainder = lowered[family.len()..].trim_start_matches('v');
    let (version, or_later) = match remainder.strip_suffix('+') {
        Some(version) => (version, true),
        None => (remainder, false),
    };

    // `GPL+` is Fedora's "any version", which is SPDX's `GPL-1.0-or-later`.
    if version.is_empty() {
        return or_later.then(|| format!("{spdx}-1.0-or-later"));
    }
    if !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }

    let version = if version.contains('.') {
        version.to_string()
    } else {
        format!("{version}.0")
    };
    let suffix = if or_later { "or-later" } else { "only" };
    Some(format!("{spdx}-{version}-{suffix}"))
}

/// Split an expression on a separator at the top level, honouring parentheses.
///
/// Returns `None` when the separator does not appear outside a group, which is what tells
/// `(GPLv2+ or LGPLv3+) and GPLv3+` apart at each level.
fn split(name: &str, separator: &str) -> Option<Vec<String>> {
    // ASCII lowering preserves byte lengths, so an index into one string indexes the other. This is
    // what lets an already uppercase SPDX ` AND ` match the lowercase separator.
    let lowered = name.to_ascii_lowercase();
    let mut parts: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut index = 0usize;

    while index < lowered.len() {
        match lowered.as_bytes()[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && lowered[index..].starts_with(separator) {
            parts.push(name[start..index].to_string());
            index += separator.len();
            start = index;
            continue;
        }
        index += 1;
    }

    if parts.is_empty() {
        return None;
    }
    parts.push(name[start..].to_string());

    let parts: Vec<String> = parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then_some(parts)
}

/// Strip one layer of grouping parentheses from a branch, if it is wrapped in one.
///
/// Only a pair that wraps the whole branch comes off, and only when what is inside is balanced, so
/// the outer parentheses of `(A or B) and (C)` are never mistaken for a wrapper around the lot.
fn ungroup(part: &str) -> &str {
    match part
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        Some(inner) if balanced(inner) => inner.trim(),
        _ => part,
    }
}

fn balanced(text: &str) -> bool {
    let mut depth = 0i32;
    for byte in text.bytes() {
        match byte {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spdx_expressions_pass_through() {
        // What a modern Fedora states. Nothing here should be rewritten.
        for license in [
            "MIT",
            "GPL-3.0-or-later",
            "LicenseRef-Fedora-Public-Domain",
            "BSD-4-Clause",
            "MPL-2.0",
        ] {
            assert_eq!(normalize(license), Some(license.to_string()));
        }
    }

    #[test]
    fn test_an_spdx_and_expression_survives() {
        assert_eq!(
            normalize("GPL-2.0-or-later AND BSD-2-Clause"),
            Some("GPL-2.0-or-later AND BSD-2-Clause".to_string())
        );
    }

    #[test]
    fn test_spdx_with_exception_is_one_atom() {
        // `WITH` is not a separator here, so the exception stays attached to its license.
        assert_eq!(
            normalize("GPL-2.0-or-later WITH GCC-exception-2.0"),
            Some("GPL-2.0-or-later WITH GCC-exception-2.0".to_string())
        );
    }

    #[test]
    fn test_a_parenthesized_spdx_expression_is_returned_unchanged() {
        // Straight off a fedora:41 image. An expression that is already correct SPDX must come back
        // byte for byte, not merely equivalent: rewriting valid input is how a report loses trust.
        for license in [
            "MIT AND CC-PDDC AND (GPL-3.0-or-later WITH Texinfo-exception)",
            "LicenseRef-Fedora-Public-Domain AND (GPL-2.0-only WITH ClassPath-exception-2.0)",
            "GPL-3.0-or-later AND LGPL-3.0-or-later AND (GPL-3.0-or-later WITH GCC-exception-3.1)",
        ] {
            assert_eq!(normalize(license), Some(license.to_string()));
        }
    }

    #[test]
    fn test_legacy_gnu_names_become_spdx() {
        let cases = [
            ("GPLv2", "GPL-2.0-only"),
            ("GPLv2+", "GPL-2.0-or-later"),
            ("GPLv3", "GPL-3.0-only"),
            ("GPLv3+", "GPL-3.0-or-later"),
            ("LGPLv2", "LGPL-2.0-only"),
            ("LGPLv2+", "LGPL-2.0-or-later"),
            ("LGPLv2.1", "LGPL-2.1-only"),
            ("LGPLv2.1+", "LGPL-2.1-or-later"),
            ("LGPLv3+", "LGPL-3.0-or-later"),
            ("AGPLv3+", "AGPL-3.0-or-later"),
            ("GFDL", "GFDL"),
            ("GPL+", "GPL-1.0-or-later"),
        ];
        for (legacy, spdx) in cases {
            assert_eq!(
                normalize(legacy),
                Some(spdx.to_string()),
                "{legacy} should map to {spdx}"
            );
        }
    }

    #[test]
    fn test_legacy_multi_word_names() {
        assert_eq!(normalize("ASL 2.0"), Some("Apache-2.0".to_string()));
        assert_eq!(
            normalize("BSD with advertising"),
            Some("BSD-4-Clause".to_string())
        );
    }

    #[test]
    fn test_lowercase_operators_normalize_to_spdx() {
        assert_eq!(
            normalize("BSD and GPLv2+"),
            Some("BSD AND GPL-2.0-or-later".to_string())
        );
        assert_eq!(
            normalize("BSD or GPLv2"),
            Some("BSD OR GPL-2.0-only".to_string())
        );
    }

    #[test]
    fn test_grouping_is_preserved() {
        // Straight off a rockylinux:9 image. Flattening the parentheses would change which licenses
        // the expression actually offers.
        assert_eq!(
            normalize("(GPLv2+ or LGPLv3+) and GPLv3+"),
            Some("(GPL-2.0-or-later OR LGPL-3.0-or-later) AND GPL-3.0-or-later".to_string())
        );
    }

    #[test]
    fn test_a_real_util_linux_expression() {
        assert_eq!(
            normalize("GPLv2 and GPLv2+ and LGPLv2+ and BSD with advertising and Public Domain"),
            Some(
                "GPL-2.0-only AND GPL-2.0-or-later AND LGPL-2.0-or-later AND BSD-4-Clause AND \
                 Public Domain"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_bare_bsd_is_not_guessed() {
        // Fedora used it for both the two and three clause licenses.
        assert_eq!(normalize("BSD"), Some("BSD".to_string()));
    }

    #[test]
    fn test_public_domain_passes_through() {
        // Not SPDX and not a mistake: it is what the package said.
        assert_eq!(
            normalize("Public Domain"),
            Some("Public Domain".to_string())
        );
    }

    #[test]
    fn test_bare_gnu_family_without_a_version_is_not_guessed() {
        assert_eq!(normalize("GPL"), Some("GPL".to_string()));
        assert_eq!(normalize("LGPL"), Some("LGPL".to_string()));
    }

    #[test]
    fn test_nothing_stated_stays_nothing() {
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("   "), None);
        assert_eq!(normalize("unknown"), None);
    }

    #[test]
    fn test_or_binds_tighter_than_and() {
        // `A or B and C` is Fedora's way of writing `(A or B) and C`, and the parentheses the
        // mapping adds are what make that survive a round trip through a consumer.
        assert_eq!(
            normalize("GPLv2+ or LGPLv3+ and GPLv3+"),
            Some("(GPL-2.0-or-later OR LGPL-3.0-or-later) AND GPL-3.0-or-later".to_string())
        );
    }
}
