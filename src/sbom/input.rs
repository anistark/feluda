//! Reading an SBOM in any serialization feluda understands.
//!
//! Five are read: SPDX 2.x as JSON or tag:value, SPDX 3.0 as JSON-LD, and CycloneDX as JSON or
//! XML. Each is mapped onto the JSON shape of its family (SPDX 2.x JSON or CycloneDX JSON), which
//! is what ingest extracts components from and what the 2.x and CycloneDX validators check. The
//! original stays alongside it, since an enriched copy is written back in the serialization it
//! arrived in.

use serde_json::Value as JsonValue;

use super::{cyclonedx_xml, detect_sbom_type_in, spdx3, tagvalue, SbomType};

/// How a document was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Serialization {
    SpdxJson,
    SpdxTagValue,
    Spdx3JsonLd,
    CycloneDxJson,
    CycloneDxXml,
}

impl Serialization {
    /// The family whose JSON shape the document is read as.
    pub fn sbom_type(self) -> SbomType {
        match self {
            Serialization::SpdxJson | Serialization::SpdxTagValue | Serialization::Spdx3JsonLd => {
                SbomType::Spdx
            }
            Serialization::CycloneDxJson | Serialization::CycloneDxXml => SbomType::CycloneDx,
        }
    }

    /// How the serialization is named in messages and reports.
    pub fn describe(self) -> &'static str {
        match self {
            Serialization::SpdxJson => "SPDX JSON",
            Serialization::SpdxTagValue => "SPDX tag:value",
            Serialization::Spdx3JsonLd => "SPDX 3.0 JSON-LD",
            Serialization::CycloneDxJson => "CycloneDX JSON",
            Serialization::CycloneDxXml => "CycloneDX XML",
        }
    }
}

/// What an enriched copy needs from the original besides its JSON shape.
#[derive(Debug, Clone)]
pub enum Original {
    /// SPDX or CycloneDX JSON: the model is the document.
    Json,
    /// The text, and where each package sits in it.
    TagValue {
        text: String,
        packages: Vec<tagvalue::PackageLines>,
    },
    /// The text, spliced rather than rewritten.
    Xml(String),
    /// The graph as it arrived; the model only holds what ingest reads.
    Spdx3(JsonValue),
}

/// A document read from any supported serialization.
#[derive(Debug, Clone)]
pub struct SbomDocument {
    pub serialization: Serialization,
    /// The document in its family's JSON shape.
    pub model: JsonValue,
    pub original: Original,
}

/// Read a document, working out its serialization from its content.
pub fn read_sbom(content: &str) -> Result<SbomDocument, String> {
    let trimmed = content.trim_start_matches('\u{feff}').trim_start();

    if trimmed.starts_with('<') {
        if !cyclonedx_xml::looks_like_cyclonedx_xml(trimmed) {
            return Err(format!(
                "{} XML is only read as CycloneDX.",
                SbomType::DETECTION_FAILURE
            ));
        }
        return Ok(SbomDocument {
            serialization: Serialization::CycloneDxXml,
            model: cyclonedx_xml::parse(trimmed)
                .map_err(|e| format!("Invalid CycloneDX XML: {e}"))?,
            original: Original::Xml(trimmed.to_string()),
        });
    }

    if tagvalue::looks_like_tag_value(trimmed) {
        let parsed =
            tagvalue::parse(trimmed).map_err(|e| format!("Invalid SPDX tag:value: {e}"))?;
        return Ok(SbomDocument {
            serialization: Serialization::SpdxTagValue,
            model: parsed.json,
            original: Original::TagValue {
                text: trimmed.to_string(),
                packages: parsed.packages,
            },
        });
    }

    let json: JsonValue =
        serde_json::from_str(trimmed).map_err(|e| format!("Invalid JSON in SBOM: {e}"))?;
    if spdx3::is_spdx3(&json) {
        return Ok(SbomDocument {
            serialization: Serialization::Spdx3JsonLd,
            model: spdx3::normalize(&json),
            original: Original::Spdx3(json),
        });
    }
    let serialization = match detect_sbom_type_in(&json) {
        Some(SbomType::Spdx) => Serialization::SpdxJson,
        Some(SbomType::CycloneDx) => Serialization::CycloneDxJson,
        None => return Err(SbomType::DETECTION_FAILURE.to_string()),
    };
    Ok(SbomDocument {
        serialization,
        model: json,
        original: Original::Json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_each_serialization_is_recognised() {
        let read = |content: &str| read_sbom(content).unwrap().serialization;
        assert_eq!(
            read(r#"{"spdxVersion": "SPDX-2.3", "packages": []}"#),
            Serialization::SpdxJson
        );
        assert_eq!(
            read(r#"{"bomFormat": "CycloneDX", "specVersion": "1.6"}"#),
            Serialization::CycloneDxJson
        );
        assert_eq!(
            read(&format!(
                r#"{{"@context": "{}", "@graph": []}}"#,
                spdx3::CONTEXT
            )),
            Serialization::Spdx3JsonLd
        );
        assert_eq!(
            read("\u{feff}SPDXVersion: SPDX-2.3\nDataLicense: CC0-1.0\n"),
            Serialization::SpdxTagValue
        );
        assert_eq!(
            read(
                r#"<?xml version="1.0"?><bom xmlns="http://cyclonedx.org/schema/bom/1.5" version="1"/>"#
            ),
            Serialization::CycloneDxXml
        );
    }

    #[test]
    fn test_unknown_documents_are_refused_with_a_reason() {
        let error = read_sbom(r#"{"dependencies": []}"#).unwrap_err();
        assert!(error.contains("neither SPDX nor CycloneDX"), "{error}");

        let error = read_sbom("<rdf:RDF xmlns:rdf=\"x\"/>").unwrap_err();
        assert!(error.contains("only read as CycloneDX"), "{error}");

        let error = read_sbom("this is not an SBOM").unwrap_err();
        assert!(error.contains("Invalid JSON"), "{error}");
    }
}
