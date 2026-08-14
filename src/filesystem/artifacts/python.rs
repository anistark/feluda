//! Cataloging installed Python distributions.
//!
//! An installed distribution leaves a metadata directory next to its code: `*.dist-info/METADATA`
//! for anything installed as a wheel, `*.egg-info/PKG-INFO` for the older setuptools layout that
//! Debian's own Python packages still use. Both files are RFC822 headers, so one parser serves
//! both.
//!
//! Neither is looked for under a directory named `site-packages` specifically. Debian installs into
//! `dist-packages`, virtualenvs into `site-packages`, and an application image frequently has
//! neither name anywhere in the path. What identifies a distribution is the metadata directory, not
//! where it sits.

use std::path::Path;

use crate::licenses::{detect_license_from_content, detect_license_in_dir};
use crate::purl::Ecosystem;
use crate::spdx;

use super::super::deb822::{parse_stanzas, Stanza};
use super::Artifact;

/// The metadata file a wheel installation leaves, and the directory suffix it sits in.
const DIST_INFO: (&str, &str) = ("METADATA", ".dist-info");

/// The same, for the setuptools layout.
const EGG_INFO: (&str, &str) = ("PKG-INFO", ".egg-info");

/// Where a wheel puts the license files it ships, relative to its metadata directory.
///
/// Older wheels drop them beside `METADATA`; PEP 639 gave them a subdirectory. A single
/// `python:3.12-slim` image with four packages installed has both layouts in it.
const LICENSE_DIRECTORIES: &[&str] = &[".", "licenses"];

/// The classifier prefix that names a license.
const LICENSE_CLASSIFIER: &str = "License :: ";

/// Longest a `License` field may be before it is read as license text rather than as a name.
///
/// Distributions built before PEP 639 routinely set `license=open("LICENSE").read()`, which lands
/// the entire license in the field. The longest real SPDX identifier in use is well under this.
const MAX_LICENSE_NAME: usize = 64;

/// Whether `path` is a Python distribution's metadata file.
pub fn is_metadata(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(directory) = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
    else {
        return false;
    };

    [DIST_INFO, EGG_INFO]
        .iter()
        .any(|(file, suffix)| name == *file && directory.ends_with(suffix))
}

/// Read the distribution described by a metadata file.
///
/// Returns `None` when the file names no distribution, which is the only field there is no
/// reasonable substitute for: a finding with no name has no PURL and cannot be reported.
pub fn read(path: &Path) -> Option<Artifact> {
    let content = std::fs::read_to_string(path).ok()?;
    // The headers are the first stanza; the long description below the blank line is not.
    let metadata = parse_stanzas(&content).into_iter().next()?;

    let name = metadata.first_line("Name")?.to_string();
    let version = metadata
        .first_line("Version")
        .unwrap_or_default()
        .to_string();

    Some(Artifact {
        ecosystem: Ecosystem::Pypi,
        name,
        version,
        license: license(&metadata, path.parent()),
    })
}

/// The license the distribution states, in descending order of confidence.
///
/// `metadata_directory` is the `dist-info` or `egg-info` directory the file was read from, which is
/// where a wheel ships its own license text.
fn license(metadata: &Stanza, metadata_directory: Option<&Path>) -> Option<String> {
    // PEP 639: an SPDX expression by definition, so nothing has to be inferred from it.
    if let Some(expression) = metadata.first_line("License-Expression") {
        return Some(expression.to_string());
    }

    let stated = metadata.get("License").map(str::trim).filter(|license| {
        !license.is_empty() && !license.eq_ignore_ascii_case("unknown") && *license != "UNKNOWN"
    });
    let name = stated.filter(|license| is_license_name(license));

    // A dual license is something no classifier can express: `MIT OR Apache-2.0` becomes two
    // classifiers with nothing left to say which applies. Where the field states an expression, it
    // is the more complete statement.
    if let Some(expression) = name.filter(|license| spdx::is_compound(license)) {
        return Some(expression.to_string());
    }

    // Classifiers come from a fixed vocabulary that maps onto SPDX; the `License` field is free
    // text, and distributions fill it with everything from `Apache-2.0` to `Apache 2.0` to the
    // license itself.
    if let Some(spdx) = classifier_license(metadata) {
        return Some(spdx);
    }

    // A short single-line `License` naming a license the content rules recognise (`MIT License`)
    // is that license, spelled as an identifier.
    let matched = name.and_then(detect_license_from_content);
    if matched.is_some() {
        return matched;
    }

    // Nothing has produced an identifier yet, which is where the distributions whose only statement
    // is a `BSD License` classifier land. The license they shipped says which BSD it is.
    if let Some(shipped) = metadata_directory.and_then(shipped_license) {
        return Some(shipped);
    }

    // Fall back to reporting the field as stated rather than dropping it, and to matching it as
    // text when it turned out to be the license itself rather than a name.
    name.map(str::to_string)
        .or_else(|| stated.and_then(detect_license_from_content))
}

/// The license of a file the metadata directory ships, matched as text.
fn shipped_license(metadata_directory: &Path) -> Option<String> {
    LICENSE_DIRECTORIES
        .iter()
        .find_map(|directory| detect_license_in_dir(&metadata_directory.join(directory)))
}

/// Whether a `License` value names a license rather than reproducing one.
fn is_license_name(license: &str) -> bool {
    license.lines().count() == 1 && license.len() <= MAX_LICENSE_NAME
}

/// The SPDX identifier the distribution's license classifiers name.
///
/// A distribution may carry several, and only the license ones matter. `license_from_classifier`
/// returns nothing for the ones that name a family rather than a license, so a distribution
/// classified only as `BSD License` falls through rather than being assigned a guess.
fn classifier_license(metadata: &Stanza) -> Option<String> {
    metadata
        .all("Classifier")
        .map(str::trim)
        .filter_map(|classifier| classifier.strip_prefix(LICENSE_CLASSIFIER))
        .find_map(license_from_classifier)
        .map(str::to_string)
}

/// Translate a Trove license classifier into an SPDX identifier.
///
/// The classifier is given with the `License :: ` prefix already removed, so both
/// `OSI Approved :: MIT License` and the handful that sit outside `OSI Approved` are matched on
/// their trailing name.
///
/// Returns `None` for the classifiers that name a license family rather than a license.
/// `BSD License` covers BSD-2-Clause, BSD-3-Clause and BSD-4-Clause, and
/// `GNU General Public License (GPL)` names no version at all; picking one would put a license in
/// the distribution's mouth that it never claimed. Those fall through to the license text the wheel
/// shipped, which does say which one it is.
fn license_from_classifier(classifier: &str) -> Option<&'static str> {
    let name = classifier.rsplit(" :: ").next()?.trim();
    let spdx = match name {
        "MIT License" => "MIT",
        "MIT No Attribution License (MIT-0)" => "MIT-0",
        "Apache Software License" => "Apache-2.0",
        "BSD-2-Clause" | "Simplified BSD License" => "BSD-2-Clause",
        "BSD-3-Clause" | "New BSD License" => "BSD-3-Clause",
        "ISC License (ISCL)" => "ISC",
        "GNU General Public License v2 (GPLv2)" => "GPL-2.0-only",
        "GNU General Public License v2 or later (GPLv2+)" => "GPL-2.0-or-later",
        "GNU General Public License v3 (GPLv3)" => "GPL-3.0-only",
        "GNU General Public License v3 or later (GPLv3+)" => "GPL-3.0-or-later",
        "GNU Lesser General Public License v2 (LGPLv2)" => "LGPL-2.0-only",
        "GNU Lesser General Public License v2 or later (LGPLv2+)" => "LGPL-2.0-or-later",
        "GNU Lesser General Public License v3 (LGPLv3)" => "LGPL-3.0-only",
        "GNU Lesser General Public License v3 or later (LGPLv3+)" => "LGPL-3.0-or-later",
        "GNU Affero General Public License v3" => "AGPL-3.0-only",
        "GNU Affero General Public License v3 or later (AGPL v3+)" => "AGPL-3.0-or-later",
        "Mozilla Public License 1.1 (MPL 1.1)" => "MPL-1.1",
        "Mozilla Public License 2.0 (MPL 2.0)" => "MPL-2.0",
        "Eclipse Public License 1.0 (EPL-1.0)" => "EPL-1.0",
        "Eclipse Public License 2.0 (EPL-2.0)" => "EPL-2.0",
        "Boost Software License 1.0 (BSL-1.0)" => "BSL-1.0",
        "zlib/libpng License" => "Zlib",
        "Python Software Foundation License" => "Python-2.0",
        "The Unlicense (Unlicense)" => "Unlicense",
        "CC0 1.0 Universal (CC0 1.0) Public Domain Dedication" => "CC0-1.0",
        "Universal Permissive License (UPL)" => "UPL-1.0",
        "Academic Free License (AFL)" => "AFL-3.0",
        "Artistic License" => "Artistic-2.0",
        "PostgreSQL License" => "PostgreSQL",
        _ => return None,
    };
    Some(spdx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn metadata(content: &str) -> Stanza {
        parse_stanzas(content).into_iter().next().unwrap()
    }

    #[test]
    fn test_recognises_both_metadata_layouts() {
        assert!(is_metadata(&PathBuf::from(
            "usr/lib/python3.12/site-packages/requests-2.32.3.dist-info/METADATA"
        )));
        assert!(is_metadata(&PathBuf::from(
            "usr/lib/python3/dist-packages/PyYAML-6.0.egg-info/PKG-INFO"
        )));
    }

    #[test]
    fn test_metadata_elsewhere_is_not_a_distribution() {
        // The file names alone mean nothing; the metadata directory is what identifies one.
        assert!(!is_metadata(&PathBuf::from("src/METADATA")));
        assert!(!is_metadata(&PathBuf::from(
            "requests-2.32.3.dist-info/RECORD"
        )));
        assert!(!is_metadata(&PathBuf::from("some.egg-info/METADATA")));
    }

    #[test]
    fn test_reads_name_version_and_license() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp
            .path()
            .join("requests-2.32.3.dist-info")
            .join("METADATA");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "Metadata-Version: 2.1\n\
             Name: requests\n\
             Version: 2.32.3\n\
             License: Apache-2.0\n\
             \n\
             # Requests\n\
             \n\
             Requests is an elegant HTTP library.\n",
        )
        .unwrap();

        let artifact = read(&path).expect("metadata names a distribution");
        assert_eq!(artifact.name, "requests");
        assert_eq!(artifact.version, "2.32.3");
        assert_eq!(artifact.license.as_deref(), Some("Apache-2.0"));
        assert_eq!(artifact.ecosystem, Ecosystem::Pypi);
    }

    #[test]
    fn test_license_expression_wins_over_everything() {
        // PEP 639 is the distribution's own SPDX statement; a stale classifier does not override it.
        let stanza = metadata(
            "Name: pkg\n\
             Version: 1.0\n\
             License-Expression: MIT OR Apache-2.0\n\
             License: BSD\n\
             Classifier: License :: OSI Approved :: BSD License\n",
        );
        assert_eq!(license(&stanza, None).as_deref(), Some("MIT OR Apache-2.0"));
    }

    #[test]
    fn test_a_dual_license_field_beats_the_classifiers() {
        // Two classifiers cannot say that either license applies; the expression can.
        let stanza = metadata(
            "Name: pkg\n\
             Version: 1.0\n\
             License: MIT OR Apache-2.0\n\
             Classifier: License :: OSI Approved :: MIT License\n\
             Classifier: License :: OSI Approved :: Apache Software License\n",
        );
        assert_eq!(license(&stanza, None).as_deref(), Some("MIT OR Apache-2.0"));
    }

    #[test]
    fn test_a_classifier_beats_a_free_text_license_field() {
        // `Apache 2.0` is not an identifier; the classifier maps to one.
        let stanza = metadata(
            "Name: pkg\n\
             Version: 1.0\n\
             License: Apache 2.0\n\
             Classifier: License :: OSI Approved :: Apache Software License\n",
        );
        assert_eq!(license(&stanza, None).as_deref(), Some("Apache-2.0"));
    }

    #[test]
    fn test_the_license_field_answers_when_the_classifier_is_a_family() {
        // click's real metadata: the field is precise, the classifier is not.
        let stanza = metadata(
            "Name: click\n\
             Version: 8.1.7\n\
             License: BSD-3-Clause\n\
             Classifier: License :: OSI Approved :: BSD License\n",
        );
        assert_eq!(license(&stanza, None).as_deref(), Some("BSD-3-Clause"));
    }

    #[test]
    fn test_a_spelled_out_license_name_becomes_an_identifier() {
        let stanza = metadata("Name: pkg\nVersion: 1.0\nLicense: MIT License\n");
        assert_eq!(license(&stanza, None).as_deref(), Some("MIT"));
    }

    #[test]
    fn test_an_unrecognised_license_name_is_reported_as_stated() {
        // Passed through rather than dropped: what the distribution said is the honest answer.
        let stanza = metadata("Name: pkg\nVersion: 1.0\nLicense: Public Domain\n");
        assert_eq!(license(&stanza, None).as_deref(), Some("Public Domain"));
    }

    #[test]
    fn test_full_license_text_is_matched_as_text() {
        // Older setuptools packages set `license=open("LICENSE").read()`, which lands the whole
        // license in the field as continuation lines.
        let stanza = metadata(
            "Name: pkg\n\
             Version: 1.0\n\
             License: Copyright (c) 2020 Someone.\n \
             Permission is hereby granted, free of charge, to any person obtaining a copy of this\n \
             software and associated documentation files (the \"Software\"), to deal in the\n \
             Software without restriction.\n",
        );
        assert_eq!(license(&stanza, None).as_deref(), Some("MIT"));
    }

    #[test]
    fn test_the_shipped_license_says_which_bsd_the_classifier_meant() {
        // Jinja2's real metadata: a `BSD License` classifier, no License field, and the license
        // itself in the PEP 639 subdirectory. Both wheel layouts appear in one stock image.
        for directory in ["", "licenses"] {
            let temp = tempfile::tempdir().unwrap();
            let dist_info = temp.path().join("jinja2-3.1.6.dist-info");
            let path = dist_info.join("METADATA");
            std::fs::create_dir_all(dist_info.join(directory)).unwrap();
            std::fs::write(
                &path,
                "Metadata-Version: 2.1\n\
                 Name: Jinja2\n\
                 Version: 3.1.6\n\
                 Classifier: License :: OSI Approved :: BSD License\n\
                 License-File: LICENSE.txt\n",
            )
            .unwrap();
            std::fs::write(
                dist_info.join(directory).join("LICENSE.txt"),
                "Copyright 2007 Pallets\n\n\
                 Redistribution and use in source and binary forms, with or without modification, \
                 are permitted provided that the following conditions are met:\n\
                 1. Redistributions of source code must retain the above copyright notice.\n\
                 3. Neither the name of the copyright holder nor the names of its contributors \
                 may be used to endorse or promote products derived from this software.\n\
                 THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS \"AS IS\" (BSD).\n",
            )
            .unwrap();

            assert_eq!(
                read(&path).unwrap().license.as_deref(),
                Some("BSD-3-Clause"),
                "layout: {directory:?}"
            );
        }
    }

    #[test]
    fn test_a_license_family_classifier_is_not_a_license() {
        // BSD-2-Clause and BSD-3-Clause are different licenses; the classifier names neither.
        let stanza = metadata(
            "Name: pkg\nVersion: 1.0\nClassifier: License :: OSI Approved :: BSD License\n",
        );
        assert_eq!(license(&stanza, None), None);

        let stanza = metadata(
            "Name: pkg\nVersion: 1.0\nClassifier: License :: OSI Approved :: GNU General Public License (GPL)\n",
        );
        assert_eq!(license(&stanza, None), None);
    }

    #[test]
    fn test_a_precise_classifier_is_preferred_to_a_vague_one() {
        let stanza = metadata(
            "Name: pkg\n\
             Version: 1.0\n\
             Classifier: License :: OSI Approved :: GNU General Public License (GPL)\n\
             Classifier: License :: OSI Approved :: GNU General Public License v2 (GPLv2)\n",
        );
        assert_eq!(license(&stanza, None).as_deref(), Some("GPL-2.0-only"));
    }

    #[test]
    fn test_the_unknown_placeholder_is_not_a_license() {
        // setuptools writes `UNKNOWN` into every field the author left unset.
        let stanza = metadata("Name: pkg\nVersion: 1.0\nLicense: UNKNOWN\n");
        assert_eq!(license(&stanza, None), None);
    }

    #[test]
    fn test_metadata_without_a_name_yields_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("METADATA");
        std::fs::write(&path, "Metadata-Version: 2.1\nVersion: 1.0\n").unwrap();
        assert!(read(&path).is_none());
    }

    #[test]
    fn test_non_license_classifiers_are_ignored() {
        let stanza = metadata(
            "Name: pkg\n\
             Version: 1.0\n\
             Classifier: Programming Language :: Python :: 3\n\
             Classifier: License :: OSI Approved :: MIT License\n",
        );
        assert_eq!(license(&stanza, None).as_deref(), Some("MIT"));
    }
}
