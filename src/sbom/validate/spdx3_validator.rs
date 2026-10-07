//! Structural checks for SPDX 3.0 JSON-LD.
//!
//! What a reader needs to trust the graph: the context, a creation info every element can reach,
//! ids that are IRIs and unique, references that resolve, and license expressions over listed
//! licenses and the custom ids they map. Full SHACL validation is the official `spdx3-validate`'s
//! job.

use super::reporter::{ValidationIssue, ValidationReport};
use crate::debug::FeludaResult;
use crate::sbom::spdx3::{
    element_id, element_type, elements, is_relationship, is_spdx3, relationship_targets,
};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};

/// Element types identified by a blank node `@id` rather than an `spdxId`.
const NODE_TYPES: [&str; 6] = [
    "CreationInfo",
    "DictionaryEntry",
    "ExternalIdentifier",
    "ExternalRef",
    "ExternalMap",
    "Hash",
];

/// Ids SPDX itself defines, which a document refers to without defining.
fn is_well_known(id: &str) -> bool {
    id.starts_with("https://spdx.org/licenses/")
        || id.starts_with("https://spdx.org/rdf/3.")
        || matches!(
            id,
            "expandedlicensing_NoAssertionLicense"
                | "expandedlicensing_NoneLicense"
                | "NoAssertionElement"
                | "NoneElement"
                | "SpdxOrganization"
        )
}

pub fn validate(json: &JsonValue) -> FeludaResult<ValidationReport> {
    let mut report = ValidationReport::new("SPDX 3.0");

    if !is_spdx3(json) {
        report.add_issue(ValidationIssue::error("Missing SPDX 3 @context").with_field("@context"));
        return Ok(report);
    }
    if json.get("@context").and_then(|c| c.as_str()) != Some(crate::sbom::spdx3::CONTEXT) {
        report.add_issue(
            ValidationIssue::info(format!(
                "@context is not {}; only SPDX 3.0.1 is checked",
                crate::sbom::spdx3::CONTEXT
            ))
            .with_field("@context"),
        );
    }

    let graph = elements(json);
    if graph.is_empty() {
        report.add_issue(
            ValidationIssue::error("Document has no @graph elements").with_field("@graph"),
        );
        return Ok(report);
    }

    // Every id the document defines, and the ids it imports from other documents.
    let mut defined: HashMap<&str, &JsonValue> = HashMap::new();
    for element in &graph {
        let Some(id) = element_id(element) else {
            continue;
        };
        if defined.insert(id, element).is_some() {
            report.add_issue(
                ValidationIssue::error(format!("Duplicate element id: {id}")).with_field("spdxId"),
            );
        }
    }
    let imported: HashSet<&str> = graph
        .iter()
        .filter(|element| element_type(element) == Some("SpdxDocument"))
        .filter_map(|document| document.get("import")?.as_array())
        .flatten()
        .filter_map(|map| map.get("externalSpdxId")?.as_str())
        .collect();
    let resolves =
        |id: &str| defined.contains_key(id) || imported.contains(id) || is_well_known(id);

    let documents = graph
        .iter()
        .filter(|element| element_type(element) == Some("SpdxDocument"))
        .count();
    if documents == 0 {
        report.add_issue(ValidationIssue::warning(
            "No SpdxDocument element; readers have no document to start from",
        ));
    }

    let mut creation_infos = 0;
    for (index, element) in graph.iter().enumerate() {
        let Some(kind) = element_type(element) else {
            report.add_issue(
                ValidationIssue::error(format!("Element[{index}] has no type")).with_field("type"),
            );
            continue;
        };
        let label = element_id(element)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{kind}[{index}]"));

        if kind == "CreationInfo" {
            creation_infos += 1;
            validate_creation_info(&mut report, element, &label, &resolves);
            continue;
        }
        if NODE_TYPES.contains(&kind) {
            continue;
        }

        match element.get("spdxId").and_then(|id| id.as_str()) {
            None => report.add_issue(
                ValidationIssue::error(format!("{kind} '{label}': missing spdxId"))
                    .with_field("spdxId"),
            ),
            Some(id) if id.starts_with("_:") || !id.contains(':') => report.add_issue(
                ValidationIssue::error(format!("{kind} '{id}': spdxId must be an IRI"))
                    .with_field("spdxId"),
            ),
            Some(_) => {}
        }

        match element.get("creationInfo") {
            None => report.add_issue(
                ValidationIssue::error(format!("{kind} '{label}': missing creationInfo"))
                    .with_field("creationInfo"),
            ),
            Some(JsonValue::String(info)) => {
                let is_creation_info = defined
                    .get(info.as_str())
                    .is_some_and(|target| element_type(target) == Some("CreationInfo"));
                if !is_creation_info {
                    report.add_issue(
                        ValidationIssue::error(format!(
                            "{kind} '{label}': creationInfo {info} is not a CreationInfo in this document"
                        ))
                        .with_field("creationInfo"),
                    );
                }
            }
            Some(_) => {}
        }

        if kind == "software_Package" && element.get("name").and_then(|n| n.as_str()).is_none() {
            report.add_issue(
                ValidationIssue::error(format!("Package '{label}': missing name"))
                    .with_field("name"),
            );
        }

        if is_relationship(element) {
            validate_relationship(&mut report, element, &label, &resolves);
        }

        if kind == "simplelicensing_LicenseExpression" {
            validate_expression(&mut report, element, &label);
        }

        for field in ["rootElement", "element"] {
            for id in element
                .get(field)
                .and_then(|ids| ids.as_array())
                .into_iter()
                .flatten()
                .filter_map(|id| id.as_str())
            {
                if !resolves(id) {
                    report.add_issue(
                        ValidationIssue::warning(format!(
                            "{kind} '{label}': {field} {id} is not defined in this document or imported"
                        ))
                        .with_field(field),
                    );
                }
            }
        }
    }

    if creation_infos == 0 {
        report.add_issue(ValidationIssue::error(
            "No CreationInfo; every element needs one",
        ));
    }

    Ok(report)
}

fn validate_creation_info(
    report: &mut ValidationReport,
    info: &JsonValue,
    label: &str,
    resolves: &dyn Fn(&str) -> bool,
) {
    match info.get("specVersion").and_then(|v| v.as_str()) {
        None => report.add_issue(
            ValidationIssue::error(format!("CreationInfo '{label}': missing specVersion"))
                .with_field("specVersion"),
        ),
        Some(version) if !version.starts_with("3.0") => report.add_issue(
            ValidationIssue::warning(format!(
                "CreationInfo '{label}': specVersion {version} is not 3.0"
            ))
            .with_field("specVersion"),
        ),
        Some(_) => {}
    }

    match info.get("created").and_then(|v| v.as_str()) {
        None => report.add_issue(
            ValidationIssue::error(format!("CreationInfo '{label}': missing created"))
                .with_field("created"),
        ),
        // SPDX 3 dates are whole seconds in UTC.
        Some(created)
            if chrono::NaiveDateTime::parse_from_str(created, "%Y-%m-%dT%H:%M:%SZ").is_err() =>
        {
            report.add_issue(
                ValidationIssue::error(format!(
                    "CreationInfo '{label}': created '{created}' must be YYYY-MM-DDThh:mm:ssZ"
                ))
                .with_field("created"),
            )
        }
        Some(_) => {}
    }

    let creators: Vec<&str> = info
        .get("createdBy")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .collect();
    if creators.is_empty() {
        report.add_issue(
            ValidationIssue::error(format!("CreationInfo '{label}': createdBy is empty"))
                .with_field("createdBy"),
        );
    }
    for creator in creators.into_iter().filter(|creator| !resolves(creator)) {
        report.add_issue(
            ValidationIssue::warning(format!(
                "CreationInfo '{label}': createdBy {creator} is not defined in this document"
            ))
            .with_field("createdBy"),
        );
    }
}

fn validate_relationship(
    report: &mut ValidationReport,
    relationship: &JsonValue,
    label: &str,
    resolves: &dyn Fn(&str) -> bool,
) {
    if relationship.get("relationshipType").is_none() {
        report.add_issue(
            ValidationIssue::error(format!("Relationship '{label}': missing relationshipType"))
                .with_field("relationshipType"),
        );
    }
    match relationship.get("from").and_then(|from| from.as_str()) {
        None => report.add_issue(
            ValidationIssue::error(format!("Relationship '{label}': missing from"))
                .with_field("from"),
        ),
        Some(from) if !resolves(from) => report.add_issue(
            ValidationIssue::warning(format!(
                "Relationship '{label}': from {from} is not defined in this document or imported"
            ))
            .with_field("from"),
        ),
        Some(_) => {}
    }

    let targets = relationship_targets(relationship);
    if targets.is_empty() {
        report.add_issue(
            ValidationIssue::error(format!("Relationship '{label}': missing to")).with_field("to"),
        );
    }
    for to in targets.iter().filter_map(|to| to.as_str()) {
        if !resolves(to) {
            report.add_issue(
                ValidationIssue::warning(format!(
                    "Relationship '{label}': to {to} is not defined in this document or imported"
                ))
                .with_field("to"),
            );
        }
    }
}

/// An expression holds listed licenses, and `LicenseRef-` ids its `customIdToUri` maps.
fn validate_expression(report: &mut ValidationReport, element: &JsonValue, label: &str) {
    let Some(expression) = element
        .get("simplelicensing_licenseExpression")
        .and_then(|v| v.as_str())
    else {
        report.add_issue(
            ValidationIssue::error(format!(
                "LicenseExpression '{label}': missing simplelicensing_licenseExpression"
            ))
            .with_field("simplelicensing_licenseExpression"),
        );
        return;
    };

    if crate::spdx::listed_expression(expression).is_none() {
        report.add_issue(
            ValidationIssue::warning(format!(
                "LicenseExpression '{label}': '{expression}' is not an SPDX license expression"
            ))
            .with_field("simplelicensing_licenseExpression"),
        );
        return;
    }

    let mapped: Vec<&str> = element
        .get("simplelicensing_customIdToUri")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("key")?.as_str())
        .collect();
    for id in expression
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|token| token.starts_with("LicenseRef-"))
    {
        if !mapped.contains(&id) {
            report.add_issue(
                ValidationIssue::warning(format!(
                    "LicenseExpression '{label}': {id} has no simplelicensing_customIdToUri entry"
                ))
                .with_field("simplelicensing_customIdToUri"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sbom::spdx::{SpdxDocument, SpdxPackage};
    use crate::sbom::spdx3::CONTEXT;
    use serde_json::json;

    fn messages(document: JsonValue) -> Vec<String> {
        validate(&document)
            .unwrap()
            .issues
            .into_iter()
            .map(|issue| issue.message)
            .collect()
    }

    #[test]
    fn test_feluda_output_is_clean() {
        let mut document = SpdxDocument::new("demo");
        document.add_package(
            SpdxPackage::new("serde", &document.document_namespace)
                .with_version("1.0.219")
                .with_purl("pkg:cargo/serde@1.0.219")
                .with_license("MIT OR Apache-2.0"),
        );
        let written = crate::sbom::spdx3::write(&document);
        assert_eq!(messages(written), Vec::<String>::new());
    }

    #[test]
    fn test_structural_problems_are_reported() {
        let found = messages(json!({
            "@context": CONTEXT,
            "@graph": [
                {
                    "type": "CreationInfo",
                    "@id": "_:c",
                    "specVersion": "3.0.1",
                    "created": "2026-10-07T10:00:00.123Z",
                    "createdBy": []
                },
                { "type": "software_Package", "spdxId": "urn:a", "creationInfo": "_:c" },
                { "type": "software_Package", "spdxId": "urn:a", "creationInfo": "_:c", "name": "dup" },
                { "type": "software_Package", "spdxId": "_:blank", "creationInfo": "_:missing", "name": "b" },
                {
                    "type": "simplelicensing_LicenseExpression",
                    "spdxId": "urn:l",
                    "creationInfo": "_:c",
                    "simplelicensing_licenseExpression": "MIT OR LicenseRef-x"
                },
                {
                    "type": "Relationship",
                    "spdxId": "urn:r",
                    "creationInfo": "_:c",
                    "from": "urn:a",
                    "relationshipType": "hasConcludedLicense",
                    "to": ["urn:nowhere"]
                }
            ]
        }));

        let expect = [
            "must be YYYY-MM-DDThh:mm:ssZ",
            "createdBy is empty",
            "Duplicate element id: urn:a",
            "Package 'urn:a': missing name",
            "spdxId must be an IRI",
            "creationInfo _:missing is not a CreationInfo",
            "LicenseRef-x has no simplelicensing_customIdToUri entry",
            "to urn:nowhere is not defined",
            "No SpdxDocument element",
        ];
        for expected in expect {
            assert!(
                found.iter().any(|message| message.contains(expected)),
                "missing '{expected}' in {found:?}"
            );
        }
        assert_eq!(found.len(), expect.len(), "{found:?}");
    }

    #[test]
    fn test_missing_context_stops_early() {
        let found = messages(json!({ "@graph": [] }));
        assert_eq!(found, ["Missing SPDX 3 @context"]);
    }
}
