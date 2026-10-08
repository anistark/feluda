//! SPDX 2.x tag:value, read and written.
//!
//! Reading maps a tag:value document onto the SPDX JSON shape, so ingest and validation read it
//! with the same code as JSON. Only the tags those two use are mapped; anything else is skipped
//! rather than refused, since a reader that keeps going is worth more to a license gate than one
//! that knows every tag.
//!
//! Writing goes the other way, from the prepared [`SpdxDocument`], so it states exactly what the
//! JSON writer would.
//!
//! An enriched copy of a tag:value input is written in tag:value, by patching the original text:
//! every line feluda did not resolve stays byte for byte what it was.

use serde_json::{json, Map, Value as JsonValue};
use std::ops::Range;

use super::spdx::{ExtractedLicensingInfo, SpdxDocument};

/// Where one package sits in the original text, so an enriched copy can patch it in place.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageLines {
    /// The `PackageName:` line.
    pub start: usize,
    /// The lines of its `PackageLicenseConcluded:` value, when it has one.
    pub concluded: Option<Range<usize>>,
}

/// A parsed tag:value document: the SPDX JSON it stands for, and where each package came from.
#[derive(Debug, Clone)]
pub struct TagValueDocument {
    pub json: JsonValue,
    pub packages: Vec<PackageLines>,
}

/// Whether `content` reads as SPDX tag:value: its first tag is `SPDXVersion`.
pub fn looks_like_tag_value(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .is_some_and(|line| line.starts_with("SPDXVersion:"))
}

/// Which part of the document the tags being read belong to.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Document,
    Package,
    /// Files and snippets: nothing feluda reads, but their `SPDXID` must not land on a package.
    Skipped,
    ExtractedLicense,
}

/// Parse a tag:value document into the SPDX JSON shape.
pub fn parse(content: &str) -> Result<TagValueDocument, String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut document = Map::new();
    let mut creation_info = Map::new();
    let mut packages: Vec<Map<String, JsonValue>> = Vec::new();
    let mut package_lines: Vec<PackageLines> = Vec::new();
    let mut extracted: Vec<Map<String, JsonValue>> = Vec::new();
    let mut relationships: Vec<JsonValue> = Vec::new();
    let mut section = Section::Document;

    let mut index = 0;
    while index < lines.len() {
        let start = index;
        let line = lines[index].trim();
        index += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((tag, value)) = line.split_once(':') else {
            return Err(format!(
                "line {}: expected 'Tag: value', found '{line}'",
                start + 1
            ));
        };
        let tag = tag.trim();
        let mut value = value.trim().to_string();

        // A `<text>` value runs until `</text>`, which may be several lines on.
        if let Some(opened) = value.strip_prefix("<text>") {
            let mut text = opened.to_string();
            loop {
                if let Some(end) = text.find("</text>") {
                    text.truncate(end);
                    break;
                }
                let Some(next) = lines.get(index) else {
                    return Err(format!(
                        "line {}: <text> for {tag} is never closed",
                        start + 1
                    ));
                };
                text.push('\n');
                text.push_str(next);
                index += 1;
            }
            value = text;
        }
        let value_lines = start..index;

        match tag {
            "PackageName" => {
                section = Section::Package;
                let mut package = Map::new();
                package.insert("name".to_string(), json!(value));
                packages.push(package);
                package_lines.push(PackageLines {
                    start,
                    concluded: None,
                });
                continue;
            }
            "FileName" | "SnippetSPDXID" => {
                section = Section::Skipped;
                continue;
            }
            "LicenseID" => {
                section = Section::ExtractedLicense;
                let mut license = Map::new();
                license.insert("licenseId".to_string(), json!(value));
                extracted.push(license);
                continue;
            }
            "Relationship" => {
                let parts: Vec<&str> = value.split_whitespace().collect();
                if let [element, kind, related] = parts[..] {
                    relationships.push(json!({
                        "spdxElementId": element,
                        "relationshipType": kind,
                        "relatedSpdxElement": related,
                    }));
                }
                continue;
            }
            _ => {}
        }

        match section {
            Section::Document => match tag {
                "Creator" => push(&mut creation_info, "creators", json!(value)),
                "Created" => set(&mut creation_info, "created", value),
                "LicenseListVersion" => set(&mut creation_info, "licenseListVersion", value),
                "CreatorComment" => set(&mut creation_info, "comment", value),
                _ => {
                    if let Some(key) = document_key(tag) {
                        set(&mut document, key, value);
                    }
                }
            },
            Section::Package => {
                let (Some(package), Some(lines)) = (packages.last_mut(), package_lines.last_mut())
                else {
                    continue;
                };
                match tag {
                    "FilesAnalyzed" => {
                        package.insert(
                            "filesAnalyzed".to_string(),
                            json!(value.eq_ignore_ascii_case("true")),
                        );
                    }
                    "ExternalRef" => {
                        let parts: Vec<&str> = value.split_whitespace().collect();
                        if let [category, kind, locator] = parts[..] {
                            push(
                                package,
                                "externalRefs",
                                json!({
                                    "referenceCategory": category,
                                    "referenceType": kind,
                                    "referenceLocator": locator,
                                }),
                            );
                        }
                    }
                    "PackageChecksum" => {
                        if let Some((algorithm, checksum)) = value.split_once(':') {
                            push(
                                package,
                                "checksums",
                                json!({
                                    "algorithm": algorithm.trim(),
                                    "checksumValue": checksum.trim(),
                                }),
                            );
                        }
                    }
                    "PackageLicenseInfoFromFiles" => {
                        push(package, "licenseInfoFromFiles", json!(value))
                    }
                    "PackageAttributionText" => push(package, "attributionTexts", json!(value)),
                    _ => {
                        if tag == "PackageLicenseConcluded" {
                            lines.concluded = Some(value_lines);
                        }
                        if let Some(key) = package_key(tag) {
                            set(package, key, value);
                        }
                    }
                }
            }
            Section::ExtractedLicense => {
                let Some(license) = extracted.last_mut() else {
                    continue;
                };
                match tag {
                    "ExtractedText" => set(license, "extractedText", value),
                    "LicenseName" => set(license, "name", value),
                    "LicenseComment" => set(license, "comment", value),
                    "LicenseCrossReference" => push(license, "seeAlsos", json!(value)),
                    _ => {}
                }
            }
            Section::Skipped => {}
        }
    }

    if !creation_info.is_empty() {
        document.insert("creationInfo".to_string(), JsonValue::Object(creation_info));
    }
    document.insert(
        "packages".to_string(),
        JsonValue::Array(packages.into_iter().map(JsonValue::Object).collect()),
    );
    if !relationships.is_empty() {
        document.insert("relationships".to_string(), JsonValue::Array(relationships));
    }
    if !extracted.is_empty() {
        document.insert(
            "hasExtractedLicensingInfos".to_string(),
            JsonValue::Array(extracted.into_iter().map(JsonValue::Object).collect()),
        );
    }

    Ok(TagValueDocument {
        json: JsonValue::Object(document),
        packages: package_lines,
    })
}

/// The JSON field a document level tag stands for.
fn document_key(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "SPDXVersion" => "spdxVersion",
        "DataLicense" => "dataLicense",
        "SPDXID" => "SPDXID",
        "DocumentName" => "name",
        "DocumentNamespace" => "documentNamespace",
        "DocumentComment" => "comment",
        _ => return None,
    })
}

/// The JSON field a single valued package tag stands for.
fn package_key(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "SPDXID" => "SPDXID",
        "PackageVersion" => "versionInfo",
        "PackageFileName" => "packageFileName",
        "PackageSupplier" => "supplier",
        "PackageOriginator" => "originator",
        "PackageDownloadLocation" => "downloadLocation",
        "PackageHomePage" => "homepage",
        "PackageSourceInfo" => "sourceInfo",
        "PackageLicenseConcluded" => "licenseConcluded",
        "PackageLicenseDeclared" => "licenseDeclared",
        "PackageLicenseComments" => "licenseComments",
        "PackageCopyrightText" => "copyrightText",
        "PackageSummary" => "summary",
        "PackageDescription" => "description",
        "PackageComment" => "comment",
        "PrimaryPackagePurpose" => "primaryPackagePurpose",
        _ => return None,
    })
}

fn set(object: &mut Map<String, JsonValue>, key: &str, value: String) {
    object.insert(key.to_string(), JsonValue::String(value));
}

fn push(object: &mut Map<String, JsonValue>, key: &str, value: JsonValue) {
    match object
        .entry(key.to_string())
        .or_insert_with(|| JsonValue::Array(Vec::new()))
    {
        JsonValue::Array(values) => values.push(value),
        other => *other = JsonValue::Array(vec![value]),
    }
}

// =============================================================================
// ENRICHED COPY
// =============================================================================

/// The original text with the given packages' concluded licenses set, and `refs` defined at the
/// end.
///
/// `concluded` pairs a package's position (its index in [`TagValueDocument::packages`]) with the
/// license to state. An existing `PackageLicenseConcluded` is replaced where it stands; a package
/// without one gets it on the line after `PackageName`.
pub fn patch(
    content: &str,
    packages: &[PackageLines],
    concluded: &[(usize, String)],
    refs: &[ExtractedLicensingInfo],
) -> String {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    // What replaces a line range, and what goes in after a line.
    let mut replaced: Vec<(Range<usize>, String)> = Vec::new();
    let mut inserted: Vec<(usize, String)> = Vec::new();
    for (position, license) in concluded {
        let Some(package) = packages.get(*position) else {
            continue;
        };
        let line = format!("PackageLicenseConcluded: {}{newline}", single_line(license));
        match &package.concluded {
            Some(range) => replaced.push((range.clone(), line)),
            None => inserted.push((package.start, line)),
        }
    }

    let mut output = String::with_capacity(content.len());
    let mut index = 0;
    while index < lines.len() {
        if let Some((range, line)) = replaced.iter().find(|(range, _)| range.start == index) {
            output.push_str(line);
            index = range.end.max(index + 1);
            continue;
        }
        output.push_str(lines[index]);
        // A last line without a newline still needs one before anything is added after it.
        if !lines[index].ends_with('\n')
            && (inserted.iter().any(|(after, _)| *after == index) || !refs.is_empty())
        {
            output.push_str(newline);
        }
        for (_, line) in inserted.iter().filter(|(after, _)| *after == index) {
            output.push_str(line);
        }
        index += 1;
    }

    if !refs.is_empty() {
        output.push_str(newline);
        output.push_str(&format!("## Licenses defined by Feluda{newline}"));
        for info in refs {
            output.push_str(&extracted_license(info).replace('\n', newline));
        }
    }
    output
}

// =============================================================================
// WRITING
// =============================================================================

/// A prepared SPDX 2.x document in tag:value.
///
/// `document` is what `prepare_spdx_document` returns: sanitized, every license stated the way
/// SPDX accepts, and already written down to the version asked for.
pub fn write(document: &SpdxDocument) -> String {
    let mut out = String::new();
    let tag = |out: &mut String, name: &str, value: &str| {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push('\n');
    };

    tag(&mut out, "SPDXVersion", &document.spdx_version);
    tag(&mut out, "DataLicense", &document.data_license);
    tag(&mut out, "SPDXID", &document.spdx_id);
    tag(&mut out, "DocumentName", &single_line(&document.name));
    tag(&mut out, "DocumentNamespace", &document.document_namespace);
    for creator in &document.creation_info.creators {
        tag(&mut out, "Creator", &single_line(creator));
    }
    tag(
        &mut out,
        "Created",
        &document
            .creation_info
            .created
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    if let Some(version) = &document.creation_info.license_list_version {
        tag(&mut out, "LicenseListVersion", version);
    }

    for package in &document.packages {
        out.push_str(&format!(
            "\n##### Package: {}\n\n",
            single_line(&package.name)
        ));
        tag(&mut out, "PackageName", &single_line(&package.name));
        tag(&mut out, "SPDXID", &package.spdx_id);
        if let Some(version) = &package.version_info {
            tag(&mut out, "PackageVersion", &single_line(version));
        }
        tag(
            &mut out,
            "PackageDownloadLocation",
            &package.download_location,
        );
        tag(
            &mut out,
            "FilesAnalyzed",
            if package.files_analyzed {
                "true"
            } else {
                "false"
            },
        );
        if let Some(license) = &package.license_concluded {
            tag(&mut out, "PackageLicenseConcluded", license);
        }
        if let Some(license) = &package.license_declared {
            tag(&mut out, "PackageLicenseDeclared", license);
        }
        if let Some(comments) = &package.license_comments {
            tag(&mut out, "PackageLicenseComments", &text(comments));
        }
        if let Some(copyright) = &package.copyright_text {
            tag(
                &mut out,
                "PackageCopyrightText",
                &text_or_keyword(copyright),
            );
        }
        if let Some(comment) = &package.comment {
            tag(&mut out, "PackageComment", &text(comment));
        }
        for reference in &package.external_refs {
            tag(
                &mut out,
                "ExternalRef",
                &format!(
                    "{} {} {}",
                    reference.reference_category,
                    reference.reference_type,
                    reference.reference_locator
                ),
            );
            if let Some(comment) = &reference.comment {
                tag(&mut out, "ExternalRefComment", &text(comment));
            }
        }
    }

    if !document.relationships.is_empty() {
        out.push_str("\n##### Relationships\n\n");
        for relationship in &document.relationships {
            tag(
                &mut out,
                "Relationship",
                &format!(
                    "{} {} {}",
                    relationship.spdx_element_id,
                    relationship.relationship_type,
                    relationship.related_spdx_element
                ),
            );
        }
    }

    if !document.annotations.is_empty() {
        out.push_str("\n##### Annotations\n\n");
        for annotation in &document.annotations {
            tag(&mut out, "Annotator", &single_line(&annotation.annotator));
            tag(
                &mut out,
                "AnnotationDate",
                &annotation
                    .annotation_date
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            );
            tag(&mut out, "AnnotationType", &annotation.annotation_type);
            tag(&mut out, "SPDXREF", &annotation.spdx_identifier_reference);
            tag(&mut out, "AnnotationComment", &text(&annotation.comment));
        }
    }

    if !document.has_extracted_licensing_infos.is_empty() {
        out.push_str("\n##### Licenses not on the SPDX list\n");
        for info in &document.has_extracted_licensing_infos {
            out.push_str(&extracted_license(info));
        }
    }

    out
}

/// One `LicenseID` block, preceded by a blank line.
fn extracted_license(info: &ExtractedLicensingInfo) -> String {
    let mut out = format!(
        "\nLicenseID: {}\nExtractedText: {}\n",
        info.license_id,
        text(&info.extracted_text)
    );
    if let Some(name) = &info.name {
        out.push_str(&format!("LicenseName: {}\n", single_line(name)));
    }
    if let Some(comment) = &info.comment {
        out.push_str(&format!("LicenseComment: {}\n", text(comment)));
    }
    out
}

/// Free text as a `<text>` value. A literal `</text>` inside would end it early, so it is broken.
fn text(value: &str) -> String {
    format!("<text>{}</text>", value.replace("</text>", "</ text>"))
}

/// `NONE` and `NOASSERTION` are keywords; anything else is free text.
fn text_or_keyword(value: &str) -> String {
    if value == "NONE" || value == "NOASSERTION" {
        value.to_string()
    } else {
        text(value)
    }
}

/// A value that has to fit on its tag's line.
fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYFT_STYLE: &str = "SPDXVersion: SPDX-2.3
DataLicense: CC0-1.0
SPDXID: SPDXRef-DOCUMENT
DocumentName: nginx
DocumentNamespace: https://anchore.com/syft/image/nginx-1234
Creator: Organization: Anchore, Inc
Creator: Tool: syft-1.52.0
Created: 2026-10-07T10:00:00Z

##### Package: libssl3

PackageName: libssl3
SPDXID: SPDXRef-Package-deb-libssl3
PackageVersion: 3.0.15-1
PackageDownloadLocation: NOASSERTION
FilesAnalyzed: false
PackageLicenseConcluded: NOASSERTION
PackageLicenseDeclared: Apache-2.0
PackageCopyrightText: NOASSERTION
ExternalRef: PACKAGE-MANAGER purl pkg:deb/debian/libssl3@3.0.15-1?arch=amd64

##### File: /etc/ssl/openssl.cnf

FileName: /etc/ssl/openssl.cnf
SPDXID: SPDXRef-File-openssl
LicenseConcluded: NOASSERTION

##### Package: lodash

PackageName: lodash
SPDXID: SPDXRef-Package-npm-lodash
PackageVersion: 4.17.21
PackageDownloadLocation: NOASSERTION
PackageLicenseDeclared: LicenseRef-1
PackageComment: <text>first line
second line</text>
ExternalRef: PACKAGE-MANAGER purl pkg:npm/lodash@4.17.21

##### Relationships

Relationship: SPDXRef-DOCUMENT DESCRIBES SPDXRef-Package-deb-libssl3

LicenseID: LicenseRef-1
ExtractedText: <text>Permission is hereby granted, free of charge,
to any person</text>
LicenseName: MIT-ish
";

    #[test]
    fn test_detects_tag_value() {
        assert!(looks_like_tag_value(SYFT_STYLE));
        assert!(looks_like_tag_value("# comment\n\nSPDXVersion: SPDX-2.2\n"));
        assert!(!looks_like_tag_value("{\"spdxVersion\": \"SPDX-2.3\"}"));
        assert!(!looks_like_tag_value(
            "DocumentName: x\nSPDXVersion: SPDX-2.3"
        ));
    }

    #[test]
    fn test_parses_into_the_json_shape() {
        let parsed = parse(SYFT_STYLE).unwrap();
        let json = &parsed.json;
        assert_eq!(json["spdxVersion"], "SPDX-2.3");
        assert_eq!(json["SPDXID"], "SPDXRef-DOCUMENT");
        assert_eq!(json["name"], "nginx");
        assert_eq!(
            json["creationInfo"]["creators"],
            json!(["Organization: Anchore, Inc", "Tool: syft-1.52.0"])
        );

        let packages = json["packages"].as_array().unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0]["SPDXID"], "SPDXRef-Package-deb-libssl3");
        assert_eq!(packages[0]["filesAnalyzed"], false);
        assert_eq!(packages[0]["licenseDeclared"], "Apache-2.0");
        assert_eq!(
            packages[0]["externalRefs"][0]["referenceLocator"],
            "pkg:deb/debian/libssl3@3.0.15-1?arch=amd64"
        );
        // The file's SPDXID belongs to the file, not to the package before it.
        assert_eq!(packages[1]["SPDXID"], "SPDXRef-Package-npm-lodash");
        assert_eq!(packages[1]["comment"], "first line\nsecond line");

        assert_eq!(
            json["relationships"][0]["relatedSpdxElement"],
            "SPDXRef-Package-deb-libssl3"
        );
        let extracted = &json["hasExtractedLicensingInfos"][0];
        assert_eq!(extracted["licenseId"], "LicenseRef-1");
        assert_eq!(
            extracted["extractedText"],
            "Permission is hereby granted, free of charge,\nto any person"
        );
        assert_eq!(extracted["name"], "MIT-ish");

        assert_eq!(parsed.packages[0].start, 11);
        assert_eq!(parsed.packages[0].concluded, Some(16..17));
        assert_eq!(parsed.packages[1].concluded, None);
    }

    #[test]
    fn test_parse_errors_name_the_line() {
        let error = parse("SPDXVersion: SPDX-2.3\nnot a tag\n").unwrap_err();
        assert!(error.contains("line 2"), "{error}");

        let error = parse("SPDXVersion: SPDX-2.3\nDocumentComment: <text>open\n").unwrap_err();
        assert!(error.contains("never closed"), "{error}");
    }

    #[test]
    fn test_patch_replaces_or_inserts_the_conclusion() {
        let parsed = parse(SYFT_STYLE).unwrap();
        let refs = vec![ExtractedLicensingInfo {
            license_id: "LicenseRef-feluda-Acme".to_string(),
            extracted_text: "Acme".to_string(),
            name: Some("Acme".to_string()),
            comment: None,
        }];
        let patched = patch(
            SYFT_STYLE,
            &parsed.packages,
            &[
                (0, "OpenSSL".to_string()),
                (1, "LicenseRef-feluda-Acme".to_string()),
            ],
            &refs,
        );

        assert!(patched.contains("PackageLicenseConcluded: OpenSSL\nPackageLicenseDeclared"));
        assert!(!patched.contains("PackageLicenseConcluded: NOASSERTION"));
        assert!(patched.contains(
            "PackageName: lodash\nPackageLicenseConcluded: LicenseRef-feluda-Acme\nSPDXID"
        ));
        assert!(patched.ends_with("\nLicenseID: LicenseRef-feluda-Acme\nExtractedText: <text>Acme</text>\nLicenseName: Acme\n"));

        // Everything else is untouched, and the result still parses.
        let reparsed = parse(&patched).unwrap();
        assert_eq!(
            reparsed.json["packages"][1]["comment"],
            "first line\nsecond line"
        );
        assert_eq!(
            reparsed.json["packages"][1]["licenseConcluded"],
            "LicenseRef-feluda-Acme"
        );
        assert_eq!(
            reparsed.json["hasExtractedLicensingInfos"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn test_patch_keeps_crlf_and_a_missing_final_newline() {
        let content = "SPDXVersion: SPDX-2.3\r\nPackageName: a\r\nSPDXID: SPDXRef-a";
        let parsed = parse(content).unwrap();
        let patched = patch(content, &parsed.packages, &[(0, "MIT".to_string())], &[]);
        assert_eq!(
            patched,
            "SPDXVersion: SPDX-2.3\r\nPackageName: a\r\nPackageLicenseConcluded: MIT\r\nSPDXID: SPDXRef-a"
        );
    }

    #[test]
    fn test_write_round_trips_through_parse() {
        let mut document = SpdxDocument::new("demo");
        document.add_package(
            super::super::spdx::SpdxPackage::new("serde", &document.document_namespace)
                .with_version("1.0.219")
                .with_purl("pkg:cargo/serde@1.0.219")
                .with_license("MIT OR Apache-2.0"),
        );
        document
            .has_extracted_licensing_infos
            .push(ExtractedLicensingInfo {
                license_id: "LicenseRef-x".to_string(),
                extracted_text: "two\nlines </text> inside".to_string(),
                name: Some("X".to_string()),
                comment: None,
            });

        let written = write(&document);
        assert!(written.starts_with("SPDXVersion: SPDX-2.3\nDataLicense: CC0-1.0\n"));
        assert!(written.contains("ExternalRef: PACKAGE-MANAGER purl pkg:cargo/serde@1.0.219\n"));

        let parsed = parse(&written).unwrap().json;
        let package = &parsed["packages"][0];
        assert_eq!(package["name"], "serde");
        assert_eq!(package["versionInfo"], "1.0.219");
        assert_eq!(package["licenseConcluded"], "MIT OR Apache-2.0");
        assert_eq!(package["copyrightText"], "NOASSERTION");
        assert_eq!(parsed["relationships"][0]["relationshipType"], "DESCRIBES");
        assert_eq!(
            parsed["hasExtractedLicensingInfos"][0]["extractedText"],
            "two\nlines </ text> inside"
        );
    }
}
