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

    // The `LicenseRef-` ids this document defines, which its license fields may use.
    let defined_refs: Vec<String> = parser::get_array(&json_obj, "hasExtractedLicensingInfos")
        .unwrap_or_default()
        .iter()
        .filter_map(|info| parser::get_string(info, "licenseId"))
        .collect();

    if let Some(packages) = parser::get_array(&json_obj, "packages") {
        if packages.is_empty() {
            report.add_issue(ValidationIssue::warning("No packages defined in SBOM"));
        }

        for (idx, package) in packages.iter().enumerate() {
            validate_package(report, package, idx, &defined_refs);
        }
    }
}

/// A license field holds `NOASSERTION`, `NONE`, or an expression over listed SPDX licenses and
/// `LicenseRef-` ids the document defines. Anything else, such as a registry's title for the
/// license, has to be defined as a `LicenseRef-` first.
fn validate_license_field(
    report: &mut ValidationReport,
    package_name: &str,
    field: &str,
    license: &str,
    defined_refs: &[String],
) {
    let license = license.trim();
    if license.eq_ignore_ascii_case("NOASSERTION") || license.eq_ignore_ascii_case("NONE") {
        return;
    }

    if crate::spdx::listed_expression(license).is_none() {
        report.add_issue(
            ValidationIssue::warning(format!(
                "Package '{package_name}': {field} '{license}' is not an SPDX license expression; define other licenses as a LicenseRef- in hasExtractedLicensingInfos"
            ))
            .with_field(field),
        );
        return;
    }

    for id in license
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|token| token.starts_with("LicenseRef-"))
    {
        if !defined_refs.iter().any(|defined| defined == id) {
            report.add_issue(
                ValidationIssue::warning(format!(
                    "Package '{package_name}': {field} uses {id}, which hasExtractedLicensingInfos does not define"
                ))
                .with_field(field),
            );
        }
    }
}

fn validate_package(
    report: &mut ValidationReport,
    package: &JsonValue,
    index: usize,
    defined_refs: &[String],
) {
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

        for field in ["licenseConcluded", "licenseDeclared"] {
            if let Some(license) = parser::get_string(&pkg_json, field) {
                validate_license_field(report, &package_name, field, &license, defined_refs);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn license_warnings(document: JsonValue) -> Vec<String> {
        validate(&document)
            .unwrap()
            .issues
            .into_iter()
            .map(|issue| issue.message)
            .filter(|message| message.contains("license") || message.contains("LicenseRef-"))
            .collect()
    }

    fn package(name: &str, license: &str) -> JsonValue {
        json!({
            "name": name,
            "SPDXID": format!("SPDXRef-{name}"),
            "downloadLocation": "NOASSERTION",
            "licenseConcluded": license,
            "licenseDeclared": "NOASSERTION"
        })
    }

    #[test]
    fn test_license_fields_must_be_spdx_expressions() {
        let warnings = license_warnings(json!({
            "spdxVersion": "SPDX-2.3",
            "packages": [
                package("a", "MIT OR Apache-2.0"),
                package("b", "NONE"),
                package("c", "LicenseRef-defined AND MIT"),
                package("d", "SEE LICENSE IN LICENSE.txt"),
                package("e", "LicenseRef-missing"),
            ],
            "hasExtractedLicensingInfos": [
                { "licenseId": "LicenseRef-defined", "extractedText": "Acme" }
            ]
        }));

        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings[0].contains("'d'") && warnings[0].contains("not an SPDX license expression")
        );
        assert!(warnings[1].contains("'e'") && warnings[1].contains("does not define"));
    }
}
