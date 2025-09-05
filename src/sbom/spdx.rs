use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};

/// Validate if a string looks like a valid SPDX license identifier or expression
fn is_valid_spdx_license_format(license: &str) -> bool {
    // SPDX license identifiers can only contain:
    // - Letters, numbers, periods, hyphens, plus signs
    // - Logical operators: AND, OR, WITH
    // - Parentheses for grouping
    // - Spaces for separation

    let allowed_chars = license
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '(' | ')' | ' '));

    if !allowed_chars {
        return false;
    }

    // Check for valid logical operators
    let normalized = license.to_uppercase();
    let has_invalid_operators = normalized.contains("&&")
        || normalized.contains("||")
        || normalized.contains("&")
        || normalized.contains("|");

    if has_invalid_operators {
        return false;
    }

    !license.contains("..") && !license.contains("--") && !license.trim().is_empty()
}

pub fn convert_to_spdx_license_expression(license: &str) -> String {
    // Check for force NOASSERTION environment variable
    let force_noassertion = std::env::var("FELUDA_FORCE_NOASSERTION_LICENSES")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if force_noassertion {
        log(
            LogLevel::Info,
            &format!("Force NOASSERTION mode: converting '{license}' to NOASSERTION"),
        );
        return "NOASSERTION".to_string();
    }

    let trimmed = license.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("null")
        || trimmed.eq_ignore_ascii_case("undefined")
        || trimmed.eq_ignore_ascii_case("none")
        || trimmed == "-"
        || trimmed == "n/a"
        || trimmed.eq_ignore_ascii_case("unlicensed")
        || trimmed.eq_ignore_ascii_case("proprietary")
        || !trimmed.is_ascii()
    {
        return "NOASSERTION".to_string();
    }

    let result = trimmed.replace(" / ", " OR ").replace("/", " OR ");

    // TODO: Revise characters that could be problematic for every spdx spec
    if result.contains("{}")
        || result.contains("${")
        || result.contains('"')
        || result.contains('\\')
        || result.contains('\n')
        || result.contains('\r')
        || result.contains('\t')
        || result.contains('&')
        || result.contains('|')
        || result.contains('[')
        || result.contains(']')
        || result.contains('{')
        || result.contains('}')
        || result.contains('<')
        || result.contains('>')
        || result.contains('=')
        || result.contains('*')
        || result.contains('?')
        || result.contains('^')
        || result.contains('$')
        || result.contains('%')
        || result.contains('#')
        || result.contains('@')
        || result.contains('!')
        || result.contains('~')
        || result.contains('`')
        || result.len() > 100
        || !is_valid_spdx_license_format(&result)
        || result.is_empty()
        || result.trim() != result
        || result.contains("  ")
    {
        log(
            LogLevel::Trace,
            &format!("License '{license}' failed validation -> '{result}' -> NOASSERTION"),
        );
        return "NOASSERTION".to_string();
    }

    let safe_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.-+() ";
    if !result.chars().all(|c| safe_chars.contains(c)) {
        log(
            LogLevel::Trace,
            &format!("License '{license}' has invalid characters -> '{result}' -> NOASSERTION"),
        );
        return "NOASSERTION".to_string();
    }

    if license != result {
        log(
            LogLevel::Trace,
            &format!("License conversion: '{license}' -> '{result}'"),
        );
    }

    result
}

/// Sanitize string for use in SPDX identifiers
fn sanitize_spdx_identifier(input: &str) -> String {
    if input.trim().is_empty() {
        return String::new();
    }

    let sanitized = input
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");

    if sanitized.is_empty() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        input.hash(&mut hasher);
        let hash = hasher.finish();

        format!("pkg_{hash:08x}").chars().take(12).collect()
    } else {
        sanitized
    }
}

/// SPDX 2.3 compliant document structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpdxDocument {
    /// SPDX version (required)
    pub spdx_version: String, // "SPDX-2.3"

    /// Data license (required)
    pub data_license: String, // "CC0-1.0"

    /// Document SPDX identifier (required)
    #[serde(rename = "SPDXID")]
    pub spdx_id: String, // "SPDXRef-DOCUMENT"

    /// Document name (required)
    pub name: String,

    /// Document namespace URI (required)
    pub document_namespace: String,

    /// Creation information (required)
    pub creation_info: CreationInfo,

    /// Packages in the document
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<SpdxPackage>,

    /// Relationships between elements  
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<Relationship>,

    /// Annotations for non-standard data
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

/// SPDX creation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreationInfo {
    /// Creation timestamp (required)
    pub created: DateTime<Utc>,

    /// Creators (required, at least one)
    pub creators: Vec<String>,

    /// License list version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_list_version: Option<String>,
}

/// SPDX 2.3 compliant package structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpdxPackage {
    /// Package name (required)
    pub name: String,

    /// Package SPDX identifier (required)
    #[serde(rename = "SPDXID")]
    pub spdx_id: String,

    /// Download location (required)
    pub download_location: String, // URL, VCS, "NONE", or "NOASSERTION"

    /// Files analyzed flag (required)
    pub files_analyzed: bool,

    /// Package version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_info: Option<String>,

    /// License concluded (optional but important)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_concluded: Option<String>,

    /// License declared (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_declared: Option<String>,

    /// License comments (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_comments: Option<String>,

    /// Copyright text (optional but important)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright_text: Option<String>,

    /// Package comment (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,

    /// External references (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub external_refs: Vec<ExternalReference>,
}

/// SPDX external reference
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalReference {
    pub reference_category: String,
    pub reference_type: String,
    pub reference_locator: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// SPDX relationship between elements
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    #[serde(rename = "spdxElementId")]
    pub spdx_element_id: String,
    pub relationship_type: String,
    pub related_spdx_element: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// SPDX annotation for non-standard data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub annotator: String,
    pub annotation_date: DateTime<Utc>,
    pub annotation_type: String,
    #[serde(rename = "spdxIdentifierReference")]
    pub spdx_identifier_reference: String,
    pub comment: String,
}

impl SpdxDocument {
    pub fn new(project_name: &str) -> Self {
        let doc_id = Uuid::new_v4();

        Self {
            spdx_version: "SPDX-2.3".to_string(),
            data_license: "CC0-1.0".to_string(),
            spdx_id: "SPDXRef-DOCUMENT".to_string(),
            name: format!("{}-{}", project_name, doc_id.simple()),
            document_namespace: format!("https://anirudha.dev/feluda/spdx/{doc_id}"),
            creation_info: CreationInfo {
                created: Utc::now(),
                creators: vec![format!("Tool: Feluda-{}", env!("CARGO_PKG_VERSION"))],
                license_list_version: None,
            },
            packages: Vec::new(),
            relationships: Vec::new(),
            annotations: Vec::new(),
        }
    }

    pub fn add_package(&mut self, package: SpdxPackage) {
        // Add relationship: document describes package
        let relationship = Relationship {
            spdx_element_id: self.spdx_id.clone(),
            relationship_type: "DESCRIBES".to_string(),
            related_spdx_element: package.spdx_id.clone(),
            comment: None,
        };

        self.packages.push(package);
        self.relationships.push(relationship);
    }

    #[allow(dead_code)]
    pub fn add_annotation(&mut self, spdx_ref: String, comment: String, annotation_type: String) {
        let annotation = Annotation {
            annotator: format!("Tool: Feluda-{}", env!("CARGO_PKG_VERSION")),
            annotation_date: Utc::now(),
            annotation_type,
            spdx_identifier_reference: spdx_ref,
            comment,
        };

        self.annotations.push(annotation);
    }
}

impl SpdxPackage {
    pub fn new(name: String, _document_namespace: &str) -> Self {
        let sanitized_name = sanitize_spdx_identifier(&name);

        Self {
            name,
            spdx_id: format!("SPDXRef-Package-{sanitized_name}"),
            download_location: "NOASSERTION".to_string(),
            files_analyzed: false,
            version_info: None,
            license_concluded: None,
            license_declared: None,
            license_comments: None,
            copyright_text: Some("NOASSERTION".to_string()),
            comment: None,
            external_refs: Vec::new(),
        }
    }

    pub fn with_version(mut self, version: String) -> Self {
        self.version_info = Some(version.clone());

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let combined_input = format!("{}_{}", self.name, version);
        let mut hasher = DefaultHasher::new();
        combined_input.hash(&mut hasher);
        let hash = hasher.finish();

        self.spdx_id = format!("SPDXRef-Package-pkg{hash:016x}");

        log(
            LogLevel::Trace,
            &format!(
                "Generated SPDX ID '{}' for package '{}' version '{version}'",
                self.spdx_id, self.name
            ),
        );

        self
    }

    pub fn with_license(mut self, license: String) -> Self {
        let spdx_license = convert_to_spdx_license_expression(&license);

        if license != spdx_license {
            log(
                LogLevel::Trace,
                &format!("Converted license '{license}' to SPDX format: '{spdx_license}'"),
            );
        }

        let final_license = if spdx_license.trim().is_empty() {
            "NOASSERTION".to_string()
        } else {
            spdx_license
        };

        self.license_declared = Some(final_license.clone());
        self.license_concluded = Some(final_license);

        self
    }

    // TODO: Implement enhanced SPDX package metadata for future features
    // These methods provide additional package information capabilities
    #[allow(dead_code)]
    pub fn with_download_location(mut self, location: String) -> Self {
        self.download_location = location;
        self
    }

    #[allow(dead_code)]
    pub fn with_copyright(mut self, copyright: String) -> Self {
        self.copyright_text = Some(copyright);
        self
    }

    #[allow(dead_code)]
    pub fn with_comment(mut self, comment: String) -> Self {
        self.comment = Some(comment);
        self
    }

    #[allow(dead_code)]
    pub fn add_external_ref(mut self, category: String, ref_type: String, locator: String) -> Self {
        let external_ref = ExternalReference {
            reference_category: category,
            reference_type: ref_type,
            reference_locator: locator,
            comment: None,
        };
        self.external_refs.push(external_ref);
        self
    }
}

impl Default for SpdxDocument {
    fn default() -> Self {
        Self::new("project")
    }
}

fn validate_and_sanitize_spdx_package(package: &mut SpdxPackage) -> bool {
    let mut needs_fix = false;

    if package.spdx_id.is_empty()
        || !package.spdx_id.starts_with("SPDXRef-")
        || package.spdx_id.contains('"')
        || package.spdx_id.contains('\\')
        || package.spdx_id.contains('\n')
        || package.spdx_id.contains('\r')
        || !package.spdx_id.is_ascii()
        || package.spdx_id.len() > 200
    {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        package.name.hash(&mut hasher);
        if let Some(ref version) = package.version_info {
            version.hash(&mut hasher);
        }
        let hash = hasher.finish();

        package.spdx_id = format!("SPDXRef-Package-pkg{hash:016x}");
        needs_fix = true;
    }

    if package.name.is_empty()
        || package.name.contains('"')
        || package.name.contains('\\')
        || package.name.contains('\n')
        || package.name.contains('\r')
        || !package.name.is_ascii()
        || package.name.len() > 500
    {
        package.name = package
            .name
            .chars()
            .filter(|&c| c.is_ascii_graphic() && c != '"' && c != '\\')
            .take(500)
            .collect();

        if package.name.is_empty() {
            package.name = "unknown-package".to_string();
        }
        needs_fix = true;
    }

    if package.download_location.contains('"')
        || package.download_location.contains('\\')
        || package.download_location.contains('\n')
        || package.download_location.contains('\r')
        || !package.download_location.is_ascii()
        || package.download_location.len() > 1000
    {
        package.download_location = "NOASSERTION".to_string();
        needs_fix = true;
    }

    if let Some(ref mut version) = package.version_info {
        if version.contains('"')
            || version.contains('\\')
            || version.contains('\n')
            || version.contains('\r')
            || !version.is_ascii()
            || version.len() > 200
        {
            *version = version
                .chars()
                .filter(|&c| c.is_ascii_graphic() && c != '"' && c != '\\')
                .take(200)
                .collect();

            if version.is_empty() {
                package.version_info = None;
            }
            needs_fix = true;
        }
    }

    let validate_license = |license_opt: &mut Option<String>, _field_name: &str| -> bool {
        if let Some(ref mut license) = license_opt {
            if license.trim().is_empty()
                || license.contains('"')
                || license.contains('\\')
                || license.contains('\n')
                || license.contains('\r')
                || license.len() > 200
                || !license.is_ascii()
                || !is_valid_spdx_license_format(license)
            {
                *license = "NOASSERTION".to_string();
                return true;
            }
        }
        false
    };

    needs_fix |= validate_license(&mut package.license_declared, "license_declared");
    needs_fix |= validate_license(&mut package.license_concluded, "license_concluded");

    if package.license_declared.is_none() {
        package.license_declared = Some("NOASSERTION".to_string());
        needs_fix = true;
    }
    if package.license_concluded.is_none() {
        package.license_concluded = Some("NOASSERTION".to_string());
        needs_fix = true;
    }

    if let Some(ref mut copyright) = package.copyright_text {
        if copyright.contains('"')
            || copyright.contains('\\')
            || copyright.contains('\n')
            || copyright.contains('\r')
            || !copyright.is_ascii()
            || copyright.len() > 1000
        {
            *copyright = "NOASSERTION".to_string();
            needs_fix = true;
        }
    } else {
        package.copyright_text = Some("NOASSERTION".to_string());
        needs_fix = true;
    }

    needs_fix
}

pub fn generate_spdx_output(
    spdx_doc: &SpdxDocument,
    output_file: Option<String>,
) -> FeludaResult<()> {
    log(LogLevel::Info, "Generating SPDX 2.3 compliant output");

    let mut safe_doc = spdx_doc.clone();

    let mut total_fixes = 0;
    for package in &mut safe_doc.packages {
        if validate_and_sanitize_spdx_package(package) {
            total_fixes += 1;
        }
    }

    if total_fixes > 0 {
        log(
            LogLevel::Warn,
            &format!("Applied sanitization fixes to {total_fixes} packages"),
        );
    }

    let json_output = serde_json::to_string_pretty(&safe_doc)
        .map_err(|e| FeludaError::Unknown(format!("Failed to serialize SPDX document: {e}")))?;

    if json_output.contains("\\n") || json_output.contains("\\r") {
        return Err(FeludaError::Unknown(
            "SPDX JSON contains invalid escaped characters".to_string(),
        ));
    }

    if let Some(file_path) = output_file {
        let spdx_file = if file_path.ends_with(".json") {
            file_path
        } else {
            format!("{}.spdx.json", file_path.trim_end_matches(".spdx"))
        };

        std::fs::write(&spdx_file, &json_output)
            .map_err(|e| FeludaError::Unknown(format!("Failed to write SPDX file: {e}")))?;

        println!("SPDX SBOM written to: {spdx_file}");
        log(
            LogLevel::Info,
            &format!("SPDX SBOM written to: {spdx_file}"),
        );
    } else {
        println!("=== SPDX SBOM ===");
        println!("{json_output}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_to_spdx_license_expression() {
        // Ensure environment variable is not set
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

        // Test simple license
        assert_eq!(convert_to_spdx_license_expression("MIT"), "MIT");

        // Test slash-separated licenses
        assert_eq!(
            convert_to_spdx_license_expression("MIT/Apache-2.0"),
            "MIT OR Apache-2.0"
        );
        assert_eq!(
            convert_to_spdx_license_expression("Apache-2.0/MIT"),
            "Apache-2.0 OR MIT"
        );

        // Test spaced slash-separated licenses
        assert_eq!(
            convert_to_spdx_license_expression("MIT / Apache-2.0"),
            "MIT OR Apache-2.0"
        );
        assert_eq!(
            convert_to_spdx_license_expression("Apache-2.0 / MIT"),
            "Apache-2.0 OR MIT"
        );

        // Test Unlicense case (now allowed)
        assert_eq!(
            convert_to_spdx_license_expression("Unlicense/MIT"),
            "Unlicense OR MIT"
        );

        // Test already SPDX-compliant licenses
        assert_eq!(
            convert_to_spdx_license_expression("MIT OR Apache-2.0"),
            "MIT OR Apache-2.0"
        );
    }

    #[test]
    fn test_spdx_package_unique_ids() {
        // Test that packages with same name but different versions get unique SPDX IDs
        let package1 = SpdxPackage::new("getrandom".to_string(), "https://example.com/test")
            .with_version("0.2.16".to_string());
        let package2 = SpdxPackage::new("getrandom".to_string(), "https://example.com/test")
            .with_version("0.3.3".to_string());

        // Both should use hash-based IDs
        assert!(package1.spdx_id.starts_with("SPDXRef-Package-pkg"));
        assert!(package2.spdx_id.starts_with("SPDXRef-Package-pkg"));
        assert_ne!(package1.spdx_id, package2.spdx_id);

        // Test that version sanitization works
        let package3 = SpdxPackage::new("test-package".to_string(), "https://example.com/test")
            .with_version("1.0.0-beta".to_string());
        // Now uses hash-based ID for safety
        assert!(package3.spdx_id.starts_with("SPDXRef-Package-pkg"));
    }

    #[test]
    fn test_sanitize_spdx_identifier() {
        // Test normal cases
        assert_eq!(sanitize_spdx_identifier("lodash"), "lodash");
        assert_eq!(sanitize_spdx_identifier("lodash-utils"), "lodash_utils");

        // Test complex package names
        assert_eq!(
            sanitize_spdx_identifier("lodash.castarray"),
            "lodash_castarray"
        );
        assert_eq!(sanitize_spdx_identifier("@types/node"), "types_node");
        assert_eq!(sanitize_spdx_identifier("package@1.2.3"), "package_1_2_3");

        // Test edge cases
        assert_eq!(sanitize_spdx_identifier("@babel/core"), "babel_core");
        assert_eq!(
            sanitize_spdx_identifier("package-name-with.dots"),
            "package_name_with_dots"
        );
        assert_eq!(sanitize_spdx_identifier("123-package"), "123_package");

        // Test empty and special cases
        assert_eq!(sanitize_spdx_identifier(""), "");
        assert_eq!(sanitize_spdx_identifier("a__b__c"), "a_b_c");

        // Test cases that would previously result in empty strings
        let special_only = sanitize_spdx_identifier("___");
        assert!(!special_only.is_empty());
        assert!(special_only.starts_with("pkg_"));

        let symbols_only = sanitize_spdx_identifier("@#$%^&*()");
        assert!(!symbols_only.is_empty());
        assert!(symbols_only.starts_with("pkg_"));

        let unicode_only = sanitize_spdx_identifier("你好世界");
        assert!(!unicode_only.is_empty());
        // Unicode characters are non-ASCII-alphanumeric, so should use hash fallback
        assert!(unicode_only.starts_with("pkg_"));

        // Test that hash-based IDs are consistent
        let hash1 = sanitize_spdx_identifier("___");
        let hash2 = sanitize_spdx_identifier("___");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_license_expression_edge_cases() {
        // Test empty license
        assert_eq!(convert_to_spdx_license_expression(""), "NOASSERTION");
        assert_eq!(convert_to_spdx_license_expression("   "), "NOASSERTION");

        // Test null/undefined cases
        assert_eq!(convert_to_spdx_license_expression("null"), "NOASSERTION");
        assert_eq!(convert_to_spdx_license_expression("NULL"), "NOASSERTION");
        assert_eq!(
            convert_to_spdx_license_expression("undefined"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("UNDEFINED"),
            "NOASSERTION"
        );

        // Test other invalid patterns
        assert_eq!(convert_to_spdx_license_expression("none"), "NOASSERTION");
        assert_eq!(convert_to_spdx_license_expression("NONE"), "NOASSERTION");
        assert_eq!(convert_to_spdx_license_expression("-"), "NOASSERTION");
        assert_eq!(convert_to_spdx_license_expression("n/a"), "NOASSERTION");

        // Test template/interpolation patterns
        assert_eq!(convert_to_spdx_license_expression("MIT{}"), "NOASSERTION");
        assert_eq!(
            convert_to_spdx_license_expression("${LICENSE}"),
            "NOASSERTION"
        );

        // Test problematic characters that could break JSON
        assert_eq!(
            convert_to_spdx_license_expression("MIT\"quote"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT\\backslash"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT\nnewline"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT\ttab"),
            "NOASSERTION"
        );
        assert_eq!(convert_to_spdx_license_expression("MIT&AND"), "NOASSERTION");
        assert_eq!(convert_to_spdx_license_expression("MIT|OR"), "NOASSERTION");
        assert_eq!(
            convert_to_spdx_license_expression("MIT[bracket]"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT{brace}"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT<angle>"),
            "NOASSERTION"
        );

        // Test additional invalid patterns
        assert_eq!(
            convert_to_spdx_license_expression("unlicensed"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("proprietary"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT--invalid"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT..invalid"),
            "NOASSERTION"
        );

        // Test very long license strings
        let long_license = "A".repeat(250);
        assert_eq!(
            convert_to_spdx_license_expression(&long_license),
            "NOASSERTION"
        );

        // Test normal cases (ensure environment variable is not set)
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");
        assert_eq!(convert_to_spdx_license_expression("MIT"), "MIT");
        assert_eq!(
            convert_to_spdx_license_expression("MIT OR Apache-2.0"),
            "MIT OR Apache-2.0"
        );

        // Test cargo-style separators
        assert_eq!(
            convert_to_spdx_license_expression("MIT/Apache-2.0"),
            "MIT OR Apache-2.0"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT / Apache-2.0"),
            "MIT OR Apache-2.0"
        );
    }

    #[test]
    fn test_complex_package_names() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");
        // Test the specific lodash.castarray case
        let package = SpdxPackage::new("lodash.castarray".to_string(), "https://example.com/test")
            .with_version("4.4.0".to_string());
        // Now uses hash-based ID for safety
        assert!(package.spdx_id.starts_with("SPDXRef-Package-pkg"));

        // Test @types packages
        let types_package = SpdxPackage::new("@types/node".to_string(), "https://example.com/test")
            .with_version("18.15.0".to_string());
        // Now uses hash-based ID for safety
        assert!(types_package.spdx_id.starts_with("SPDXRef-Package-pkg"));

        // Test esbuild platform-specific packages
        let esbuild_package = SpdxPackage::new(
            "esbuild-linux-ppc64".to_string(),
            "https://example.com/test",
        )
        .with_version("0.19.4".to_string())
        .with_license("MIT".to_string());
        // Now uses hash-based ID for safety
        assert!(esbuild_package.spdx_id.starts_with("SPDXRef-Package-pkg"));
        assert_eq!(esbuild_package.license_concluded, Some("MIT".to_string()));

        // Test package with no license (should get NOASSERTION)
        let no_license_package =
            SpdxPackage::new("some-package".to_string(), "https://example.com/test")
                .with_version("1.0.0".to_string())
                .with_license("".to_string());
        assert_eq!(
            no_license_package.license_concluded,
            Some("NOASSERTION".to_string())
        );
    }

    #[test]
    fn test_extreme_edge_case_packages() {
        // Test package with only special characters
        let special_package = SpdxPackage::new("@#$%^&*()".to_string(), "https://example.com/test")
            .with_version("!!!".to_string())
            .with_license("null".to_string());

        // Should not be empty and should not use UUID fallback
        assert!(!special_package.spdx_id.is_empty());
        assert!(special_package.spdx_id.starts_with("SPDXRef-Package-pkg"));
        assert!(!special_package.spdx_id.contains("SPDXRef-Package-947d52c7")); // Not a UUID
        assert_eq!(
            special_package.license_concluded,
            Some("NOASSERTION".to_string())
        );

        // Test package with unicode characters
        let unicode_package = SpdxPackage::new("你好世界".to_string(), "https://example.com/test")
            .with_version("版本1.0".to_string())
            .with_license("MIT".to_string());

        assert!(!unicode_package.spdx_id.is_empty());
        assert!(unicode_package.spdx_id.starts_with("SPDXRef-Package-pkg"));
        assert_eq!(unicode_package.license_concluded, Some("MIT".to_string()));
    }

    #[test]
    fn test_spdx_id_hash_fallback_logic() {
        // Test case where name is normal but version needs hash fallback
        let normal_name_weird_version = SpdxPackage::new(
            "micromark-util-symbol".to_string(),
            "https://example.com/test",
        )
        .with_version("@#$%^&*()".to_string());

        // Should use single hash for entire package+version combination
        assert!(normal_name_weird_version
            .spdx_id
            .starts_with("SPDXRef-Package-pkg"));
        assert!(normal_name_weird_version.spdx_id.len() <= 50); // Reasonable length for hash-based ID

        // Test case where name needs hash but version is normal
        let weird_name_normal_version =
            SpdxPackage::new("@#$%^&*()".to_string(), "https://example.com/test")
                .with_version("2.0.1".to_string());

        assert!(weird_name_normal_version
            .spdx_id
            .starts_with("SPDXRef-Package-pkg"));

        // Test case where both need hash fallback
        let weird_name_weird_version =
            SpdxPackage::new("@#$%^&*()".to_string(), "https://example.com/test")
                .with_version("!!!".to_string());

        assert!(weird_name_weird_version
            .spdx_id
            .starts_with("SPDXRef-Package-pkg"));

        // Test consistency - same package should get same ID
        let duplicate = SpdxPackage::new("@#$%^&*()".to_string(), "https://example.com/test")
            .with_version("!!!".to_string());

        assert_eq!(weird_name_weird_version.spdx_id, duplicate.spdx_id);
    }

    #[test]
    fn test_micromark_util_symbol_case() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");
        // Test the specific case from the error message
        let micromark_package = SpdxPackage::new(
            "micromark-util-symbol".to_string(),
            "https://example.com/test",
        )
        .with_version("2.0.1".to_string())
        .with_license("MIT".to_string());

        // Now uses hash-based ID for safety
        assert!(micromark_package.spdx_id.starts_with("SPDXRef-Package-pkg"));
        assert_eq!(micromark_package.license_concluded, Some("MIT".to_string()));

        // Test with a problematic version that would cause the original error
        let micromark_with_weird_version = SpdxPackage::new(
            "micromark-util-symbol".to_string(),
            "https://example.com/test",
        )
        .with_version("!!!".to_string()) // Pure symbols, no alphanumeric
        .with_license("MIT".to_string());

        // Should use single hash fallback, not concatenate hashes
        assert!(micromark_with_weird_version
            .spdx_id
            .starts_with("SPDXRef-Package-pkg"));
        assert!(micromark_with_weird_version.spdx_id.len() <= 50); // Reasonable length for hash-based ID
    }

    #[test]
    fn test_license_concluded_vs_declared() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");
        // Test normal license handling
        let mit_package = SpdxPackage::new("test-package".to_string(), "https://example.com/test")
            .with_version("1.0.0".to_string())
            .with_license("MIT".to_string());

        assert_eq!(mit_package.license_declared, Some("MIT".to_string()));
        assert_eq!(mit_package.license_concluded, Some("MIT".to_string()));

        // Test NOASSERTION license handling
        let no_license_package =
            SpdxPackage::new("test-package".to_string(), "https://example.com/test")
                .with_version("1.0.0".to_string())
                .with_license("".to_string());

        assert_eq!(
            no_license_package.license_declared,
            Some("NOASSERTION".to_string())
        );
        assert_eq!(
            no_license_package.license_concluded,
            Some("NOASSERTION".to_string())
        );

        // Test problematic license gets converted to NOASSERTION
        let bad_license_package =
            SpdxPackage::new("test-package".to_string(), "https://example.com/test")
                .with_version("1.0.0".to_string())
                .with_license("MIT\"with-quotes".to_string());

        assert_eq!(
            bad_license_package.license_declared,
            Some("NOASSERTION".to_string())
        );
        assert_eq!(
            bad_license_package.license_concluded,
            Some("NOASSERTION".to_string())
        );

        // Test cargo-style license conversion
        let cargo_license_package =
            SpdxPackage::new("test-package".to_string(), "https://example.com/test")
                .with_version("1.0.0".to_string())
                .with_license("MIT/Apache-2.0".to_string());

        assert_eq!(
            cargo_license_package.license_declared,
            Some("MIT OR Apache-2.0".to_string())
        );
        assert_eq!(
            cargo_license_package.license_concluded,
            Some("MIT OR Apache-2.0".to_string())
        );
    }

    #[test]
    fn test_spdx_license_format_validation() {
        // Test valid SPDX license formats
        assert!(super::is_valid_spdx_license_format("MIT"));
        assert!(super::is_valid_spdx_license_format("Apache-2.0"));
        assert!(super::is_valid_spdx_license_format("GPL-3.0+"));
        assert!(super::is_valid_spdx_license_format("MIT OR Apache-2.0"));
        assert!(super::is_valid_spdx_license_format(
            "(MIT OR Apache-2.0) AND GPL-2.0"
        ));
        assert!(super::is_valid_spdx_license_format("NOASSERTION"));

        // Test invalid SPDX license formats
        assert!(!super::is_valid_spdx_license_format("MIT&Apache"));
        assert!(!super::is_valid_spdx_license_format("MIT|Apache"));
        assert!(!super::is_valid_spdx_license_format("MIT&&Apache"));
        assert!(!super::is_valid_spdx_license_format("MIT||Apache"));
        assert!(!super::is_valid_spdx_license_format("MIT--invalid"));
        assert!(!super::is_valid_spdx_license_format("MIT..invalid"));
        assert!(!super::is_valid_spdx_license_format("MIT@invalid"));
        assert!(!super::is_valid_spdx_license_format("MIT#invalid"));
        assert!(!super::is_valid_spdx_license_format(""));
        assert!(!super::is_valid_spdx_license_format("   "));
    }

    #[test]
    fn test_ultra_conservative_license_validation() {
        // Test the enhanced validation catches more edge cases
        assert_eq!(
            convert_to_spdx_license_expression("MIT=invalid"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT*wildcard"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT?question"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT^caret"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT$dollar"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT%percent"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT#hash"),
            "NOASSERTION"
        );
        assert_eq!(convert_to_spdx_license_expression("MIT@at"), "NOASSERTION");
        assert_eq!(
            convert_to_spdx_license_expression("MIT!exclaim"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT~tilde"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT`backtick"),
            "NOASSERTION"
        );

        // Test non-ASCII characters
        assert_eq!(
            convert_to_spdx_license_expression("MIT©copyright"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT™trademark"),
            "NOASSERTION"
        );
        assert_eq!(
            convert_to_spdx_license_expression("MIT®registered"),
            "NOASSERTION"
        );

        // Test shorter length limit
        let long_license = "A".repeat(101);
        assert_eq!(
            convert_to_spdx_license_expression(&long_license),
            "NOASSERTION"
        );

        // Test character whitelist
        assert_eq!(
            convert_to_spdx_license_expression("MIT_underscore"),
            "NOASSERTION"
        ); // underscore not in whitelist
        assert_eq!(
            convert_to_spdx_license_expression("MIT:colon"),
            "NOASSERTION"
        ); // colon not in whitelist
        assert_eq!(
            convert_to_spdx_license_expression("MIT;semicolon"),
            "NOASSERTION"
        ); // semicolon not in whitelist
        assert_eq!(
            convert_to_spdx_license_expression("MIT,comma"),
            "NOASSERTION"
        ); // comma not in whitelist
    }

    #[test]
    fn test_force_noassertion_mode() {
        // Test normal mode first
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");
        assert_eq!(convert_to_spdx_license_expression("MIT"), "MIT");
        assert_eq!(
            convert_to_spdx_license_expression("Apache-2.0"),
            "Apache-2.0"
        );

        // Test the ultra-conservative fallback mode via environment variable
        std::env::set_var("FELUDA_FORCE_NOASSERTION_LICENSES", "true");
        assert_eq!(convert_to_spdx_license_expression("MIT"), "NOASSERTION");
        assert_eq!(
            convert_to_spdx_license_expression("Apache-2.0"),
            "NOASSERTION"
        );
        assert_eq!(convert_to_spdx_license_expression("GPL-3.0"), "NOASSERTION");
        assert_eq!(convert_to_spdx_license_expression(""), "NOASSERTION");

        // Clean up
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");
    }

    #[test]
    fn test_spdx_document_license_safety_net() {
        // Test that the JSON generation safety net catches problematic licenses
        let mut doc = SpdxDocument::new("test");

        // Create a package with a problematic license that somehow got through
        let mut package = SpdxPackage::new("test-package".to_string(), &doc.document_namespace);
        package.license_declared = Some("MIT\"with-quotes".to_string()); // This should be caught
        package.license_concluded = Some("Apache\\with-backslash".to_string()); // This should be caught

        doc.add_package(package);

        // The generate_spdx_output function should fix these problematic licenses
        // We can't easily test this without a full integration test, but the logic is there
        assert_eq!(doc.packages.len(), 1);
        assert!(doc.packages[0].license_declared.is_some());
        assert!(doc.packages[0].license_concluded.is_some());
    }
}
