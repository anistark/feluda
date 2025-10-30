use super::parser;
use super::reporter::{ValidationIssue, ValidationReport};
use crate::debug::FeludaResult;
use serde_json::Value as JsonValue;

pub fn validate(json: &JsonValue) -> FeludaResult<ValidationReport> {
    let mut report = ValidationReport::new("SPDX");

    let obj = match json.as_object() {
        Some(o) => o,
        None => {
            report.add_issue(ValidationIssue::error(
                "SPDX document must be a JSON object",
            ));
            return Ok(report);
        }
    };

    validate_required_fields(&mut report, obj);
    validate_spdx_version(&mut report, obj);
    validate_document_name(&mut report, obj);
    validate_namespace(&mut report, obj);
    validate_packages(&mut report, obj);

    Ok(report)
}

fn validate_required_fields(
    report: &mut ValidationReport,
    obj: &serde_json::Map<String, JsonValue>,
) {
    let required_fields = [
        "spdxVersion",
        "dataLicense",
        "SPDXID",
        "name",
        "documentNamespace",
        "creationInfo",
    ];

    for field in required_fields {
        if !obj.contains_key(field) {
            report.add_issue(
                ValidationIssue::error(format!("Missing required field: {field}"))
                    .with_field(field),
            );
        }
    }
}

fn validate_spdx_version(report: &mut ValidationReport, obj: &serde_json::Map<String, JsonValue>) {
    if let Some(version) = parser::get_string(&JsonValue::Object(obj.clone()), "spdxVersion") {
        if !version.starts_with("SPDX-") {
            report.add_issue(
                ValidationIssue::warning(format!(
                    "Invalid SPDX version format: {version}. Expected format: SPDX-X.Y"
                ))
                .with_field("spdxVersion"),
            );
        }

        let supported_versions = ["SPDX-2.2", "SPDX-2.3"];
        if !supported_versions.iter().any(|v| version.starts_with(v)) {
            report.add_issue(
                ValidationIssue::info(format!("SPDX version {version} may not be fully supported"))
                    .with_field("spdxVersion"),
            );
        }
    }
}

fn validate_document_name(report: &mut ValidationReport, obj: &serde_json::Map<String, JsonValue>) {
    let json_obj = JsonValue::Object(obj.clone());

    if let Some(name) = parser::get_string(&json_obj, "name") {
        if name.is_empty() {
            report.add_issue(
                ValidationIssue::error("Document name cannot be empty").with_field("name"),
            );
        }
    }
}

fn validate_namespace(report: &mut ValidationReport, obj: &serde_json::Map<String, JsonValue>) {
    let json_obj = JsonValue::Object(obj.clone());

    if let Some(namespace) = parser::get_string(&json_obj, "documentNamespace") {
        if namespace.is_empty() {
            report.add_issue(
                ValidationIssue::error("Document namespace cannot be empty")
                    .with_field("documentNamespace"),
            );
        } else if !namespace.starts_with("https://") && !namespace.starts_with("http://") {
            report.add_issue(
                ValidationIssue::warning("Document namespace should be a valid URI")
                    .with_field("documentNamespace"),
            );
        }
    }
}

fn validate_packages(report: &mut ValidationReport, obj: &serde_json::Map<String, JsonValue>) {
    let json_obj = JsonValue::Object(obj.clone());

    if let Some(packages) = parser::get_array(&json_obj, "packages") {
        if packages.is_empty() {
            report.add_issue(ValidationIssue::warning("No packages defined in SBOM"));
        }

        for (idx, package) in packages.iter().enumerate() {
            validate_package(report, package, idx);
        }
    }
}

fn validate_package(report: &mut ValidationReport, package: &JsonValue, index: usize) {
    if let Some(pkg_obj) = package.as_object() {
        let pkg_json = JsonValue::Object(pkg_obj.clone());

        let package_name =
            parser::get_string(&pkg_json, "name").unwrap_or_else(|| format!("Package[{index}]"));

        if !parser::has_key(&pkg_json, "SPDXID") {
            report.add_issue(
                ValidationIssue::error(format!("Package '{package_name}': missing SPDXID"))
                    .with_field("SPDXID"),
            );
        } else if let Some(spdx_id) = parser::get_string(&pkg_json, "SPDXID") {
            if !spdx_id.starts_with("SPDXRef-") {
                report.add_issue(
                    ValidationIssue::warning(format!(
                        "Package '{package_name}': SPDXID should start with 'SPDXRef-'"
                    ))
                    .with_field("SPDXID"),
                );
            }
        }

        if !parser::has_key(&pkg_json, "downloadLocation") {
            report.add_issue(
                ValidationIssue::error(format!(
                    "Package '{package_name}': missing downloadLocation"
                ))
                .with_field("downloadLocation"),
            );
        }

        if !parser::has_key(&pkg_json, "filesAnalyzed") {
            report.add_issue(
                ValidationIssue::info(format!(
                    "Package '{package_name}': filesAnalyzed not specified"
                ))
                .with_field("filesAnalyzed"),
            );
        }
    }
}
