use super::parser;
use super::reporter::{ValidationIssue, ValidationReport};
use crate::debug::FeludaResult;
use serde_json::Value as JsonValue;

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
        let valid_versions = ["1.0", "1.1", "1.2", "1.3", "1.4", "1.5"];
        if !valid_versions.contains(&spec_version.as_str()) {
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
            let valid_types = [
                "application",
                "framework",
                "library",
                "container",
                "operating-system",
                "device",
                "firmware",
                "file",
                "install",
                "archive",
                "filing-system",
                "media",
                "other",
            ];
            if !valid_types.contains(&comp_type.as_str()) {
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

            if parser::has_key(&metadata, "tools") {
                if let Some(tools) = parser::get_array(&metadata, "tools") {
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
