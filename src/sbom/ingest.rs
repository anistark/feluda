//! Reading an existing SBOM as a scan source.
//!
//! Every other entry point starts at a project's manifests, which means feluda only ever sees what
//! a source tree declares — never what a shipped container or a vendor-supplied artifact actually
//! contains. Cataloging those is syft's competency; classifying what they contain is feluda's. So
//! an SPDX or CycloneDX document produced by anyone else becomes an input:
//!
//! ```sh
//! syft nginx:latest -o spdx-json | feluda --sbom-input - --fail-on-restrictive
//! ```
//!
//! Components map onto [`LicenseInfo`] keyed by the PURL the document carries, and from there the
//! normal pipeline applies: licenses the document left as `NOASSERTION` are resolved against the
//! package's registry, everything is classified for restrictiveness and compatibility, and the CI
//! gates behave exactly as they do on a source scan.

use rayon::prelude::*;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::fs;
use std::io::Read;

use crate::cli::with_spinner;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::languages::resolve_license_for;
use crate::licenses::{
    classify_findings, detect_license_from_content, LicenseCompatibility, LicenseInfo, OsiStatus,
};
use crate::purl::{parse_purl, Ecosystem};
use crate::sbom::cyclonedx::convert_spdx_license_to_cyclonedx;
use crate::sbom::input::{read_sbom, Original, SbomDocument};
use crate::sbom::spdx::{spdx_license_field, LicenseRefs};
use crate::sbom::{cyclonedx_xml, spdx3, tagvalue, CycloneDxVersion, SbomType};

/// The source argument that means "read the document from stdin".
const STDIN_SOURCE: &str = "-";

/// Where a component sat in the document it came from, and whether feluda had to resolve its
/// license. Together these are what an enriched copy needs in order to write conclusions back to
/// the right entries and only to those.
struct Origin {
    position: usize,
    resolved: bool,
}

/// Read an SBOM and turn its components into analyzable dependencies.
///
/// `source` is a file path, or `-` for stdin. When `enriched_output` is set, the input document is
/// written back out with the licenses feluda resolved filled in.
pub fn ingest_sbom(
    source: &str,
    strict: bool,
    enriched_output: Option<&str>,
) -> FeludaResult<Vec<LicenseInfo>> {
    let content = read_source(source)?;
    let document = read_sbom(&content).map_err(input_error)?;
    log(
        LogLevel::Info,
        &format!(
            "Ingesting {} document from {source}",
            document.serialization.describe()
        ),
    );

    let (mut components, mut origins) = match document.serialization.sbom_type() {
        SbomType::Spdx => extract_spdx(&document.model),
        SbomType::CycloneDx => extract_cyclonedx(&document.model),
    };

    log(
        LogLevel::Info,
        &format!("Read {} components from the SBOM", components.len()),
    );

    resolve_missing_licenses(&mut components, &mut origins);
    classify_findings(&mut components, strict);
    // Before the enriched document is written, so what it states is everything feluda resolved.
    for index in crate::clearlydefined::resolve_unknown_licenses(&mut components, strict) {
        origins[index].resolved = true;
    }

    if let Some(output_path) = enriched_output {
        write_enriched(&document, &components, &origins, output_path)?;
    }

    Ok(components)
}

/// Read the document from a file, or from stdin when the source is `-`.
fn read_source(source: &str) -> FeludaResult<String> {
    if source == STDIN_SOURCE {
        log(LogLevel::Info, "Reading SBOM from stdin");
        let mut content = String::new();
        std::io::stdin()
            .read_to_string(&mut content)
            .map_err(|e| input_error(format!("Failed to read SBOM from stdin: {e}")))?;
        return Ok(content);
    }

    fs::read_to_string(source)
        .map_err(|e| input_error(format!("Failed to read SBOM file {source}: {e}")))
}

/// Report a bad input and turn it into an error.
///
/// Whoever piped the document in has to be told what was wrong with it, and `FeludaError::log`
/// only prints under `--debug` — an unexplained exit code is no use in a pipeline.
fn input_error(message: String) -> FeludaError {
    eprintln!("❌ {message}");
    FeludaError::Parser(message)
}

// =============================================================================
// COMPONENT EXTRACTION
// =============================================================================

/// Read the `packages` of an SPDX document.
fn extract_spdx(document: &JsonValue) -> (Vec<LicenseInfo>, Vec<Origin>) {
    let extracted_licenses = extracted_licensing_info(document);

    collect(document, "packages", |package| {
        let name = string_field(package, "name")?;
        let version = string_field(package, "versionInfo").unwrap_or_default();
        let license = spdx_license(package).map(|id| expand_license_refs(&id, &extracted_licenses));
        Some(component_info(
            spdx_purl(package).as_deref(),
            &name,
            &version,
            license,
        ))
    })
}

/// Read the `components` of a CycloneDX document.
///
/// Only the top-level array is read. Nested `components[].components` are rare — syft, Trivy and
/// cdxgen all emit flat inventories — and staying flat keeps a component's array position usable
/// as the write-back key for the enriched copy.
fn extract_cyclonedx(document: &JsonValue) -> (Vec<LicenseInfo>, Vec<Origin>) {
    collect(document, "components", |component| {
        let name = string_field(component, "name")?;
        let version = string_field(component, "version").unwrap_or_default();
        let purl = string_field(component, "purl");
        Some(component_info(
            purl.as_deref(),
            &name,
            &version,
            cyclonedx_license(component),
        ))
    })
}

/// Walk an array of document entries, keeping each usable one alongside its position.
fn collect(
    document: &JsonValue,
    key: &str,
    read: impl Fn(&JsonValue) -> Option<LicenseInfo>,
) -> (Vec<LicenseInfo>, Vec<Origin>) {
    let Some(entries) = document.get(key).and_then(|v| v.as_array()) else {
        log(
            LogLevel::Warn,
            &format!("SBOM has no `{key}` array; nothing to analyze"),
        );
        return (Vec::new(), Vec::new());
    };

    let mut components = Vec::with_capacity(entries.len());
    let mut origins = Vec::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate() {
        match read(entry) {
            Some(info) => {
                components.push(info);
                origins.push(Origin {
                    position,
                    resolved: false,
                });
            }
            // An entry with no name identifies nothing, so there is nothing to look up or report.
            None => log(
                LogLevel::Warn,
                &format!("Skipping unnamed {key} entry at position {position}"),
            ),
        }
    }
    (components, origins)
}

/// Build a dependency from a component's identity and whatever license the document gave it.
///
/// The PURL is the identity when there is one: it names the ecosystem, which is what decides where
/// a missing license gets resolved from and what keeps a Debian `libssl3` distinct from an npm
/// package of the same name. Without a PURL the component is generic, and only its own document
/// can say anything about its license.
fn component_info(
    purl: Option<&str>,
    name: &str,
    version: &str,
    license: Option<String>,
) -> LicenseInfo {
    let (ecosystem, name, version, qualifiers) = match purl.and_then(parse_purl) {
        Some(parsed) => {
            // A versionless PURL still leaves the document's own version field to fall back on.
            let version = if parsed.version.is_empty() {
                version.to_string()
            } else {
                parsed.version
            };
            // Kept so an SBOM generated from this one says `arch` and `distro` wherever the input
            // did.
            (parsed.ecosystem, parsed.name, version, parsed.qualifiers)
        }
        None => (
            Ecosystem::Generic,
            name.to_string(),
            version.to_string(),
            Default::default(),
        ),
    };

    LicenseInfo {
        name,
        version,
        license,
        // Filled in by `classify` once every license that can be resolved has been.
        is_restrictive: false,
        compatibility: LicenseCompatibility::Unknown,
        osi_status: OsiStatus::Unknown,
        ecosystem,
        sub_project: None,
        qualifiers,
    }
}

// =============================================================================
// LICENSE FIELDS
// =============================================================================

/// Whether a license field names a license, as opposed to saying nothing.
///
/// SPDX spells "nothing" as `NOASSERTION` (nobody looked) or `NONE` (looked, found no license);
/// both mean feluda should try to resolve one itself.
fn is_stated(license: &str) -> bool {
    let license = license.trim();
    !license.is_empty()
        && !license.eq_ignore_ascii_case("NOASSERTION")
        && !license.eq_ignore_ascii_case("NONE")
}

/// The license an SPDX package states: the concluded license, or the declared one when no
/// conclusion was recorded.
fn spdx_license(package: &JsonValue) -> Option<String> {
    ["licenseConcluded", "licenseDeclared"]
        .iter()
        .filter_map(|field| string_field(package, field))
        .find(|license| is_stated(license))
}

/// The PURL an SPDX package carries in its external references.
fn spdx_purl(package: &JsonValue) -> Option<String> {
    package
        .get("externalRefs")?
        .as_array()?
        .iter()
        .find(|reference| {
            string_field(reference, "referenceType")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("purl"))
        })
        .and_then(|reference| string_field(reference, "referenceLocator"))
}

/// Map every `LicenseRef-*` the document defines to a real SPDX id.
///
/// A `LicenseRef` is an id local to one document, so it means nothing to feluda's classification
/// on its own. The extracted text is the license itself, though, which is exactly what feluda's
/// content detector reads; failing that, the entry's human name is better than an opaque ref.
fn extracted_licensing_info(document: &JsonValue) -> HashMap<String, String> {
    let Some(entries) = document
        .get("hasExtractedLicensingInfos")
        .and_then(|v| v.as_array())
    else {
        return HashMap::new();
    };

    entries
        .iter()
        .filter_map(|entry| {
            let license_id = string_field(entry, "licenseId")?;
            let detected = string_field(entry, "extractedText")
                .and_then(|text| detect_license_from_content(&text))
                .or_else(|| string_field(entry, "name").filter(|name| is_stated(name)))?;
            log(
                LogLevel::Info,
                &format!("Resolved {license_id} to {detected} from the document's extracted text"),
            );
            Some((license_id, detected))
        })
        .collect()
}

/// Substitute the document's `LicenseRef-*` ids for the real licenses they stand for.
///
/// Refs are replaced longest id first, so `LicenseRef-1` cannot eat the prefix of `LicenseRef-10`.
/// Substitution is textual because a ref can appear anywhere inside a compound expression.
pub(super) fn expand_license_refs(license: &str, extracted: &HashMap<String, String>) -> String {
    if extracted.is_empty() || !license.contains("LicenseRef-") {
        return license.to_string();
    }

    let mut refs: Vec<(&String, &String)> = extracted.iter().collect();
    refs.sort_by_key(|(id, _)| std::cmp::Reverse(id.len()));

    let mut expanded = license.to_string();
    for (license_id, detected) in refs {
        expanded = expanded.replace(license_id.as_str(), detected);
    }
    expanded
}

/// The license a CycloneDX component states.
///
/// The array holds either SPDX expressions or individual ids and names. Multiple entries are
/// joined with `AND`: CycloneDX has no way to say the entries are alternatives — a dual license
/// arrives as a single `expression` entry — and for a compliance gate, treating several stated
/// licenses as cumulative is the reading that cannot understate an obligation.
fn cyclonedx_license(component: &JsonValue) -> Option<String> {
    let entries = component.get("licenses")?.as_array()?;

    let mut licenses: Vec<String> = Vec::new();
    for entry in entries {
        let stated = string_field(entry, "expression").or_else(|| {
            let license = entry.get("license")?;
            string_field(license, "id").or_else(|| string_field(license, "name"))
        });

        if let Some(stated) = stated.filter(|license| is_stated(license)) {
            let stated = stated.trim().to_string();
            if !licenses.contains(&stated) {
                licenses.push(stated);
            }
        }
    }

    match licenses.len() {
        0 => None,
        1 => licenses.pop(),
        _ => Some(
            licenses
                .iter()
                // An alternative inside a conjunction has to keep its own grouping.
                .map(|license| {
                    if crate::spdx::is_compound(license) {
                        format!("({license})")
                    } else {
                        license.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" AND "),
        ),
    }
}

/// Read a string field, trimmed, treating an empty value as absent.
fn string_field(value: &JsonValue, key: &str) -> Option<String> {
    let field = value.get(key)?.as_str()?.trim();
    if field.is_empty() {
        None
    } else {
        Some(field.to_string())
    }
}

// =============================================================================
// RESOLUTION AND CLASSIFICATION
// =============================================================================

/// Ask each package's registry about the components the document left unlicensed.
///
/// A component that already states a license costs nothing here, which is what keeps a fully
/// populated document an offline operation.
fn resolve_missing_licenses(components: &mut [LicenseInfo], origins: &mut [Origin]) {
    let unresolved = components
        .iter()
        .filter(|info| info.license.is_none())
        .count();
    if unresolved == 0 {
        return;
    }

    log(
        LogLevel::Info,
        &format!("Resolving licenses for {unresolved} components the SBOM left unstated"),
    );

    with_spinner("🔍: licenses missing from the SBOM", |indicator| {
        let resolved: Vec<Option<String>> = components
            .par_iter()
            .map(|info| {
                if info.license.is_some() {
                    return None;
                }
                resolve_license_for(info.ecosystem, &info.name, &info.version)
            })
            .collect();

        let mut count = 0;
        for ((info, origin), license) in components.iter_mut().zip(origins).zip(resolved) {
            if let Some(license) = license {
                info.license = Some(license);
                origin.resolved = true;
                count += 1;
            }
        }
        indicator.update_progress(&format!("{count} of {unresolved} resolved"));
    });
}

// =============================================================================
// ENRICHED OUTPUT
// =============================================================================

/// Write the input document back out with the licenses feluda resolved, in the serialization it
/// arrived in.
///
/// Only components feluda actually resolved are touched, so a document that already stated every
/// license round-trips unchanged rather than being rewritten with feluda's opinion of it.
fn write_enriched(
    document: &SbomDocument,
    components: &[LicenseInfo],
    origins: &[Origin],
    output_path: &str,
) -> FeludaResult<()> {
    // What feluda resolved, by the position of the entry it came from.
    let resolved: Vec<(usize, &str)> = components
        .iter()
        .zip(origins)
        .filter_map(|(info, origin)| {
            let license = info.license.as_deref().filter(|_| origin.resolved)?;
            Some((origin.position, license))
        })
        .collect();

    let serialize = |value: &JsonValue| {
        serde_json::to_string_pretty(value).map_err(|e| {
            FeludaError::Serialization(format!("Failed to serialize enriched SBOM: {e}"))
        })
    };
    let written = match &document.original {
        Original::Json => serialize(&enriched_json(
            &document.model,
            document.serialization.sbom_type(),
            &resolved,
        ))?,
        // The resolved license is a conclusion feluda drew, not something the document declared,
        // which is exactly the distinction `PackageLicenseConcluded` carries.
        Original::TagValue { text, packages } => {
            let mut refs = LicenseRefs::with_existing(existing_refs(&document.model));
            let concluded: Vec<(usize, String)> = resolved
                .iter()
                .map(|(position, license)| (*position, spdx_license_field(license, &mut refs)))
                .collect();
            tagvalue::patch(text, packages, &concluded, &refs.into_defined())
        }
        Original::Xml(text) => {
            let acknowledgement = acknowledgement(&document.model);
            let licenses: Vec<_> = resolved
                .iter()
                .map(|(position, license)| {
                    (
                        *position,
                        convert_spdx_license_to_cyclonedx(license, acknowledgement),
                    )
                })
                .collect();
            cyclonedx_xml::patch(text, &licenses).map_err(|e| {
                FeludaError::Serialization(format!("Failed to write enriched SBOM: {e}"))
            })?
        }
        // A 3.0 conclusion is a relationship to a license element, found by the package's id.
        // Refs are scoped to the expression that maps them, so none can collide with the
        // document's own.
        Original::Spdx3(graph) => {
            let mut refs = LicenseRefs::default();
            let concluded: Vec<(String, String)> = resolved
                .iter()
                .filter_map(|(position, license)| {
                    let id = document.model["packages"].get(*position)?["SPDXID"].as_str()?;
                    Some((id.to_string(), spdx_license_field(license, &mut refs)))
                })
                .collect();
            serialize(&spdx3::enrich(graph, &concluded, &refs.into_defined()))?
        }
    };

    fs::write(output_path, written).map_err(|e| {
        FeludaError::FileWrite(format!(
            "Failed to write enriched SBOM to {output_path}: {e}"
        ))
    })?;

    let patched = resolved.len();
    log(
        LogLevel::Info,
        &format!(
            "Wrote enriched {} to {output_path} with {patched} resolved licenses",
            document.serialization.describe()
        ),
    );
    // Stderr, so the enriched copy can be written during a `--json` run without landing in the
    // report a pipeline is reading.
    eprintln!("✓ Enriched SBOM written to {output_path} ({patched} licenses resolved)");

    Ok(())
}

/// The `LicenseRef-` ids an SPDX 2.x document already defines, with their text, so new ones
/// neither repeat nor collide with them.
fn existing_refs(document: &JsonValue) -> Vec<(String, String)> {
    document
        .get("hasExtractedLicensingInfos")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|info| {
            Some((
                string_field(info, "licenseId")?,
                string_field(info, "extractedText").unwrap_or_default(),
            ))
        })
        .collect()
}

/// From 1.6 a CycloneDX license can say whether it was declared or concluded, and what feluda
/// resolved is a conclusion, the same distinction `licenseConcluded` carries in SPDX.
fn acknowledgement(document: &JsonValue) -> Option<&'static str> {
    string_field(document, "specVersion")
        .and_then(|version| <CycloneDxVersion as clap::ValueEnum>::from_str(&version, true).ok())
        .filter(|version| *version >= CycloneDxVersion::V1_6)
        .map(|_| "concluded")
}

/// A JSON document with the resolved licenses written into its entries.
fn enriched_json(document: &JsonValue, format: SbomType, resolved: &[(usize, &str)]) -> JsonValue {
    let mut enriched = document.clone();
    let key = match format {
        SbomType::Spdx => "packages",
        SbomType::CycloneDx => "components",
    };

    let mut refs = LicenseRefs::with_existing(existing_refs(document));
    let acknowledgement = acknowledgement(document);
    for (position, license) in resolved {
        let Some(entry) = enriched
            .get_mut(key)
            .and_then(|entries| entries.get_mut(*position))
        else {
            continue;
        };

        match format {
            // A license outside the SPDX list is written as a `LicenseRef-` the document defines.
            SbomType::Spdx => {
                entry["licenseConcluded"] = json!(spdx_license_field(license, &mut refs));
            }
            // Stated the way `feluda sbom` writes it: a listed id as `id`, an expression over listed
            // licenses as `expression`, and anything else, such as a registry's free-form title,
            // as `name`, since calling it an `id` would make the document invalid.
            SbomType::CycloneDx => {
                let stated = json!(convert_spdx_license_to_cyclonedx(license, acknowledgement));
                entry["licenses"] = json!([stated]);
            }
        }
    }

    let extracted_refs: Vec<JsonValue> =
        refs.into_defined().iter().map(|info| json!(info)).collect();
    if !extracted_refs.is_empty() {
        let existing = enriched
            .get_mut("hasExtractedLicensingInfos")
            .and_then(|value| value.as_array_mut());
        match existing {
            Some(entries) => entries.extend(extracted_refs),
            None => enriched["hasExtractedLicensingInfos"] = JsonValue::Array(extracted_refs),
        }
    }
    enriched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sbom::detect_sbom_type_in;
    use crate::sbom::input::Serialization;
    use serde_json::json;

    /// Write an enriched copy of a JSON document, as ingest does for a JSON input.
    fn write_json(
        document: &JsonValue,
        format: SbomType,
        components: &[LicenseInfo],
        origins: &[Origin],
        output_path: &str,
    ) -> FeludaResult<()> {
        let serialization = match format {
            SbomType::Spdx => Serialization::SpdxJson,
            SbomType::CycloneDx => Serialization::CycloneDxJson,
        };
        let document = SbomDocument {
            serialization,
            model: document.clone(),
            original: Original::Json,
        };
        write_enriched(&document, components, origins, output_path)
    }

    fn spdx_fixture() -> JsonValue {
        json!({
            "spdxVersion": "SPDX-2.3",
            "name": "nginx:latest",
            "packages": [
                {
                    "name": "libssl3",
                    "SPDXID": "SPDXRef-Package-deb-libssl3",
                    "versionInfo": "3.0.15-1",
                    "licenseConcluded": "NOASSERTION",
                    "licenseDeclared": "OpenSSL",
                    "externalRefs": [{
                        "referenceCategory": "PACKAGE-MANAGER",
                        "referenceType": "purl",
                        "referenceLocator": "pkg:deb/debian/libssl3@3.0.15-1?arch=amd64&distro=debian-12"
                    }]
                },
                {
                    "name": "readline",
                    "versionInfo": "8.2",
                    "licenseConcluded": "NOASSERTION",
                    "licenseDeclared": "GPL-3.0-or-later",
                    "externalRefs": [{
                        "referenceCategory": "PACKAGE_MANAGER",
                        "referenceType": "purl",
                        "referenceLocator": "pkg:deb/debian/readline@8.2"
                    }]
                },
                {
                    "name": "lodash",
                    "versionInfo": "4.17.21",
                    "licenseConcluded": "MIT",
                    "licenseDeclared": "NOASSERTION",
                    "externalRefs": [{
                        "referenceCategory": "PACKAGE-MANAGER",
                        "referenceType": "purl",
                        "referenceLocator": "pkg:npm/lodash@4.17.21"
                    }]
                }
            ]
        })
    }

    fn cyclonedx_fixture() -> JsonValue {
        json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "metadata": { "component": { "name": "nginx", "type": "container" } },
            "components": [
                {
                    "type": "library",
                    "name": "core",
                    "group": "@babel",
                    "version": "7.24.0",
                    "purl": "pkg:npm/%40babel/core@7.24.0",
                    "licenses": [{ "license": { "id": "MIT" } }]
                },
                {
                    "type": "library",
                    "name": "jackson-databind",
                    "group": "com.fasterxml.jackson.core",
                    "version": "2.17.0",
                    "purl": "pkg:maven/com.fasterxml.jackson.core/jackson-databind@2.17.0",
                    "licenses": [{ "expression": "Apache-2.0 OR LGPL-2.1-only" }]
                },
                {
                    "type": "library",
                    "name": "mystery",
                    "version": "1.0.0"
                }
            ]
        })
    }

    #[test]
    fn test_detects_both_formats() {
        assert_eq!(detect_sbom_type_in(&spdx_fixture()), Some(SbomType::Spdx));
        assert_eq!(
            detect_sbom_type_in(&cyclonedx_fixture()),
            Some(SbomType::CycloneDx)
        );
        assert_eq!(detect_sbom_type_in(&json!({ "packages": [] })), None);
    }

    #[test]
    fn test_extract_spdx_identity_and_licenses() {
        let (components, origins) = extract_spdx(&spdx_fixture());
        assert_eq!(components.len(), 3);
        assert_eq!(origins.len(), 3);

        // The PURL, not the package name, decides the ecosystem. An OS package keeps the distro
        // namespace and the qualifiers the document gave it, so the PURL it round-trips to is the
        // one that arrived.
        assert_eq!(components[0].ecosystem, Ecosystem::Deb);
        assert_eq!(components[0].name, "debian/libssl3");
        assert_eq!(components[0].version, "3.0.15-1");
        assert_eq!(
            components[0].purl().as_deref(),
            Some("pkg:deb/debian/libssl3@3.0.15-1?arch=amd64&distro=debian-12")
        );
        // NOASSERTION on the conclusion falls through to the declaration.
        assert_eq!(components[0].license.as_deref(), Some("OpenSSL"));

        assert_eq!(components[1].license.as_deref(), Some("GPL-3.0-or-later"));

        // A concluded license wins over the declaration.
        assert_eq!(components[2].ecosystem, Ecosystem::Npm);
        assert_eq!(components[2].license.as_deref(), Some("MIT"));
        assert_eq!(
            components[2].purl().as_deref(),
            Some("pkg:npm/lodash@4.17.21")
        );
    }

    #[test]
    fn test_extract_cyclonedx_identity_and_licenses() {
        let (components, origins) = extract_cyclonedx(&cyclonedx_fixture());
        assert_eq!(components.len(), 3);

        assert_eq!(components[0].ecosystem, Ecosystem::Npm);
        assert_eq!(components[0].name, "@babel/core");
        assert_eq!(components[0].license.as_deref(), Some("MIT"));

        assert_eq!(components[1].ecosystem, Ecosystem::Maven);
        assert_eq!(
            components[1].name,
            "com.fasterxml.jackson.core:jackson-databind"
        );
        assert_eq!(
            components[1].license.as_deref(),
            Some("Apache-2.0 OR LGPL-2.1-only")
        );

        // No PURL and no licenses: generic identity, nothing stated.
        assert_eq!(components[2].ecosystem, Ecosystem::Generic);
        assert_eq!(components[2].name, "mystery");
        assert!(components[2].license.is_none());

        // The metadata component is the artifact being described, not one of its dependencies.
        assert!(!components.iter().any(|info| info.name == "nginx"));
        assert_eq!(origins[2].position, 2);
    }

    #[test]
    fn test_unnamed_entries_are_skipped_without_shifting_positions() {
        let document = json!({
            "bomFormat": "CycloneDX",
            "components": [
                { "type": "library", "version": "1.0.0" },
                { "type": "library", "name": "serde", "version": "1.0.219", "purl": "pkg:cargo/serde@1.0.219" }
            ]
        });
        let (components, origins) = extract_cyclonedx(&document);
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "serde");
        // The surviving component still points at its own slot in the document.
        assert_eq!(origins[0].position, 1);
    }

    #[test]
    fn test_multiple_cyclonedx_licenses_join_conjunctively() {
        let component = json!({
            "name": "dual",
            "licenses": [
                { "license": { "id": "MIT" } },
                { "expression": "GPL-2.0-only OR Apache-2.0" },
                { "license": { "name": "MIT" } }
            ]
        });
        // The alternative keeps its grouping, and the repeated id is not stated twice.
        assert_eq!(
            cyclonedx_license(&component).as_deref(),
            Some("MIT AND (GPL-2.0-only OR Apache-2.0)")
        );
    }

    #[test]
    fn test_cyclonedx_license_name_fallback() {
        let component = json!({
            "name": "custom",
            "licenses": [{ "license": { "name": "Acme Commercial License" } }]
        });
        assert_eq!(
            cyclonedx_license(&component).as_deref(),
            Some("Acme Commercial License")
        );

        let noassertion = json!({
            "name": "empty",
            "licenses": [{ "license": { "id": "NOASSERTION" } }]
        });
        assert!(cyclonedx_license(&noassertion).is_none());
    }

    #[test]
    fn test_license_refs_expand_from_extracted_text() {
        let document = json!({
            "spdxVersion": "SPDX-2.3",
            "hasExtractedLicensingInfos": [
                {
                    "licenseId": "LicenseRef-1",
                    "name": "MIT-ish",
                    "extractedText": "Permission is hereby granted, free of charge, to any person \
        obtaining a copy of this software and associated documentation files (the \"Software\"), to deal \
        in the Software without restriction, including without limitation the rights to use, copy, \
        modify, merge, publish, distribute, sublicense, and/or sell copies of the Software"
                },
                {
                    "licenseId": "LicenseRef-10",
                    "name": "Acme EULA"
                }
            ],
            "packages": [
                { "name": "a", "versionInfo": "1", "licenseDeclared": "LicenseRef-1" },
                { "name": "b", "versionInfo": "1", "licenseDeclared": "LicenseRef-10" },
                { "name": "c", "versionInfo": "1", "licenseDeclared": "MIT AND LicenseRef-10" }
            ]
        });

        let (components, _) = extract_spdx(&document);
        // Content detection turns the extracted text into a canonical SPDX id.
        assert_eq!(components[0].license.as_deref(), Some("MIT"));
        // No text to detect, so the entry's own name stands in.
        assert_eq!(components[1].license.as_deref(), Some("Acme EULA"));
        // The longer ref is substituted first, so it is not truncated by the shorter one.
        assert_eq!(components[2].license.as_deref(), Some("MIT AND Acme EULA"));
    }

    #[test]
    fn test_missing_component_array_is_not_an_error() {
        let (components, origins) = extract_spdx(&json!({ "spdxVersion": "SPDX-2.3" }));
        assert!(components.is_empty());
        assert!(origins.is_empty());
    }

    #[test]
    fn test_is_stated() {
        assert!(is_stated("MIT"));
        assert!(!is_stated("NOASSERTION"));
        assert!(!is_stated("noassertion"));
        assert!(!is_stated("NONE"));
        assert!(!is_stated("   "));
    }

    #[test]
    fn test_classify_marks_restrictive_components() {
        let (mut components, _) = extract_spdx(&spdx_fixture());
        classify_findings(&mut components, false);

        let readline = components
            .iter()
            .find(|info| info.name == "debian/readline")
            .expect("readline should be present");
        assert!(readline.is_restrictive);

        let lodash = components
            .iter()
            .find(|info| info.name == "lodash")
            .expect("lodash should be present");
        assert!(!lodash.is_restrictive);
        assert_eq!(lodash.osi_status, OsiStatus::Approved);
    }

    #[test]
    fn test_enriched_output_only_touches_resolved_components() {
        let document = spdx_fixture();
        let (mut components, mut origins) = extract_spdx(&document);
        // Stand in for a resolver that answered for the package the document left unstated.
        components[0].license = Some("OpenSSL".to_string());
        origins[0].resolved = true;

        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("enriched.spdx.json");
        write_json(
            &document,
            SbomType::Spdx,
            &components,
            &origins,
            output.to_str().unwrap(),
        )
        .unwrap();

        let written: JsonValue =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        let packages = written["packages"].as_array().unwrap();
        assert_eq!(packages[0]["licenseConcluded"], "OpenSSL");
        // Untouched packages keep the document's own values, NOASSERTION included.
        assert_eq!(packages[1]["licenseConcluded"], "NOASSERTION");
        assert_eq!(packages[2]["licenseConcluded"], "MIT");
    }

    #[test]
    fn test_enriched_cyclonedx_writes_licenses_array() {
        let document = cyclonedx_fixture();
        let (mut components, mut origins) = extract_cyclonedx(&document);
        components[2].license = Some("MIT OR Apache-2.0".to_string());
        origins[2].resolved = true;

        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("enriched.cdx.json");
        write_json(
            &document,
            SbomType::CycloneDx,
            &components,
            &origins,
            output.to_str().unwrap(),
        )
        .unwrap();

        let written: JsonValue =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        let components = written["components"].as_array().unwrap();
        // A compound license is an expression, not an id.
        assert_eq!(
            components[2]["licenses"][0]["expression"],
            "MIT OR Apache-2.0"
        );
        assert_eq!(components[0]["licenses"][0]["license"]["id"], "MIT");
    }

    #[test]
    fn test_free_form_licenses_become_license_refs() {
        // Maven Central answers with titles, not SPDX ids, and neither format accepts one where
        // an id is expected.
        let mut refs = LicenseRefs::default();
        assert_eq!(
            spdx_license_field("The Apache Software License, Version 2.0", &mut refs),
            "LicenseRef-feluda-The-Apache-Software-License-Version-2.0"
        );
        assert_eq!(spdx_license_field("MIT", &mut refs), "MIT");
        assert_eq!(
            spdx_license_field("MIT OR Apache-2.0", &mut refs),
            "MIT OR Apache-2.0"
        );
        // "or" inside a title does not make it an expression.
        assert_eq!(
            spdx_license_field("Apache or MIT style license", &mut refs),
            "LicenseRef-feluda-Apache-or-MIT-style-license"
        );
        assert_eq!(refs.into_defined().len(), 2);
    }

    #[test]
    fn test_enriched_spdx_defines_the_refs_it_uses() {
        let document = spdx_fixture();
        let (mut components, mut origins) = extract_spdx(&document);
        components[0].license = Some("The Apache Software License, Version 2.0".to_string());
        origins[0].resolved = true;

        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("enriched.spdx.json");
        write_json(
            &document,
            SbomType::Spdx,
            &components,
            &origins,
            output.to_str().unwrap(),
        )
        .unwrap();

        let written: JsonValue =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        let license_ref = "LicenseRef-feluda-The-Apache-Software-License-Version-2.0";
        assert_eq!(written["packages"][0]["licenseConcluded"], license_ref);

        let extracted = written["hasExtractedLicensingInfos"].as_array().unwrap();
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0]["licenseId"], license_ref);
        assert_eq!(
            extracted[0]["extractedText"],
            "The Apache Software License, Version 2.0"
        );
    }

    #[test]
    fn test_enriched_cyclonedx_writes_only_listed_ids_as_ids() {
        let document = cyclonedx_fixture();
        let (mut components, mut origins) = extract_cyclonedx(&document);
        // Id shaped, but not on the SPDX list.
        components[2].license = Some("Custom-1.0".to_string());
        origins[2].resolved = true;
        components[1].license = Some("apache-2.0".to_string());
        origins[1].resolved = true;

        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("enriched.cdx.json");
        write_json(
            &document,
            SbomType::CycloneDx,
            &components,
            &origins,
            output.to_str().unwrap(),
        )
        .unwrap();

        let written: JsonValue =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        let components = written["components"].as_array().unwrap();
        assert_eq!(
            components[2]["licenses"][0],
            json!({ "license": { "name": "Custom-1.0" } })
        );
        assert_eq!(
            components[1]["licenses"][0],
            json!({ "license": { "id": "Apache-2.0" } })
        );
    }

    #[test]
    fn test_enriched_cyclonedx_1_6_marks_resolved_licenses_concluded() {
        let temp = tempfile::tempdir().unwrap();
        for (version, expected) in [("1.5", None), ("1.6", Some("concluded"))] {
            let mut document = cyclonedx_fixture();
            document["specVersion"] = json!(version);
            let (mut components, mut origins) = extract_cyclonedx(&document);
            components[2].license = Some("MIT".to_string());
            origins[2].resolved = true;

            let output = temp.path().join(format!("enriched-{version}.cdx.json"));
            write_json(
                &document,
                SbomType::CycloneDx,
                &components,
                &origins,
                output.to_str().unwrap(),
            )
            .unwrap();

            let written: JsonValue =
                serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
            let license = &written["components"][2]["licenses"][0]["license"];
            assert_eq!(license["id"], "MIT");
            assert_eq!(
                license.get("acknowledgement").and_then(|v| v.as_str()),
                expected,
                "CycloneDX {version}"
            );
        }
    }

    #[test]
    fn test_enriched_spdx_refs_do_not_collide_with_the_documents_own() {
        let mut document = spdx_fixture();
        // A ref an earlier run defined, for a different text that folds to the same id.
        document["hasExtractedLicensingInfos"] = json!([{
            "licenseId": "LicenseRef-feluda-Acme-License",
            "extractedText": "Acme License"
        }]);
        let (mut components, mut origins) = extract_spdx(&document);
        components[0].license = Some("Acme, License".to_string());
        origins[0].resolved = true;
        components[1].license = Some("Acme License".to_string());
        origins[1].resolved = true;

        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("enriched.spdx.json");
        write_json(
            &document,
            SbomType::Spdx,
            &components,
            &origins,
            output.to_str().unwrap(),
        )
        .unwrap();

        let written: JsonValue =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        // The same text reuses the existing ref; a different one gets its own.
        assert_eq!(
            written["packages"][1]["licenseConcluded"],
            "LicenseRef-feluda-Acme-License"
        );
        let other = written["packages"][0]["licenseConcluded"].as_str().unwrap();
        assert!(
            other.starts_with("LicenseRef-feluda-Acme-License-"),
            "{other}"
        );

        let ids: Vec<&str> = written["hasExtractedLicensingInfos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|info| info["licenseId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["LicenseRef-feluda-Acme-License", other]);
    }

    #[test]
    fn test_enriched_cyclonedx_uses_name_for_free_form_licenses() {
        let document = cyclonedx_fixture();
        let (mut components, mut origins) = extract_cyclonedx(&document);
        components[2].license = Some("Acme Commercial License".to_string());
        origins[2].resolved = true;

        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("enriched.cdx.json");
        write_json(
            &document,
            SbomType::CycloneDx,
            &components,
            &origins,
            output.to_str().unwrap(),
        )
        .unwrap();

        let written: JsonValue =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        assert_eq!(
            written["components"][2]["licenses"][0]["license"]["name"],
            "Acme Commercial License"
        );
    }

    #[test]
    fn test_ingest_from_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("sbom.json");
        fs::write(&path, spdx_fixture().to_string()).unwrap();

        let components = ingest_sbom(path.to_str().unwrap(), false, None).unwrap();
        assert_eq!(components.len(), 3);
        assert!(components.iter().any(|info| info.is_restrictive));
    }

    #[test]
    fn test_ingest_rejects_unknown_documents() {
        let temp = tempfile::tempdir().unwrap();

        let not_json = temp.path().join("not.json");
        fs::write(&not_json, "this is not json").unwrap();
        assert!(ingest_sbom(not_json.to_str().unwrap(), false, None).is_err());

        let unknown = temp.path().join("unknown.json");
        fs::write(&unknown, r#"{"dependencies": []}"#).unwrap();
        assert!(ingest_sbom(unknown.to_str().unwrap(), false, None).is_err());

        assert!(ingest_sbom("/nonexistent/sbom.json", false, None).is_err());
    }
}
