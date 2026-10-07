use super::parser;
use super::reporter::{ValidationIssue, ValidationReport};
use crate::debug::FeludaResult;
use serde_json::Value as JsonValue;

/// Every published CycloneDX version. syft, Trivy and cdxgen write 1.6 by default.
const SPEC_VERSIONS: [&str; 8] = ["1.0", "1.1", "1.2", "1.3", "1.4", "1.5", "1.6", "1.7"];

/// The lifecycle phases CycloneDX 1.5 onwards defines.
const LIFECYCLE_PHASES: [&str; 7] = [
    "design",
    "pre-build",
    "build",
    "post-build",
    "operations",
    "discovery",
    "decommission",
];

/// The component types any CycloneDX version defines. 1.4 has the first eight; 1.5 added
/// `platform`, `device-driver`, `machine-learning-model` and `data`; 1.6 added
/// `cryptographic-asset`. A type is accepted whatever version the document declares, since a
/// warning here is about a misspelt type, not a version mismatch.
const COMPONENT_TYPES: [&str; 13] = [
    "application",
    "framework",
    "library",
    "container",
    "platform",
    "operating-system",
    "device",
    "device-driver",
    "firmware",
    "file",
    "machine-learning-model",
    "data",
    "cryptographic-asset",
];

pub fn validate(json: &JsonValue) -> FeludaResult<ValidationReport> {
    let mut report = ValidationReport::new("CycloneDX");

    let obj = match json.as_object() {
        Some(o) => o,
        None => {
            report.add_issue(ValidationIssue::error(
                "CycloneDX BOM must be a JSON object",
            ));
            return Ok(report);
        }
    };

    validate_required_fields(&mut report, obj);
    validate_bom_format(&mut report, obj);
    validate_spec_version(&mut report, obj);
    validate_components(&mut report, obj);
    validate_metadata(&mut report, obj);

    Ok(report)
}

fn validate_required_fields(
    report: &mut ValidationReport,
    obj: &serde_json::Map<String, JsonValue>,
) {
    let required_fields = ["bomFormat", "specVersion"];

    for field in required_fields {
        if !obj.contains_key(field) {
            report.add_issue(
                ValidationIssue::error(format!("Missing required field: {field}"))
                    .with_field(field),
            );
        }
    }
}

fn validate_bom_format(report: &mut ValidationReport, obj: &serde_json::Map<String, JsonValue>) {
    let json_obj = JsonValue::Object(obj.clone());

    if let Some(format) = parser::get_string(&json_obj, "bomFormat") {
        if format != "CycloneDX" {
            report.add_issue(
                ValidationIssue::error(format!(
                    "Invalid bomFormat: '{format}'. Expected 'CycloneDX'"
                ))
                .with_field("bomFormat"),
            );
        }
    }
}

fn validate_spec_version(report: &mut ValidationReport, obj: &serde_json::Map<String, JsonValue>) {
    let json_obj = JsonValue::Object(obj.clone());

    if let Some(spec_version) = parser::get_string(&json_obj, "specVersion") {
        if !SPEC_VERSIONS.contains(&spec_version.as_str()) {
            report.add_issue(
                ValidationIssue::warning(format!(
                    "Unknown or unsupported specVersion: {spec_version}"
                ))
                .with_field("specVersion"),
            );
        }
    }
}

fn validate_components(report: &mut ValidationReport, obj: &serde_json::Map<String, JsonValue>) {
    let json_obj = JsonValue::Object(obj.clone());

    if let Some(components) = parser::get_array(&json_obj, "components") {
        if components.is_empty() {
            report.add_issue(ValidationIssue::info(
                "No components defined in CycloneDX BOM",
            ));
        }

        for (idx, component) in components.iter().enumerate() {
            validate_component(report, component, idx);
        }
    }
}

fn validate_component(report: &mut ValidationReport, component: &JsonValue, index: usize) {
    if let Some(comp_obj) = component.as_object() {
        let comp_json = JsonValue::Object(comp_obj.clone());

        let component_name =
            parser::get_string(&comp_json, "name").unwrap_or_else(|| format!("Component[{index}]"));

        if !parser::has_key(&comp_json, "type") {
            report.add_issue(
                ValidationIssue::error(format!(
                    "Component '{component_name}': missing 'type' field"
                ))
                .with_field("type"),
            );
        } else if let Some(comp_type) = parser::get_string(&comp_json, "type") {
            if !COMPONENT_TYPES.contains(&comp_type.as_str()) {
                report.add_issue(
                    ValidationIssue::warning(format!(
                        "Component '{component_name}': unknown component type '{comp_type}'"
                    ))
                    .with_field("type"),
                );
            }
        }

        if parser::has_key(&comp_json, "version") {
            if let Some(version) = parser::get_string(&comp_json, "version") {
                if version.is_empty() {
                    report.add_issue(
                        ValidationIssue::warning(format!(
                            "Component '{component_name}': version cannot be empty"
                        ))
                        .with_field("version"),
                    );
                }
            }
        }

        if let Some(licenses) = parser::get_array(&comp_json, "licenses") {
            for license in licenses {
                if let Some(license_obj) = license.as_object() {
                    let license_json = JsonValue::Object(license_obj.clone());
                    if !parser::has_key(&license_json, "license")
                        && !parser::has_key(&license_json, "expression")
                    {
                        report.add_issue(
                            ValidationIssue::warning(
                                format!(
                                    "Component '{component_name}': license must have either 'license' or 'expression' field"
                                ),
                            )
                            .with_field("licenses"),
                        );
                    }

                    // The schema holds `license.id` to the SPDX list, spelled its way.
                    let id = license_json
                        .get("license")
                        .and_then(|license| parser::get_string(license, "id"));
                    if let Some(id) = id {
                        if crate::spdx::listed_id(&id) != Some(id.as_str()) {
                            report.add_issue(
                                ValidationIssue::warning(format!(
                                    "Component '{component_name}': license id '{id}' is not an SPDX license id; use 'name' for other licenses"
                                ))
                                .with_field("licenses[].license.id"),
                            );
                        }
                    }
                }
            }
        }
    }
}

fn validate_metadata(report: &mut ValidationReport, obj: &serde_json::Map<String, JsonValue>) {
    let json_obj = JsonValue::Object(obj.clone());

    if let Some(metadata) = parser::get_object(&json_obj, "metadata") {
        if let Some(_meta_obj) = metadata.as_object() {
            if parser::has_key(&metadata, "timestamp") {
                if let Some(timestamp) = parser::get_string(&metadata, "timestamp") {
                    if !parser::is_valid_iso_datetime(&timestamp) {
                        report.add_issue(
                            ValidationIssue::warning(format!(
                                "Metadata: invalid timestamp format '{timestamp}'. Expected ISO 8601 format"
                            ))
                            .with_field("metadata.timestamp"),
                        );
                    }
                }
            }

            // A lifecycle is one of the predefined phases, or a phase of one's own with a name.
            for lifecycle in parser::get_array(&metadata, "lifecycles").unwrap_or_default() {
                match (
                    parser::get_string(&lifecycle, "phase"),
                    parser::get_string(&lifecycle, "name"),
                ) {
                    (Some(phase), _) if !LIFECYCLE_PHASES.contains(&phase.as_str()) => report
                        .add_issue(
                            ValidationIssue::warning(format!(
                                "Metadata: unknown lifecycle phase '{phase}'"
                            ))
                            .with_field("metadata.lifecycles[].phase"),
                        ),
                    (None, None) => report.add_issue(
                        ValidationIssue::warning(
                            "Metadata: a lifecycle needs a 'phase' or a 'name'",
                        )
                        .with_field("metadata.lifecycles"),
                    ),
                    _ => {}
                }
            }

            if parser::has_key(&metadata, "tools") {
                // 1.4 and earlier list tools directly; 1.5 and later nest them as components and
                // services.
                let tools = parser::get_array(&metadata, "tools").or_else(|| {
                    let tools = parser::get_object(&metadata, "tools")?;
                    let mut entries = parser::get_array(&tools, "components").unwrap_or_default();
                    entries.extend(parser::get_array(&tools, "services").unwrap_or_default());
                    Some(entries)
                });
                if let Some(tools) = tools {
                    for tool in tools {
                        if let Some(tool_obj) = tool.as_object() {
                            let tool_json = JsonValue::Object(tool_obj.clone());
                            if !parser::has_key(&tool_json, "name") {
                                report.add_issue(
                                    ValidationIssue::warning("Tool entry missing 'name' field")
                                        .with_field("metadata.tools[].name"),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn warnings(report: &ValidationReport) -> Vec<String> {
        report
            .issues
            .iter()
            .map(|issue| issue.message.clone())
            .collect()
    }

    #[test]
    fn test_current_spec_versions_are_supported() {
        for version in ["1.4", "1.5", "1.6", "1.7"] {
            let report =
                validate(&json!({ "bomFormat": "CycloneDX", "specVersion": version })).unwrap();
            assert!(
                !warnings(&report)
                    .iter()
                    .any(|message| message.contains("specVersion")),
                "{version} should be supported: {:?}",
                warnings(&report)
            );
        }

        let report = validate(&json!({ "bomFormat": "CycloneDX", "specVersion": "9.9" })).unwrap();
        assert!(warnings(&report)
            .iter()
            .any(|message| message.contains("unsupported specVersion: 9.9")));
    }

    #[test]
    fn test_tools_are_checked_in_both_shapes() {
        let legacy = validate(&json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.4",
            "metadata": { "tools": [{ "vendor": "anistark" }] }
        }))
        .unwrap();
        let nested = validate(&json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.6",
            "metadata": { "tools": { "components": [{ "type": "application" }] } }
        }))
        .unwrap();

        for report in [legacy, nested] {
            assert!(warnings(&report)
                .iter()
                .any(|message| message == "Tool entry missing 'name' field"));
        }
    }

    #[test]
    fn test_license_ids_must_be_listed() {
        let report = validate(&json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.6",
            "components": [
                { "type": "library", "name": "a", "licenses": [{ "license": { "id": "MIT" } }] },
                { "type": "library", "name": "b", "licenses": [{ "license": { "id": "SEE LICENSE IN LICENSE.txt" } }] },
                { "type": "library", "name": "c", "licenses": [{ "license": { "id": "mit" } }] },
                { "type": "library", "name": "d", "licenses": [{ "license": { "name": "Acme Commercial" } }] }
            ]
        }))
        .unwrap();

        let flagged: Vec<String> = warnings(&report)
            .into_iter()
            .filter(|message| message.contains("is not an SPDX license id"))
            .collect();
        assert_eq!(flagged.len(), 2, "{flagged:?}");
        assert!(flagged.iter().any(|m| m.contains("'b'")));
        // The schema's list is case sensitive.
        assert!(flagged.iter().any(|m| m.contains("'c'")));
    }

    #[test]
    fn test_component_types_follow_the_schema() {
        let report = validate(&json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.6",
            "components": [
                { "type": "platform", "name": "a" },
                { "type": "machine-learning-model", "name": "b" },
                { "type": "cryptographic-asset", "name": "c" },
                { "type": "archive", "name": "d" }
            ]
        }))
        .unwrap();

        let messages = warnings(&report);
        assert!(!messages
            .iter()
            .any(|m| m.contains("'a'") || m.contains("'b'") || m.contains("'c'")));
        // `archive` is an SPDX package purpose, not a CycloneDX component type.
        assert!(messages
            .iter()
            .any(|m| m.contains("'d'") && m.contains("unknown component type")));
    }

    #[test]
    fn test_lifecycles_are_known_phases_or_named() {
        let report = validate(&serde_json::json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.6",
            "metadata": {
                "lifecycles": [
                    { "phase": "pre-build" },
                    { "name": "staging", "description": "Our own phase" },
                    { "phase": "shipping" },
                    {}
                ]
            }
        }))
        .unwrap();
        let messages: Vec<&str> = report
            .issues
            .iter()
            .map(|issue| issue.message.as_str())
            .collect();
        assert_eq!(
            messages,
            [
                "Metadata: unknown lifecycle phase 'shipping'",
                "Metadata: a lifecycle needs a 'phase' or a 'name'"
            ]
        );
    }
}
