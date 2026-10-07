//! SPDX 3.0: read, enriched and written.
//!
//! SPDX 3.0 is a new model rather than a version bump. A document is a JSON-LD `@graph` of
//! elements addressed by `spdxId`; packages are `software_Package`; and a license is not a field
//! on the package but a `Relationship` (`hasConcludedLicense`, `hasDeclaredLicense`) pointing at a
//! license element, usually a `simplelicensing_LicenseExpression`.
//!
//! Reading maps the graph onto the SPDX 2.x package shape, licenses followed through their
//! relationships and custom license ids expanded, so ingest classifies a 3.0 document with the
//! same code as a 2.x one. Each package keeps its `spdxId` as `SPDXID`, which is how an enriched
//! copy finds it again.
//!
//! Writing starts from the same prepared [`SpdxDocument`] the 2.x writers use: every license is
//! already a listed id, an expression over them, or a `LicenseRef-` with its text, and 3.0 states
//! those as expression elements with the refs mapped to `simplelicensing_SimpleLicensingText`.

use chrono::{SecondsFormat, Utc};
use serde_json::{json, Map, Value as JsonValue};
use std::collections::HashMap;

use super::spdx::{ExtractedLicensingInfo, SpdxDocument};
use crate::licenses::detect_license_from_content;

/// The context every SPDX 3.0.1 JSON-LD document names.
pub const CONTEXT: &str = "https://spdx.org/rdf/3.0.1/spdx-context.jsonld";

/// The SPDX version feluda writes for 3.0.
pub const SPEC_VERSION: &str = "3.0.1";

/// Where listed licenses and exceptions live, as IRIs.
const LISTED_LICENSE_BASE: &str = "https://spdx.org/licenses/";

/// The element types that are packages: `software_Package` and its subclasses.
const PACKAGE_TYPES: [&str; 3] = ["software_Package", "ai_AIPackage", "dataset_DatasetPackage"];

/// How deep a license element may nest before it is treated as a cycle.
const MAX_LICENSE_DEPTH: usize = 16;

/// Whether a parsed JSON document is SPDX 3: its `@context` names an SPDX 3 context.
pub fn is_spdx3(json: &JsonValue) -> bool {
    let names_spdx3 = |value: &JsonValue| {
        value
            .as_str()
            .is_some_and(|context| context.contains("spdx.org/rdf/3."))
    };
    match json.get("@context") {
        Some(JsonValue::Array(contexts)) => contexts.iter().any(names_spdx3),
        Some(context) => names_spdx3(context),
        None => false,
    }
}

/// The elements of a document: its `@graph`, or the document itself when it is one element.
pub fn elements(json: &JsonValue) -> Vec<&JsonValue> {
    match json.get("@graph").and_then(|graph| graph.as_array()) {
        Some(graph) => graph.iter().collect(),
        None if element_type(json).is_some() => vec![json],
        None => Vec::new(),
    }
}

/// An element's id. The SPDX context aliases `spdxId` to `@id`, so either spelling is one.
pub fn element_id(element: &JsonValue) -> Option<&str> {
    element
        .get("spdxId")
        .or_else(|| element.get("@id"))
        .and_then(|id| id.as_str())
}

/// An element's type, `type` being the context's alias for `@type`.
pub fn element_type(element: &JsonValue) -> Option<&str> {
    element
        .get("type")
        .or_else(|| element.get("@type"))
        .and_then(|kind| kind.as_str())
}

/// The relationship types that carry a license.
pub fn is_relationship(element: &JsonValue) -> bool {
    matches!(
        element_type(element),
        Some("Relationship" | "LifecycleScopedRelationship")
    )
}

/// The ids a relationship points to: `to` is a list, but a lone value is read too.
pub fn relationship_targets(relationship: &JsonValue) -> Vec<&JsonValue> {
    match relationship.get("to") {
        Some(JsonValue::Array(targets)) => targets.iter().collect(),
        Some(target) => vec![target],
        None => Vec::new(),
    }
}

/// The individuals SPDX 3 defines for "no assertion" and "none", compacted or as IRIs.
fn is_no_assertion(reference: &str) -> bool {
    reference.ends_with("NoAssertionLicense") || reference.ends_with("NoAssertionElement")
}

fn is_none_license(reference: &str) -> bool {
    reference.ends_with("NoneLicense") || reference.ends_with("NoneElement")
}

/// Map an SPDX 3 document onto the SPDX 2.x JSON shape ingest reads.
pub fn normalize(json: &JsonValue) -> JsonValue {
    let elements = elements(json);
    let by_id: HashMap<&str, &JsonValue> = elements
        .iter()
        .filter_map(|element| Some((element_id(element)?, *element)))
        .collect();

    // Licenses hang off relationships, so gather them per package first.
    let mut concluded: HashMap<&str, Vec<String>> = HashMap::new();
    let mut declared: HashMap<&str, Vec<String>> = HashMap::new();
    for relationship in elements.iter().filter(|element| is_relationship(element)) {
        let Some(from) = relationship.get("from").and_then(|from| from.as_str()) else {
            continue;
        };
        let target = match relationship
            .get("relationshipType")
            .and_then(|kind| kind.as_str())
        {
            Some("hasConcludedLicense") => &mut concluded,
            Some("hasDeclaredLicense") => &mut declared,
            _ => continue,
        };
        for license in relationship_targets(relationship)
            .into_iter()
            .filter_map(|to| license_of(to, &by_id, 0))
        {
            let licenses = target.entry(from).or_default();
            if !licenses.contains(&license) {
                licenses.push(license);
            }
        }
    }

    let packages: Vec<JsonValue> = elements
        .iter()
        .filter(|element| element_type(element).is_some_and(|kind| PACKAGE_TYPES.contains(&kind)))
        .map(|package| {
            let id = element_id(package).unwrap_or_default();
            let mut entry = Map::new();
            entry.insert("SPDXID".to_string(), json!(id));
            for (from, to) in [
                ("name", "name"),
                ("software_packageVersion", "versionInfo"),
                ("software_downloadLocation", "downloadLocation"),
            ] {
                if let Some(value) = package.get(from) {
                    entry.insert(to.to_string(), value.clone());
                }
            }
            if let Some(purl) = package_url(package) {
                entry.insert(
                    "externalRefs".to_string(),
                    json!([{
                        "referenceCategory": "PACKAGE-MANAGER",
                        "referenceType": "purl",
                        "referenceLocator": purl,
                    }]),
                );
            }
            for (field, licenses) in [
                ("licenseConcluded", concluded.get(id)),
                ("licenseDeclared", declared.get(id)),
            ] {
                if let Some(license) = licenses.and_then(|licenses| conjunction(licenses)) {
                    entry.insert(field.to_string(), json!(license));
                }
            }
            JsonValue::Object(entry)
        })
        .collect();

    let spec_version = elements
        .iter()
        .find(|element| element_type(element) == Some("CreationInfo"))
        .and_then(|info| info.get("specVersion"))
        .and_then(|version| version.as_str())
        .unwrap_or("3.0");
    let mut document = Map::new();
    document.insert(
        "spdxVersion".to_string(),
        json!(format!("SPDX-{spec_version}")),
    );
    if let Some(name) = elements
        .iter()
        .find(|element| element_type(element) == Some("SpdxDocument"))
        .and_then(|document| document.get("name"))
    {
        document.insert("name".to_string(), name.clone());
    }
    document.insert("packages".to_string(), JsonValue::Array(packages));
    JsonValue::Object(document)
}

/// A package's PURL: `software_packageUrl`, or a `packageUrl` external identifier.
fn package_url(package: &JsonValue) -> Option<String> {
    if let Some(purl) = package.get("software_packageUrl").and_then(|v| v.as_str()) {
        return Some(purl.to_string());
    }
    package
        .get("externalIdentifier")?
        .as_array()?
        .iter()
        .find(|identifier| {
            identifier
                .get("externalIdentifierType")
                .and_then(|kind| kind.as_str())
                == Some("packageUrl")
        })
        .and_then(|identifier| identifier.get("identifier"))
        .and_then(|purl| purl.as_str())
        .map(str::to_string)
}

/// Several licenses on one package, all of which apply.
fn conjunction(licenses: &[String]) -> Option<String> {
    match licenses {
        [] => None,
        [license] => Some(license.clone()),
        _ => Some(
            licenses
                .iter()
                .map(|license| grouped(license))
                .collect::<Vec<_>>()
                .join(" AND "),
        ),
    }
}

/// A license as an operand: a compound one keeps its own grouping.
fn grouped(license: &str) -> String {
    if crate::spdx::is_compound(license) {
        format!("({license})")
    } else {
        license.to_string()
    }
}

/// The license a reference stands for, or `None` when it says nothing usable.
///
/// A reference is an element id, an inline element, a listed license's IRI, or one of the
/// individuals for "no assertion" and "none". An id the document does not define is an element
/// from another document, which feluda cannot see, so it says nothing.
fn license_of(
    reference: &JsonValue,
    by_id: &HashMap<&str, &JsonValue>,
    depth: usize,
) -> Option<String> {
    if depth > MAX_LICENSE_DEPTH {
        return None;
    }
    let element = match reference {
        JsonValue::String(id) => {
            if is_no_assertion(id) || is_none_license(id) {
                return None;
            }
            if let Some(listed) = id.strip_prefix(LISTED_LICENSE_BASE) {
                return Some(listed.to_string());
            }
            *by_id.get(id.as_str())?
        }
        JsonValue::Object(_) => reference,
        _ => return None,
    };

    let field = |name: &str| {
        element
            .get(name)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let nested = |name: &str| {
        element
            .get(name)
            .and_then(|value| license_of(value, by_id, depth + 1))
    };
    let set = |operator: &str| {
        let members: Vec<String> = element
            .get("expandedlicensing_member")?
            .as_array()?
            .iter()
            .filter_map(|member| license_of(member, by_id, depth + 1))
            .collect();
        match members.len() {
            0 => None,
            1 => members.into_iter().next(),
            _ => Some(
                members
                    .iter()
                    .map(|member| grouped(member))
                    .collect::<Vec<_>>()
                    .join(&format!(" {operator} ")),
            ),
        }
    };

    match element_type(element)? {
        "simplelicensing_LicenseExpression" => {
            let expression = field("simplelicensing_licenseExpression")?;
            let custom: HashMap<String, String> = element
                .get("simplelicensing_customIdToUri")
                .and_then(|entries| entries.as_array())
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    let key = entry.get("key")?.as_str()?;
                    let value = entry.get("value")?;
                    Some((key.to_string(), license_of(value, by_id, depth + 1)?))
                })
                .collect();
            Some(super::ingest::expand_license_refs(expression, &custom))
        }
        "expandedlicensing_ListedLicense" | "expandedlicensing_ListedLicenseException" => {
            element_id(element)
                .and_then(|id| id.strip_prefix(LISTED_LICENSE_BASE))
                .map(str::to_string)
                .or_else(|| field("name").map(str::to_string))
        }
        // The text is the license itself, which the content matcher reads; failing that, the
        // element's name is better than an opaque id.
        "simplelicensing_SimpleLicensingText"
        | "expandedlicensing_CustomLicense"
        | "expandedlicensing_CustomLicenseAddition" => field("simplelicensing_licenseText")
            .or_else(|| field("expandedlicensing_additionText"))
            .and_then(detect_license_from_content)
            .or_else(|| field("name").map(str::to_string)),
        "expandedlicensing_ConjunctiveLicenseSet" => set("AND"),
        "expandedlicensing_DisjunctiveLicenseSet" => set("OR"),
        "expandedlicensing_OrLaterOperator" => {
            nested("expandedlicensing_subjectLicense").map(|license| format!("{license}+"))
        }
        "expandedlicensing_WithAdditionOperator" => {
            let license = nested("expandedlicensing_subjectExtendableLicense")?;
            match nested("expandedlicensing_subjectAddition") {
                Some(addition) => Some(format!("{} WITH {addition}", grouped(&license))),
                None => Some(license),
            }
        }
        _ => None,
    }
}

// =============================================================================
// LICENSE ELEMENTS
// =============================================================================

/// Builds the elements that state licenses: one expression element per distinct license, and one
/// `simplelicensing_SimpleLicensingText` per `LicenseRef-` they use.
struct LicenseElements<'a> {
    base: &'a str,
    creation_info: &'a str,
    refs: &'a [ExtractedLicensingInfo],
    expressions: HashMap<String, String>,
    texts: HashMap<String, String>,
    elements: Vec<JsonValue>,
}

impl<'a> LicenseElements<'a> {
    fn new(base: &'a str, creation_info: &'a str, refs: &'a [ExtractedLicensingInfo]) -> Self {
        Self {
            base,
            creation_info,
            refs,
            expressions: HashMap::new(),
            texts: HashMap::new(),
            elements: Vec::new(),
        }
    }

    /// What a license relationship points to for `license`, or `None` for `NOASSERTION`, which
    /// SPDX 3 states by having no relationship at all.
    fn target(&mut self, license: &str) -> Option<String> {
        let license = license.trim();
        if license.is_empty() || license.eq_ignore_ascii_case("NOASSERTION") {
            return None;
        }
        if license.eq_ignore_ascii_case("NONE") {
            return Some("expandedlicensing_NoneLicense".to_string());
        }
        if let Some(id) = self.expressions.get(license) {
            return Some(id.clone());
        }

        let id = format!(
            "{}#LicenseExpression-{}",
            self.base,
            self.expressions.len() + 1
        );
        let mut element = json!({
            "type": "simplelicensing_LicenseExpression",
            "spdxId": id,
            "creationInfo": self.creation_info,
            "simplelicensing_licenseExpression": license,
        });
        let custom: Vec<JsonValue> = license
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
            .filter(|token| token.starts_with("LicenseRef-"))
            .filter_map(|license_ref| {
                let text_id = self.text(license_ref)?;
                Some(json!({ "type": "DictionaryEntry", "key": license_ref, "value": text_id }))
            })
            .collect();
        if !custom.is_empty() {
            element["simplelicensing_customIdToUri"] = JsonValue::Array(custom);
        }
        self.elements.push(element);
        self.expressions.insert(license.to_string(), id.clone());
        Some(id)
    }

    /// The element holding a `LicenseRef-`'s text, when the refs define it.
    fn text(&mut self, license_ref: &str) -> Option<String> {
        if let Some(id) = self.texts.get(license_ref) {
            return Some(id.clone());
        }
        let info = self
            .refs
            .iter()
            .find(|info| info.license_id == license_ref)?;
        let id = format!("{}#{}", self.base, info.license_id);
        let mut element = json!({
            "type": "simplelicensing_SimpleLicensingText",
            "spdxId": id,
            "creationInfo": self.creation_info,
            "simplelicensing_licenseText": info.extracted_text,
        });
        if let Some(name) = &info.name {
            element["name"] = json!(name);
        }
        if let Some(comment) = &info.comment {
            element["comment"] = json!(comment);
        }
        self.elements.push(element);
        self.texts.insert(license_ref.to_string(), id.clone());
        Some(id)
    }
}

/// The agent and tool every feluda creation info names, as elements.
fn feluda_agents(base: &str, creation_info: &str) -> (String, String, Vec<JsonValue>) {
    let agent = format!("{base}#Feluda");
    let tool = format!("{base}#Feluda-{}", env!("CARGO_PKG_VERSION"));
    let elements = vec![
        json!({
            "type": "SoftwareAgent",
            "spdxId": agent,
            "creationInfo": creation_info,
            "name": "Feluda",
        }),
        json!({
            "type": "Tool",
            "spdxId": tool,
            "creationInfo": creation_info,
            "name": format!("Feluda {}", env!("CARGO_PKG_VERSION")),
        }),
    ];
    (agent, tool, elements)
}

fn creation_info(
    id: &str,
    spec_version: &str,
    created: &str,
    agent: &str,
    tool: &str,
) -> JsonValue {
    json!({
        "type": "CreationInfo",
        "@id": id,
        "specVersion": spec_version,
        "created": created,
        "createdBy": [agent],
        "createdUsing": [tool],
    })
}

fn relationship(id: String, creation_info: &str, from: &str, kind: &str, to: &str) -> JsonValue {
    json!({
        "type": "Relationship",
        "spdxId": id,
        "creationInfo": creation_info,
        "from": from,
        "relationshipType": kind,
        "to": [to],
    })
}

// =============================================================================
// ENRICHED COPY
// =============================================================================

/// The original document with feluda's conclusions added.
///
/// `concluded` pairs a package's `spdxId` with the license to state, already in the form SPDX
/// accepts; `refs` defines the `LicenseRef-` ids those use. Everything feluda adds carries its
/// own creation info, so the document still says who stated what. A package whose conclusion
/// was an explicit "no assertion" has that relationship pointed at the new license, since two
/// conclusions would contradict each other.
pub fn enrich(
    original: &JsonValue,
    concluded: &[(String, String)],
    refs: &[ExtractedLicensingInfo],
) -> JsonValue {
    let mut document = match original.get("@graph") {
        Some(_) => original.clone(),
        // A one element document becomes a graph, so there is somewhere to add to.
        None => {
            let mut element = original.clone();
            let context = element
                .as_object_mut()
                .and_then(|object| object.remove("@context"))
                .unwrap_or_else(|| json!(CONTEXT));
            json!({ "@context": context, "@graph": [element] })
        }
    };

    let base = format!("https://anirudha.dev/feluda/spdx/{}", uuid::Uuid::new_v4());
    let info_id = "_:feluda-creationinfo";
    let spec_version = elements(&document)
        .iter()
        .find(|element| element_type(element) == Some("CreationInfo"))
        .and_then(|info| info.get("specVersion"))
        .and_then(|version| version.as_str())
        .unwrap_or(SPEC_VERSION)
        .to_string();
    let (agent, tool, mut added) = feluda_agents(&base, info_id);
    let created = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    added.insert(
        0,
        creation_info(info_id, &spec_version, &created, &agent, &tool),
    );

    let mut licenses = LicenseElements::new(&base, info_id, refs);
    let mut new_relationships = Vec::new();
    let Some(graph) = document
        .get_mut("@graph")
        .and_then(|graph| graph.as_array_mut())
    else {
        return document;
    };
    for (package, license) in concluded {
        let Some(target) = licenses.target(license) else {
            continue;
        };
        let unasserted = graph.iter_mut().find(|element| {
            is_relationship(element)
                && element.get("from").and_then(|from| from.as_str()) == Some(package)
                && element
                    .get("relationshipType")
                    .and_then(|kind| kind.as_str())
                    == Some("hasConcludedLicense")
                && relationship_targets(element)
                    .iter()
                    .all(|to| to.as_str().is_some_and(is_no_assertion))
        });
        match unasserted {
            Some(existing) => {
                existing["to"] = json!([target]);
                existing["creationInfo"] = json!(info_id);
            }
            None => new_relationships.push(relationship(
                format!("{base}#Relationship-{}", new_relationships.len() + 1),
                info_id,
                package,
                "hasConcludedLicense",
                &target,
            )),
        }
    }
    added.extend(licenses.elements);
    added.extend(new_relationships);

    let added_ids: Vec<JsonValue> = added
        .iter()
        .filter_map(|element| element.get("spdxId").cloned())
        .collect();
    if let Some(spdx_document) = graph
        .iter_mut()
        .find(|element| element_type(element) == Some("SpdxDocument"))
    {
        match spdx_document
            .get_mut("element")
            .and_then(|list| list.as_array_mut())
        {
            Some(list) => list.extend(added_ids),
            None => spdx_document["element"] = JsonValue::Array(added_ids),
        }
    }
    graph.extend(added);
    document
}

// =============================================================================
// WRITING
// =============================================================================

/// A prepared SPDX document as SPDX 3.0.1 JSON-LD.
///
/// `document` is what `prepare_spdx_document` returns, so its licenses are already what SPDX
/// accepts and its `hasExtractedLicensingInfos` defines every `LicenseRef-` they use. The 2.x
/// `DESCRIBES` relationships become the SBOM's root elements.
pub fn write(document: &SpdxDocument) -> JsonValue {
    let base = document.document_namespace.as_str();
    let info_id = "_:creationinfo";
    let (agent, tool, agents) = feluda_agents(base, info_id);
    let created = document
        .creation_info
        .created
        .to_rfc3339_opts(SecondsFormat::Secs, true);

    let mut licenses = LicenseElements::new(base, info_id, &document.has_extracted_licensing_infos);
    let mut packages = Vec::new();
    let mut relationships = Vec::new();
    for package in &document.packages {
        let id = format!("{base}#{}", package.spdx_id);
        let mut element = json!({
            "type": "software_Package",
            "spdxId": id,
            "creationInfo": info_id,
            "name": package.name,
        });
        if let Some(version) = &package.version_info {
            element["software_packageVersion"] = json!(version);
        }
        if let Some(purl) = package
            .external_refs
            .iter()
            .find(|reference| reference.reference_type == "purl")
        {
            element["software_packageUrl"] = json!(purl.reference_locator);
        }
        let location = package.download_location.as_str();
        if location != "NOASSERTION" && location != "NONE" {
            element["software_downloadLocation"] = json!(location);
        }
        if let Some(copyright) = package
            .copyright_text
            .as_deref()
            .filter(|text| *text != "NOASSERTION")
        {
            element["software_copyrightText"] = json!(copyright);
        }
        if let Some(comment) = &package.comment {
            element["comment"] = json!(comment);
        }
        packages.push(element);

        for (kind, license) in [
            ("hasDeclaredLicense", &package.license_declared),
            ("hasConcludedLicense", &package.license_concluded),
        ] {
            let Some(target) = license
                .as_deref()
                .and_then(|license| licenses.target(license))
            else {
                continue;
            };
            relationships.push(relationship(
                format!("{base}#Relationship-{}", relationships.len() + 1),
                info_id,
                &id,
                kind,
                &target,
            ));
        }
    }

    // The data license has to be a license element in the graph like any other; a listed
    // license's IRI on its own is not one.
    let data_license = licenses.target(&document.data_license);

    let sbom_id = format!("{base}#SBOM");
    let ids = |elements: &[JsonValue]| -> Vec<JsonValue> {
        elements
            .iter()
            .filter_map(|element| element.get("spdxId").cloned())
            .collect()
    };
    let package_ids = ids(&packages);
    let mut sbom_elements = package_ids.clone();
    sbom_elements.extend(ids(&licenses.elements));
    sbom_elements.extend(ids(&relationships));

    let mut document_elements = vec![json!(sbom_id)];
    document_elements.extend(ids(&agents));
    document_elements.extend(sbom_elements.iter().cloned());

    let mut graph = vec![creation_info(
        info_id,
        SPEC_VERSION,
        &created,
        &agent,
        &tool,
    )];
    graph.extend(agents);
    let mut spdx_document = json!({
        "type": "SpdxDocument",
        "spdxId": base,
        "creationInfo": info_id,
        "name": document.name,
        "profileConformance": ["core", "software", "simpleLicensing"],
        "rootElement": [sbom_id],
        "element": document_elements,
    });
    if let Some(data_license) = data_license {
        spdx_document["dataLicense"] = json!(data_license);
    }
    graph.push(spdx_document);
    let mut sbom = json!({
        "type": "software_Sbom",
        "spdxId": sbom_id,
        "creationInfo": info_id,
        "rootElement": package_ids,
        "element": sbom_elements,
    });
    if let Some(kind) = document.sbom_type {
        sbom["software_sbomType"] = json!([kind.as_str()]);
    }
    graph.push(sbom);
    graph.extend(packages);
    graph.extend(licenses.elements);
    graph.extend(relationships);

    json!({ "@context": CONTEXT, "@graph": graph })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sbom::spdx::SpdxPackage;

    /// Shaped like Yocto's output: expressions with custom ids mapped to licensing texts, and an
    /// explicit "no assertion" conclusion.
    fn yocto_style() -> JsonValue {
        json!({
            "@context": CONTEXT,
            "@graph": [
                {
                    "type": "CreationInfo",
                    "@id": "_:CreationInfo1",
                    "specVersion": "3.0.1",
                    "created": "2026-10-07T10:00:00Z",
                    "createdBy": ["http://spdx.org/spdxdocs/openembedded#Agent"]
                },
                {
                    "type": "SpdxDocument",
                    "spdxId": "http://spdx.org/spdxdocs/core-image",
                    "creationInfo": "_:CreationInfo1",
                    "name": "core-image-minimal",
                    "element": ["http://spdx.org/spdxdocs/core-image#busybox"]
                },
                {
                    "type": "software_Package",
                    "spdxId": "http://spdx.org/spdxdocs/core-image#busybox",
                    "creationInfo": "_:CreationInfo1",
                    "name": "busybox",
                    "software_packageVersion": "1.36.1",
                    "externalIdentifier": [{
                        "type": "ExternalIdentifier",
                        "externalIdentifierType": "packageUrl",
                        "identifier": "pkg:generic/busybox@1.36.1"
                    }]
                },
                {
                    "type": "software_Package",
                    "spdxId": "http://spdx.org/spdxdocs/core-image#zlib",
                    "creationInfo": "_:CreationInfo1",
                    "name": "zlib",
                    "software_packageVersion": "1.3.1",
                    "software_packageUrl": "pkg:generic/zlib@1.3.1"
                },
                {
                    "type": "software_Package",
                    "spdxId": "http://spdx.org/spdxdocs/core-image#mystery",
                    "creationInfo": "_:CreationInfo1",
                    "name": "mystery"
                },
                {
                    "type": "simplelicensing_LicenseExpression",
                    "spdxId": "http://spdx.org/spdxdocs/core-image#License-busybox",
                    "creationInfo": "_:CreationInfo1",
                    "simplelicensing_licenseExpression": "GPL-2.0-only AND LicenseRef-bzip2-1.0.4",
                    "simplelicensing_customIdToUri": [{
                        "type": "DictionaryEntry",
                        "key": "LicenseRef-bzip2-1.0.4",
                        "value": "http://spdx.org/spdxdocs/core-image#Text-bzip2"
                    }]
                },
                {
                    "type": "simplelicensing_SimpleLicensingText",
                    "spdxId": "http://spdx.org/spdxdocs/core-image#Text-bzip2",
                    "creationInfo": "_:CreationInfo1",
                    "name": "bzip2-1.0.4",
                    "simplelicensing_licenseText": "This program, \"bzip2\", ..."
                },
                {
                    "type": "Relationship",
                    "spdxId": "http://spdx.org/spdxdocs/core-image#R1",
                    "creationInfo": "_:CreationInfo1",
                    "from": "http://spdx.org/spdxdocs/core-image#busybox",
                    "relationshipType": "hasDeclaredLicense",
                    "to": ["http://spdx.org/spdxdocs/core-image#License-busybox"]
                },
                {
                    "type": "Relationship",
                    "spdxId": "http://spdx.org/spdxdocs/core-image#R2",
                    "creationInfo": "_:CreationInfo1",
                    "from": "http://spdx.org/spdxdocs/core-image#zlib",
                    "relationshipType": "hasConcludedLicense",
                    "to": ["https://spdx.org/licenses/Zlib"]
                },
                {
                    "type": "Relationship",
                    "spdxId": "http://spdx.org/spdxdocs/core-image#R3",
                    "creationInfo": "_:CreationInfo1",
                    "from": "http://spdx.org/spdxdocs/core-image#mystery",
                    "relationshipType": "hasConcludedLicense",
                    "to": ["expandedlicensing_NoAssertionLicense"]
                }
            ]
        })
    }

    #[test]
    fn test_detects_spdx3() {
        assert!(is_spdx3(&yocto_style()));
        assert!(is_spdx3(&json!({ "@context": [CONTEXT, { "x": "y" }] })));
        assert!(!is_spdx3(&json!({ "spdxVersion": "SPDX-2.3" })));
        assert!(!is_spdx3(&json!({ "@context": "https://schema.org" })));
    }

    #[test]
    fn test_normalizes_packages_and_their_licenses() {
        let normalized = normalize(&yocto_style());
        assert_eq!(normalized["spdxVersion"], "SPDX-3.0.1");
        assert_eq!(normalized["name"], "core-image-minimal");

        let packages = normalized["packages"].as_array().unwrap();
        assert_eq!(packages.len(), 3);
        assert_eq!(
            packages[0]["SPDXID"],
            "http://spdx.org/spdxdocs/core-image#busybox"
        );
        assert_eq!(packages[0]["versionInfo"], "1.36.1");
        assert_eq!(
            packages[0]["externalRefs"][0]["referenceLocator"],
            "pkg:generic/busybox@1.36.1"
        );
        // The custom id's text is not one the matcher knows, so its name stands in.
        assert_eq!(
            packages[0]["licenseDeclared"],
            "GPL-2.0-only AND bzip2-1.0.4"
        );
        assert!(packages[0].get("licenseConcluded").is_none());

        // A listed license's IRI is its id.
        assert_eq!(packages[1]["licenseConcluded"], "Zlib");
        // "No assertion" says nothing.
        assert!(packages[2].get("licenseConcluded").is_none());
    }

    #[test]
    fn test_expanded_licensing_elements_become_expressions() {
        let document = json!({
            "@context": CONTEXT,
            "@graph": [
                {
                    "type": "software_Package",
                    "spdxId": "urn:p",
                    "name": "p"
                },
                {
                    "type": "expandedlicensing_DisjunctiveLicenseSet",
                    "spdxId": "urn:set",
                    "expandedlicensing_member": [
                        "https://spdx.org/licenses/MIT",
                        {
                            "type": "expandedlicensing_ConjunctiveLicenseSet",
                            "expandedlicensing_member": [
                                {
                                    "type": "expandedlicensing_OrLaterOperator",
                                    "expandedlicensing_subjectLicense": "https://spdx.org/licenses/LGPL-2.1"
                                },
                                {
                                    "type": "expandedlicensing_WithAdditionOperator",
                                    "expandedlicensing_subjectExtendableLicense": "https://spdx.org/licenses/GPL-2.0-only",
                                    "expandedlicensing_subjectAddition": "https://spdx.org/licenses/Classpath-exception-2.0"
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": "Relationship",
                    "spdxId": "urn:r",
                    "from": "urn:p",
                    "relationshipType": "hasConcludedLicense",
                    "to": ["urn:set"]
                }
            ]
        });
        assert_eq!(
            normalize(&document)["packages"][0]["licenseConcluded"],
            "MIT OR (LGPL-2.1+ AND (GPL-2.0-only WITH Classpath-exception-2.0))"
        );
    }

    #[test]
    fn test_license_cycles_end() {
        let document = json!({
            "@context": CONTEXT,
            "@graph": [
                { "type": "software_Package", "spdxId": "urn:p", "name": "p" },
                {
                    "type": "expandedlicensing_OrLaterOperator",
                    "spdxId": "urn:loop",
                    "expandedlicensing_subjectLicense": "urn:loop"
                },
                {
                    "type": "Relationship",
                    "spdxId": "urn:r",
                    "from": "urn:p",
                    "relationshipType": "hasConcludedLicense",
                    "to": ["urn:loop"]
                }
            ]
        });
        assert!(normalize(&document)["packages"][0]
            .get("licenseConcluded")
            .is_none());
    }

    #[test]
    fn test_enrich_adds_elements_and_replaces_no_assertion() {
        let original = yocto_style();
        let refs = vec![ExtractedLicensingInfo {
            license_id: "LicenseRef-feluda-Acme".to_string(),
            extracted_text: "Acme".to_string(),
            name: Some("Acme".to_string()),
            comment: None,
        }];
        let enriched = enrich(
            &original,
            &[
                (
                    "http://spdx.org/spdxdocs/core-image#busybox".to_string(),
                    "GPL-2.0-only".to_string(),
                ),
                (
                    "http://spdx.org/spdxdocs/core-image#mystery".to_string(),
                    "MIT OR LicenseRef-feluda-Acme".to_string(),
                ),
            ],
            &refs,
        );

        let graph = enriched["@graph"].as_array().unwrap();
        let original_len = original["@graph"].as_array().unwrap().len();
        // Creation info, agent, tool, two expressions, one licensing text, one new relationship.
        assert_eq!(graph.len(), original_len + 7);

        // The explicit "no assertion" now points at feluda's conclusion, under feluda's name.
        let r3 = graph
            .iter()
            .find(|element| element_id(element) == Some("http://spdx.org/spdxdocs/core-image#R3"))
            .unwrap();
        assert_eq!(r3["creationInfo"], "_:feluda-creationinfo");
        let normalized = normalize(&enriched);
        assert_eq!(normalized["packages"][2]["licenseConcluded"], "MIT OR Acme");
        assert_eq!(
            normalized["packages"][0]["licenseConcluded"],
            "GPL-2.0-only"
        );
        // The declaration the document made stays.
        assert_eq!(
            normalized["packages"][0]["licenseDeclared"],
            "GPL-2.0-only AND bzip2-1.0.4"
        );

        let listed = graph
            .iter()
            .find(|element| element_type(element) == Some("SpdxDocument"))
            .unwrap()["element"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(listed, 1 + 2 + 3 + 1);
    }

    #[test]
    fn test_write_states_licenses_as_relationships() {
        let mut document = SpdxDocument::new("demo");
        document.add_package(
            SpdxPackage::new("serde", &document.document_namespace)
                .with_version("1.0.219")
                .with_purl("pkg:cargo/serde@1.0.219")
                .with_license("MIT OR LicenseRef-feluda-Acme"),
        );
        document.add_package(
            SpdxPackage::new("unknown", &document.document_namespace).with_license("NOASSERTION"),
        );
        document
            .has_extracted_licensing_infos
            .push(ExtractedLicensingInfo {
                license_id: "LicenseRef-feluda-Acme".to_string(),
                extracted_text: "Acme".to_string(),
                name: Some("Acme".to_string()),
                comment: None,
            });

        let written = write(&document);
        assert_eq!(written["@context"], CONTEXT);
        let sbom = |written: &JsonValue| {
            written["@graph"]
                .as_array()
                .unwrap()
                .iter()
                .find(|element| element_type(element) == Some("software_Sbom"))
                .unwrap()
                .clone()
        };
        // Nothing said what the inventory was made from, so nothing is claimed.
        assert!(sbom(&written).get("software_sbomType").is_none());
        let mut analyzed = document.clone();
        analyzed.sbom_type = Some(crate::sbom::spdx::SbomKind::Analyzed);
        assert_eq!(
            sbom(&write(&analyzed))["software_sbomType"],
            json!(["analyzed"])
        );
        let graph = written["@graph"].as_array().unwrap();
        assert_eq!(graph[0]["type"], "CreationInfo");
        assert_eq!(graph[0]["specVersion"], "3.0.1");

        let of_type = |kind: &str| {
            graph
                .iter()
                .filter(|element| element_type(element) == Some(kind))
                .count()
        };
        assert_eq!(of_type("software_Package"), 2);
        // Declared and concluded are the same license, so one expression serves both; the other
        // is the document's data license.
        assert_eq!(of_type("simplelicensing_LicenseExpression"), 2);
        assert_eq!(of_type("simplelicensing_SimpleLicensingText"), 1);
        // NOASSERTION is no relationship at all.
        assert_eq!(of_type("Relationship"), 2);

        // Read back, it is the same inventory.
        let normalized = normalize(&written);
        assert_eq!(normalized["packages"][0]["name"], "serde");
        assert_eq!(
            normalized["packages"][0]["externalRefs"][0]["referenceLocator"],
            "pkg:cargo/serde@1.0.219"
        );
        assert_eq!(normalized["packages"][0]["licenseConcluded"], "MIT OR Acme");
        assert!(normalized["packages"][1].get("licenseConcluded").is_none());
    }
}
