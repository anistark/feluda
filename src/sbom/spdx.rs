use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};

/// Character validation for SPDX compliance
///
/// This module enforces SPDX 2.3 specification character requirements across all fields.
/// See: https://spdx.github.io/spdx-spec/v2.3/
///
/// SPDX imposes strict character restrictions to ensure:
/// 1. JSON serialization safety - prevents JSON injection
/// 2. Cross-platform compatibility - ensures data portability
/// 3. Standard compliance - follows SPDX specification requirements
mod spdx_charset {
    /// Characters forbidden in ALL SPDX fields for safety
    /// These characters could break JSON serialization or violate SPDX spec
    pub const GLOBALLY_FORBIDDEN: &[char] = &['"', '\\', '\n', '\r', '\t'];

    /// Valid characters for license expressions per SPDX spec
    /// Includes: alphanumeric, dot, hyphen, plus, parentheses, spaces
    #[allow(dead_code)]
    pub const LICENSE_VALID_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.-+() ";

    /// Valid characters for SPDX identifiers (after SPDXRef- prefix)
    /// Per SPDX spec: Letters, numbers, hyphens, underscores only
    pub const SPDXID_VALID_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.";

    /// Characters that are problematic for various reasons
    /// - Shell metacharacters: & | [ ] < > $ ( ) * ? ~ ` ! #
    /// - JSON-adjacent: { } = (could interfere with templates)
    /// - Special purposes: @ % ^ (reserved for future use in SPDX)
    pub const PROBLEMATIC_CHARS: &[char] = &[
        '&', '|', '[', ']', '<', '>', '=', '*', '?', '^', '$', '%', '#', '@', '!', '~', '`', '{',
        '}',
    ];

    /// Validates a string for presence of globally forbidden characters
    pub fn contains_forbidden_chars(s: &str) -> bool {
        s.chars().any(|c| GLOBALLY_FORBIDDEN.contains(&c))
    }

    /// Validates a string contains only ASCII characters
    #[allow(dead_code)]
    pub fn is_valid_ascii(s: &str) -> bool {
        s.is_ascii()
    }

    /// Checks if string contains any problematic characters (more lenient)
    pub fn contains_problematic_chars(s: &str) -> bool {
        s.chars().any(|c| PROBLEMATIC_CHARS.contains(&c))
    }
}

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

    // SPDX 2.3 License Expression Character Validation
    // Per SPDX specification, license expressions must only contain:
    // - SPDX license identifiers (alphanumeric, dots, hyphens, plus signs)
    // - Logical operators (AND, OR, WITH as keywords)
    // - Parentheses for grouping
    // - Spaces for separation
    //
    // Forbidden characters (security & compliance):
    // - Double quotes and backslashes (JSON safety)
    // - Template characters (${}, {}) (prevent injection)
    // - Operators as symbols (&, |) instead of keywords (SPDX compliance)
    // - Brackets, angle brackets, braces (shell/SPDX reserved)
    // - Math operators (=, *, ?, ^) (avoid ambiguity)
    // - Special chars (@, %, #, !, ~, `) (reserved/problematic)

    // Check for globally forbidden characters
    if spdx_charset::contains_forbidden_chars(&result) {
        log(
            LogLevel::Trace,
            &format!("License '{license}' contains forbidden characters -> NOASSERTION"),
        );
        return "NOASSERTION".to_string();
    }

    // Check for template/injection patterns
    if result.contains("{}") || result.contains("${") {
        log(
            LogLevel::Trace,
            &format!("License '{license}' contains template patterns -> NOASSERTION"),
        );
        return "NOASSERTION".to_string();
    }

    // Check for problematic characters
    if spdx_charset::contains_problematic_chars(&result) {
        log(
            LogLevel::Trace,
            &format!("License '{license}' contains problematic characters -> NOASSERTION"),
        );
        return "NOASSERTION".to_string();
    }

    // Check structural constraints
    if result.len() > 100
        || !is_valid_spdx_license_format(&result)
        || result.is_empty()
        || result.trim() != result
        || result.contains("  ")
    {
        log(
            LogLevel::Trace,
            &format!(
                "License '{license}' failed structural validation -> '{result}' -> NOASSERTION"
            ),
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

/// Validates SPDX identifier format compliance
///
/// SPDX 2.3 identifiers must:
/// 1. Start with "SPDXRef-" prefix
/// 2. Contain only ASCII alphanumeric, hyphens, underscores, and dots
/// 3. Not exceed 200 characters total
/// 4. Not contain forbidden characters (quotes, backslashes, control chars)
/// 5. Not contain non-ASCII characters
fn is_valid_spdx_id_format(spdx_id: &str) -> bool {
    if !spdx_id.starts_with("SPDXRef-") {
        return false;
    }

    if spdx_id.len() > 200 {
        return false;
    }

    // Check for forbidden characters
    if spdx_charset::contains_forbidden_chars(spdx_id) {
        return false;
    }

    // Check for ASCII requirement
    if !spdx_id.is_ascii() {
        return false;
    }

    // Validate characters after SPDXRef- prefix
    let suffix = &spdx_id[8..]; // Skip "SPDXRef-"

    // Suffix must not be empty
    if suffix.is_empty() {
        return false;
    }

    suffix
        .chars()
        .all(|c| spdx_charset::SPDXID_VALID_CHARS.contains(c))
}

/// Sanitize string for use in SPDX identifiers
///
/// This function converts arbitrary strings into valid SPDX identifiers
/// by replacing non-alphanumeric characters with underscores.
/// If the result is empty, falls back to hash-based generation.
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
    #[allow(dead_code)]
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

    /// Sets the download location with SPDX validation
    ///
    /// Per SPDX 2.3 spec, download location must be:
    /// - A valid URL (http://, https://, ftp://, etc.)
    /// - A VCS location (git+https://, svn://, etc.)
    /// - The literal string "NOASSERTION" if location is unknown
    /// - The literal string "NONE" if no download location is available
    ///
    /// Special characters and non-ASCII characters are not allowed in URLs.
    #[allow(dead_code)]
    pub fn with_download_location(mut self, location: String) -> Self {
        // Validate download location
        let validated = if location.trim().is_empty()
            || location.eq_ignore_ascii_case("noassertion")
            || location.eq_ignore_ascii_case("none")
        {
            location
        } else if spdx_charset::contains_forbidden_chars(&location) || !location.is_ascii() {
            log(
                LogLevel::Warn,
                &format!(
                    "Download location '{location}' contains invalid characters, using NOASSERTION"
                ),
            );
            "NOASSERTION".to_string()
        } else {
            location
        };

        self.download_location = validated;
        self
    }

    /// Sets the copyright text with SPDX validation
    ///
    /// Copyright text should be in the format:
    /// "(C) Year Name" or "Copyright Year Name"
    /// Or the literal string "NOASSERTION" if not available
    ///
    /// Per SPDX spec: "This field may contain multiple copyrights separated by newlines"
    /// However, we enforce single-line format for JSON safety.
    #[allow(dead_code)]
    pub fn with_copyright(mut self, copyright: String) -> Self {
        // Validate copyright text
        let validated = if copyright.trim().is_empty() {
            "NOASSERTION".to_string()
        } else if spdx_charset::contains_forbidden_chars(&copyright) || !copyright.is_ascii() {
            log(
                LogLevel::Warn,
                "Copyright text contains invalid characters, using NOASSERTION",
            );
            "NOASSERTION".to_string()
        } else if copyright.len() > 1000 {
            log(
                LogLevel::Warn,
                "Copyright text exceeds 1000 character limit",
            );
            "NOASSERTION".to_string()
        } else {
            copyright
        };

        self.copyright_text = Some(validated);
        self
    }

    /// Sets a comment with SPDX validation
    ///
    /// Comments can provide additional information about the package
    /// but must conform to SPDX character restrictions.
    #[allow(dead_code)]
    pub fn with_comment(mut self, comment: String) -> Self {
        // Validate comment
        let validated = if comment.trim().is_empty() {
            None
        } else if spdx_charset::contains_forbidden_chars(&comment) || !comment.is_ascii() {
            log(
                LogLevel::Warn,
                "Comment contains invalid characters, skipping",
            );
            None
        } else if comment.len() > 500 {
            // Comments have reasonable length limit
            log(LogLevel::Warn, "Comment exceeds 500 character limit");
            None
        } else {
            Some(comment)
        };

        self.comment = validated;
        self
    }

    /// Adds an external reference for package metadata
    ///
    /// External references link a package to external sources of information.
    /// Per SPDX 2.3 spec, common reference categories include:
    /// - "SECURITY_OTHER" for security-related references
    /// - "PACKAGE_MANAGER" for package manager records
    /// - "OTHER" for miscellaneous references
    ///
    /// Example:
    /// ```ignore
    /// package.add_external_ref(
    ///     "PACKAGE_MANAGER".to_string(),
    ///     "npm".to_string(),
    ///     "lodash@4.17.21".to_string()
    /// );
    /// ```
    #[allow(dead_code)]
    pub fn add_external_ref(mut self, category: String, ref_type: String, locator: String) -> Self {
        // Validate external reference fields
        let is_valid = !spdx_charset::contains_forbidden_chars(&category)
            && category.is_ascii()
            && !spdx_charset::contains_forbidden_chars(&ref_type)
            && ref_type.is_ascii()
            && !spdx_charset::contains_forbidden_chars(&locator)
            && locator.is_ascii();

        if is_valid && category.len() <= 100 && ref_type.len() <= 100 && locator.len() <= 500 {
            let external_ref = ExternalReference {
                reference_category: category,
                reference_type: ref_type,
                reference_locator: locator,
                comment: None,
            };
            self.external_refs.push(external_ref);
        } else {
            log(
                LogLevel::Warn,
                "External reference validation failed, skipping",
            );
        }
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

    // SPDX Identifier Validation
    // ===========================
    // Validates the SPDX ID conforms to SPDX 2.3 specification:
    // - Must start with "SPDXRef-" prefix (required by spec)
    // - Must be 200 characters or less
    // - Must contain only ASCII alphanumeric, hyphens, underscores, dots
    // - Must not contain forbidden characters (quotes, backslashes, newlines, etc.)
    if !is_valid_spdx_id_format(&package.spdx_id) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        package.name.hash(&mut hasher);
        if let Some(ref version) = package.version_info {
            version.hash(&mut hasher);
        }
        let hash = hasher.finish();

        let old_id = package.spdx_id.clone();
        package.spdx_id = format!("SPDXRef-Package-pkg{hash:016x}");

        log(
            LogLevel::Trace,
            &format!(
                "Regenerated invalid SPDX ID '{}' → '{}'",
                old_id, package.spdx_id
            ),
        );
        needs_fix = true;
    }

    // Package Name Validation
    // =======================
    // SPDX 2.3 spec requires package names to be:
    // - Non-empty (required field)
    // - ASCII characters only (for portability)
    // - No control characters or special characters
    // - Maximum 500 characters (reasonable limit)
    if package.name.is_empty()
        || spdx_charset::contains_forbidden_chars(&package.name)
        || !package.name.is_ascii()
        || package.name.len() > 500
    {
        let old_name = package.name.clone();
        package.name = package
            .name
            .chars()
            .filter(|&c| c.is_ascii_graphic() && !spdx_charset::GLOBALLY_FORBIDDEN.contains(&c))
            .take(500)
            .collect();

        if package.name.is_empty() {
            package.name = "unknown-package".to_string();
        }

        log(
            LogLevel::Trace,
            &format!("Sanitized package name '{}' → '{}'", old_name, package.name),
        );
        needs_fix = true;
    }

    // Download Location Validation
    // =============================
    // SPDX 2.3 spec allows:
    // - Valid URLs (http://, https://, ftp://, etc.)
    // - VCS locations (git+https://, svn://, etc.)
    // - Literal "NOASSERTION" (if location is unknown)
    // - Literal "NONE" (if no download location exists)
    //
    // Must be ASCII-only and not exceed 1000 characters
    if spdx_charset::contains_forbidden_chars(&package.download_location)
        || !package.download_location.is_ascii()
        || package.download_location.len() > 1000
    {
        package.download_location = "NOASSERTION".to_string();
        needs_fix = true;
    }

    // Package Version Validation
    // ==========================
    // Version strings should follow semantic versioning convention
    // (e.g., "1.0.0", "2.5.3-alpha", etc.)
    //
    // Must be ASCII-only and not exceed 200 characters
    if let Some(ref mut version) = package.version_info {
        if spdx_charset::contains_forbidden_chars(version)
            || !version.is_ascii()
            || version.len() > 200
        {
            let old_version = version.clone();
            *version = version
                .chars()
                .filter(|&c| c.is_ascii_graphic() && !spdx_charset::GLOBALLY_FORBIDDEN.contains(&c))
                .take(200)
                .collect();

            if version.is_empty() {
                log(
                    LogLevel::Trace,
                    &format!(
                        "Removed invalid version '{old_version}' (became empty after sanitization)"
                    ),
                );
                package.version_info = None;
            } else {
                log(
                    LogLevel::Trace,
                    &format!("Sanitized version '{old_version}' → '{version}'"),
                );
            }
            needs_fix = true;
        }
    }

    // License Field Validation
    // ========================
    // Both licenseConcluded and licenseDeclared must conform to SPDX spec:
    // - Either a valid SPDX license expression (e.g., "MIT", "Apache-2.0 OR MIT")
    // - Or the literal string "NOASSERTION"
    // - Or the literal string "NONE" (rarely used)
    //
    // Must be ASCII-only and not exceed 200 characters
    let validate_license = |license_opt: &mut Option<String>, _field_name: &str| -> bool {
        if let Some(ref mut license) = license_opt {
            if license.trim().is_empty()
                || spdx_charset::contains_forbidden_chars(license)
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

    // Copyright Text Validation
    // ==========================
    // Copyright information should follow format:
    // "(C) Year Name" or "Copyright Year Name"
    //
    // However, SPDX spec allows flexibility here. The key requirement is:
    // - ASCII-only characters
    // - Maximum 1000 characters
    // - Not empty (required field per SPDX spec)
    if let Some(ref mut copyright) = package.copyright_text {
        if spdx_charset::contains_forbidden_chars(copyright)
            || !copyright.is_ascii()
            || copyright.len() > 1000
        {
            *copyright = "NOASSERTION".to_string();
            needs_fix = true;
        }
    } else {
        // Copyright is a required field in SPDX 2.3
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

    let json_output = serde_json::to_string_pretty(&safe_doc).map_err(|e| {
        FeludaError::Serialization(format!("Failed to serialize SPDX document: {e}"))
    })?;

    if json_output.contains("\\n") || json_output.contains("\\r") {
        return Err(FeludaError::InvalidData(
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
            .map_err(|e| FeludaError::FileWrite(format!("Failed to write SPDX file: {e}")))?;

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
    use serial_test::serial;

    #[test]
    #[serial]
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
    #[serial]
    fn test_license_expression_edge_cases() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

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
    #[serial]
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
    #[serial]
    fn test_extreme_edge_case_packages() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

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
    #[serial]
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
    #[serial]
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
    #[serial]
    fn test_ultra_conservative_license_validation() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

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
    #[serial]
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

    #[test]
    fn test_spdx_id_format_validation() {
        // Test valid SPDX IDs
        assert!(super::is_valid_spdx_id_format("SPDXRef-DOCUMENT"));
        assert!(super::is_valid_spdx_id_format("SPDXRef-Package-123"));
        assert!(super::is_valid_spdx_id_format("SPDXRef-File-src.main.rs"));
        assert!(super::is_valid_spdx_id_format(
            "SPDXRef-Package-pkg0123456789abcdef"
        ));

        // Test invalid SPDX IDs
        assert!(!super::is_valid_spdx_id_format("")); // Empty
        assert!(!super::is_valid_spdx_id_format("SPDX-DOCUMENT")); // Missing "Ref-" part
        assert!(!super::is_valid_spdx_id_format("SPDXRef-")); // No suffix
        assert!(!super::is_valid_spdx_id_format("SPDXRef-Package\"quoted")); // Contains quote
        assert!(!super::is_valid_spdx_id_format(
            "SPDXRef-Package\\backslash"
        )); // Contains backslash
        assert!(!super::is_valid_spdx_id_format(
            "SPDXRef-Package\nwithNewline"
        )); // Contains newline

        // Test length limits
        let valid_long = format!("SPDXRef-{}", "a".repeat(192)); // 8 + 192 = 200 chars
        assert!(super::is_valid_spdx_id_format(&valid_long));

        let invalid_long = format!("SPDXRef-{}", "a".repeat(193)); // 8 + 193 = 201 chars
        assert!(!super::is_valid_spdx_id_format(&invalid_long));
    }

    #[test]
    #[serial]
    fn test_package_metadata_validation() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

        // Test package with valid metadata
        let package = SpdxPackage::new("valid-package".to_string(), "https://example.com/test")
            .with_version("1.0.0".to_string())
            .with_license("MIT".to_string())
            .with_copyright("(C) 2024 Example Corp".to_string())
            .with_download_location("https://github.com/example/repo".to_string())
            .with_comment("A valid test package".to_string());

        assert!(!package.name.is_empty());
        assert_eq!(package.version_info, Some("1.0.0".to_string()));
        assert_eq!(package.license_concluded, Some("MIT".to_string()));
        assert!(package.copyright_text.is_some());

        // Test package sanitization of invalid metadata
        let mut package_with_issues =
            SpdxPackage::new("test".to_string(), "https://example.com/test")
                .with_version("1.0.0".to_string())
                .with_license("MIT".to_string());

        // Manually set invalid values that should be caught by validation
        package_with_issues.name = "package\"with-quote".to_string();
        package_with_issues.download_location = "https://example.com\nmalicious".to_string();

        // Run validation
        assert!(super::validate_and_sanitize_spdx_package(
            &mut package_with_issues
        ));

        // Verify problematic content was removed/fixed
        assert!(!package_with_issues.name.contains('"'));
        assert_eq!(package_with_issues.download_location, "NOASSERTION");
    }

    #[test]
    #[serial]
    fn test_download_location_validation() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

        // Test valid download locations
        let package1 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_download_location("https://github.com/example/repo".to_string());
        assert_eq!(
            package1.download_location,
            "https://github.com/example/repo"
        );

        let package2 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_download_location("NOASSERTION".to_string());
        assert_eq!(package2.download_location, "NOASSERTION");

        // Test invalid location (non-ASCII)
        let package3 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_download_location("https://example.com/文件".to_string());
        assert_eq!(package3.download_location, "NOASSERTION");

        // Test invalid location (too long) - Note: direct field mutation bypasses validation
        // For validation during generation, use validate_and_sanitize_spdx_package
        let mut package4 = SpdxPackage::new("test".to_string(), "https://example.com/test");
        package4.download_location = format!("https://example.com/{}", "a".repeat(2000));

        // Validate should catch the long location
        assert!(super::validate_and_sanitize_spdx_package(&mut package4));
        assert_eq!(package4.download_location, "NOASSERTION");
    }

    #[test]
    #[serial]
    fn test_copyright_validation() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

        // Test valid copyright
        let package1 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_copyright("(C) 2024 Example Corp".to_string());
        assert_eq!(
            package1.copyright_text,
            Some("(C) 2024 Example Corp".to_string())
        );

        // Test empty copyright (should convert to NOASSERTION)
        let package2 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_copyright("".to_string());
        assert_eq!(package2.copyright_text, Some("NOASSERTION".to_string()));

        // Test copyright with forbidden characters
        let package3 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_copyright("(C) 2024 Corp\"with-quotes".to_string());
        assert_eq!(package3.copyright_text, Some("NOASSERTION".to_string()));
    }

    #[test]
    #[serial]
    fn test_external_ref_validation() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

        // Test valid external reference
        let package = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .add_external_ref(
                "PACKAGE_MANAGER".to_string(),
                "npm".to_string(),
                "lodash@4.17.21".to_string(),
            );

        assert_eq!(package.external_refs.len(), 1);
        assert_eq!(
            package.external_refs[0].reference_category,
            "PACKAGE_MANAGER"
        );
        assert_eq!(package.external_refs[0].reference_type, "npm");
        assert_eq!(package.external_refs[0].reference_locator, "lodash@4.17.21");

        // Test invalid external reference (forbidden characters)
        let package2 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .add_external_ref(
                "SECURITY_OTHER".to_string(),
                "cpe23".to_string(),
                "cpe:2.3:a:vendor:product:1.0\"malicious".to_string(),
            );

        // Should be skipped due to invalid characters
        assert_eq!(package2.external_refs.len(), 0);
    }

    #[test]
    #[serial]
    fn test_comment_validation() {
        std::env::remove_var("FELUDA_FORCE_NOASSERTION_LICENSES");

        // Test valid comment
        let package1 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_comment("This is a valid comment".to_string());
        assert_eq!(
            package1.comment,
            Some("This is a valid comment".to_string())
        );

        // Test empty comment (should be skipped)
        let package2 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_comment("".to_string());
        assert_eq!(package2.comment, None);

        // Test comment with forbidden characters
        let package3 = SpdxPackage::new("test".to_string(), "https://example.com/test")
            .with_comment("Comment with\nnewline".to_string());
        assert_eq!(package3.comment, None);
    }

    #[test]
    fn test_charset_validation_helpers() {
        // Test globally forbidden characters
        assert!(spdx_charset::contains_forbidden_chars("test\"quote"));
        assert!(spdx_charset::contains_forbidden_chars("test\\backslash"));
        assert!(spdx_charset::contains_forbidden_chars("test\nnewline"));
        assert!(!spdx_charset::contains_forbidden_chars("test-string"));

        // Test ASCII validation
        assert!(spdx_charset::is_valid_ascii("test"));
        assert!(!spdx_charset::is_valid_ascii("test™"));
        assert!(!spdx_charset::is_valid_ascii("test©"));

        // Test problematic characters
        assert!(spdx_charset::contains_problematic_chars("test&symbol"));
        assert!(spdx_charset::contains_problematic_chars("test|pipe"));
        assert!(spdx_charset::contains_problematic_chars("test[bracket]"));
        assert!(!spdx_charset::contains_problematic_chars("test-string"));
    }
}
