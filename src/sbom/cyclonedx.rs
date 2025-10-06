use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::sbom::spdx::SpdxDocument;

/// CycloneDX v1.5 BOM structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycloneDxBom {
    /// BOM format identifier (required)
    pub bom_format: String, // "CycloneDX"

    /// Specification version (required)
    pub spec_version: String, // "1.5"

    /// Serial number for the BOM (optional but recommended)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,

    /// BOM version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,

    /// Metadata about the BOM (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<CycloneDxMetadata>,

    /// List of components (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<CycloneDxComponent>,
}

/// CycloneDX metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycloneDxMetadata {
    /// Timestamp when the BOM was created (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,

    /// Tools used to create the BOM (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<CycloneDxTools>,

    /// Authors of the BOM (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<CycloneDxContact>,

    /// Component that represents the BOM (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<CycloneDxComponent>,
}

/// CycloneDX tools structure (v1.5 format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycloneDxTools {
    /// Tool components
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<CycloneDxTool>,

    /// Tool services (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<CycloneDxService>,
}

/// CycloneDX service structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycloneDxService {
    /// Service name (required)
    pub name: String,

    /// Service version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// CycloneDX tool structure (individual tool entry)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycloneDxTool {
    /// Component type (required for tools/components)
    #[serde(rename = "type")]
    pub component_type: String,

    /// Tool name (required)
    pub name: String,

    /// Tool version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// CycloneDX contact structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycloneDxContact {
    /// Name of the contact
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Email of the contact
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// CycloneDX component structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycloneDxComponent {
    /// Component type (required)
    #[serde(rename = "type")]
    pub component_type: String, // "library", "application", "framework", etc.

    /// Component name (required)
    pub name: String,

    /// Component version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Component description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Component scope (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>, // "required", "optional", "excluded"

    /// Component licenses (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub licenses: Vec<CycloneDxLicenseChoice>,

    /// Copyright information (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,

    /// Package URL (PURL) (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purl: Option<String>,

    /// External references (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub external_references: Vec<CycloneDxExternalReference>,
}

/// CycloneDX license choice (either a license object or expression string)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CycloneDxLicenseChoice {
    /// License object wrapper
    License { license: CycloneDxLicense },
    /// SPDX license expression wrapper
    Expression { expression: String },
}

/// CycloneDX license object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycloneDxLicense {
    /// SPDX license identifier (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// License name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// License URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// CycloneDX external reference structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycloneDxExternalReference {
    /// Reference type (required)
    #[serde(rename = "type")]
    pub ref_type: String, // "website", "vcs", "distribution", etc.

    /// Reference URL (required)
    pub url: String,

    /// Reference comment (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl CycloneDxBom {
    pub fn new() -> Self {
        let serial_number = format!("urn:uuid:{}", Uuid::new_v4());

        Self {
            bom_format: "CycloneDX".to_string(),
            spec_version: "1.5".to_string(),
            serial_number: Some(serial_number),
            version: Some(1),
            metadata: Some(CycloneDxMetadata {
                timestamp: Some(Utc::now()),
                tools: Some(CycloneDxTools {
                    components: vec![CycloneDxTool {
                        component_type: "application".to_string(),
                        name: "feluda".to_string(),
                        version: Some(env!("CARGO_PKG_VERSION").to_string()),
                    }],
                    services: vec![],
                }),
                authors: vec![],
                component: None,
            }),
            components: Vec::new(),
        }
    }

    pub fn add_component(&mut self, component: CycloneDxComponent) {
        self.components.push(component);
    }
}

impl Default for CycloneDxBom {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert SPDX license to CycloneDX license format
fn convert_spdx_license_to_cyclonedx(spdx_license: &str) -> CycloneDxLicenseChoice {
    // Check if it looks like an SPDX expression (contains AND, OR, WITH)
    if spdx_license.contains(" AND ")
        || spdx_license.contains(" OR ")
        || spdx_license.contains(" WITH ")
    {
        CycloneDxLicenseChoice::Expression {
            expression: spdx_license.to_string(),
        }
    } else if spdx_license == "NOASSERTION" {
        // Handle NOASSERTION case
        CycloneDxLicenseChoice::License {
            license: CycloneDxLicense {
                id: None,
                name: Some("NOASSERTION".to_string()),
                url: None,
            },
        }
    } else {
        // Single license identifier
        CycloneDxLicenseChoice::License {
            license: CycloneDxLicense {
                id: Some(spdx_license.to_string()),
                name: None,
                url: None,
            },
        }
    }
}

/// Convert SPDX document to CycloneDX BOM
pub fn convert_spdx_to_cyclonedx(spdx_doc: &SpdxDocument) -> CycloneDxBom {
    let mut bom = CycloneDxBom::new();

    // Convert each SPDX package to CycloneDX component
    for spdx_package in &spdx_doc.packages {
        let mut component = CycloneDxComponent {
            component_type: "library".to_string(), // Default to library for dependencies
            name: spdx_package.name.clone(),
            version: spdx_package.version_info.clone(),
            description: None,
            scope: Some("required".to_string()), // Default scope
            licenses: Vec::new(),
            copyright: spdx_package.copyright_text.clone(),
            purl: None, // Could be enhanced in the future
            external_references: Vec::new(),
        };

        // Convert licenses
        if let Some(ref license_concluded) = spdx_package.license_concluded {
            component
                .licenses
                .push(convert_spdx_license_to_cyclonedx(license_concluded));
        } else if let Some(ref license_declared) = spdx_package.license_declared {
            component
                .licenses
                .push(convert_spdx_license_to_cyclonedx(license_declared));
        }

        bom.add_component(component);
    }

    log(
        LogLevel::Info,
        &format!(
            "Converted {} SPDX packages to CycloneDX components",
            spdx_doc.packages.len()
        ),
    );

    bom
}

pub fn generate_cyclonedx_output(
    spdx_doc: &SpdxDocument,
    output_file: Option<String>,
) -> FeludaResult<()> {
    log(LogLevel::Info, "Generating CycloneDX 1.5 BOM output");

    // Convert SPDX document to CycloneDX BOM
    let cyclonedx_bom = convert_spdx_to_cyclonedx(spdx_doc);

    // Serialize to JSON
    let json_output = serde_json::to_string_pretty(&cyclonedx_bom).map_err(|e| {
        FeludaError::Serialization(format!("Failed to serialize CycloneDX BOM: {e}"))
    })?;

    // Output to file or stdout
    if let Some(file_path) = output_file {
        let cyclonedx_file = if file_path.ends_with(".json") {
            file_path
        } else {
            format!(
                "{}.cyclonedx.json",
                file_path.trim_end_matches(".cyclonedx")
            )
        };

        std::fs::write(&cyclonedx_file, &json_output)
            .map_err(|e| FeludaError::FileWrite(format!("Failed to write CycloneDX file: {e}")))?;

        println!("🧪 CycloneDX BOM written to: {cyclonedx_file} (EXPERIMENTAL)");
        log(
            LogLevel::Info,
            &format!("CycloneDX BOM written to: {cyclonedx_file}"),
        );
    } else {
        println!("=== CycloneDX BOM (EXPERIMENTAL) ===");
        println!("{json_output}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sbom::spdx::{SpdxDocument, SpdxPackage};

    #[test]
    fn test_cyclonedx_bom_creation() {
        let bom = CycloneDxBom::new();

        assert_eq!(bom.bom_format, "CycloneDX");
        assert_eq!(bom.spec_version, "1.5");
        assert!(bom.serial_number.is_some());
        assert_eq!(bom.version, Some(1));
        assert!(bom.metadata.is_some());
        assert!(bom.components.is_empty());
    }

    #[test]
    fn test_cyclonedx_bom_add_component() {
        let mut bom = CycloneDxBom::new();
        let component = CycloneDxComponent {
            component_type: "library".to_string(),
            name: "test-package".to_string(),
            version: Some("1.0.0".to_string()),
            description: None,
            scope: Some("required".to_string()),
            licenses: Vec::new(),
            copyright: None,
            purl: None,
            external_references: Vec::new(),
        };

        bom.add_component(component);
        assert_eq!(bom.components.len(), 1);
        assert_eq!(bom.components[0].name, "test-package");
    }

    #[test]
    fn test_convert_spdx_license_to_cyclonedx() {
        // Test simple license
        let license = convert_spdx_license_to_cyclonedx("MIT");
        match license {
            CycloneDxLicenseChoice::License { license } => {
                assert_eq!(license.id, Some("MIT".to_string()));
                assert_eq!(license.name, None);
                assert_eq!(license.url, None);
            }
            _ => panic!("Expected License variant"),
        }

        // Test SPDX expression
        let license = convert_spdx_license_to_cyclonedx("MIT OR Apache-2.0");
        match license {
            CycloneDxLicenseChoice::Expression { expression } => {
                assert_eq!(expression, "MIT OR Apache-2.0");
            }
            _ => panic!("Expected Expression variant"),
        }

        // Test NOASSERTION
        let license = convert_spdx_license_to_cyclonedx("NOASSERTION");
        match license {
            CycloneDxLicenseChoice::License { license } => {
                assert_eq!(license.id, None);
                assert_eq!(license.name, Some("NOASSERTION".to_string()));
                assert_eq!(license.url, None);
            }
            _ => panic!("Expected License variant"),
        }
    }

    #[test]
    fn test_convert_spdx_to_cyclonedx() {
        let mut spdx_doc = SpdxDocument::new("test-project");

        // Add a test package
        let package = SpdxPackage::new("test-package".to_string(), &spdx_doc.document_namespace)
            .with_version("1.0.0".to_string())
            .with_license("MIT".to_string());

        spdx_doc.add_package(package);

        let cyclonedx_bom = convert_spdx_to_cyclonedx(&spdx_doc);

        assert_eq!(cyclonedx_bom.bom_format, "CycloneDX");
        assert_eq!(cyclonedx_bom.spec_version, "1.5");
        assert_eq!(cyclonedx_bom.components.len(), 1);

        let component = &cyclonedx_bom.components[0];
        assert_eq!(component.name, "test-package");
        assert_eq!(component.version, Some("1.0.0".to_string()));
        assert_eq!(component.component_type, "library");
        assert_eq!(component.scope, Some("required".to_string()));
        assert!(!component.licenses.is_empty());
    }

    #[test]
    fn test_cyclonedx_serialization() {
        let bom = CycloneDxBom::new();
        let json = serde_json::to_string_pretty(&bom).unwrap();

        // Verify it contains required fields
        assert!(json.contains("\"bomFormat\": \"CycloneDX\""));
        assert!(json.contains("\"specVersion\": \"1.5\""));
        assert!(json.contains("\"serialNumber\""));
        assert!(json.contains("\"metadata\""));
    }

    #[test]
    fn test_cyclonedx_component_serialization() {
        let component = CycloneDxComponent {
            component_type: "library".to_string(),
            name: "test-lib".to_string(),
            version: Some("2.1.0".to_string()),
            description: Some("A test library".to_string()),
            scope: Some("required".to_string()),
            licenses: vec![CycloneDxLicenseChoice::License {
                license: CycloneDxLicense {
                    id: Some("MIT".to_string()),
                    name: None,
                    url: None,
                },
            }],
            copyright: Some("Copyright 2023 Test".to_string()),
            purl: None,
            external_references: Vec::new(),
        };

        let json = serde_json::to_string_pretty(&component).unwrap();

        // Verify serialization
        assert!(json.contains("\"type\": \"library\""));
        assert!(json.contains("\"name\": \"test-lib\""));
        assert!(json.contains("\"version\": \"2.1.0\""));
        assert!(json.contains("\"scope\": \"required\""));
        assert!(json.contains("\"id\": \"MIT\""));
    }

    #[test]
    fn test_cyclonedx_license_choice_variants() {
        // Test License variant
        let license_variant = CycloneDxLicenseChoice::License {
            license: CycloneDxLicense {
                id: Some("Apache-2.0".to_string()),
                name: None,
                url: Some("https://opensource.org/licenses/Apache-2.0".to_string()),
            },
        };

        let json = serde_json::to_string(&license_variant).unwrap();
        assert!(json.contains("\"id\":\"Apache-2.0\""));
        assert!(json.contains("\"url\":\"https://opensource.org/licenses/Apache-2.0\""));

        // Test Expression variant
        let expression_variant = CycloneDxLicenseChoice::Expression {
            expression: "MIT AND Apache-2.0".to_string(),
        };

        let json = serde_json::to_string(&expression_variant).unwrap();
        assert!(json.contains("\"expression\":\"MIT AND Apache-2.0\""));
    }

    #[test]
    fn test_cyclonedx_metadata_with_tools() {
        let bom = CycloneDxBom::new();
        let metadata = bom.metadata.unwrap();

        assert!(metadata.timestamp.is_some());
        assert!(metadata.tools.is_some());

        let tools = metadata.tools.unwrap();
        assert!(!tools.components.is_empty());
        assert!(tools.services.is_empty());

        let tool = &tools.components[0];
        assert_eq!(tool.name, "feluda");
        assert!(tool.version.is_some());
        assert_eq!(tool.component_type, "application");
    }

    #[test]
    fn test_complex_spdx_to_cyclonedx_conversion() {
        let mut spdx_doc = SpdxDocument::new("complex-project");

        // Add multiple packages with different license formats
        let packages = vec![
            ("simple-mit", "1.0.0", "MIT"),
            ("dual-license", "2.1.0", "MIT OR Apache-2.0"),
            (
                "complex-expr",
                "3.0.0",
                "(MIT OR Apache-2.0) AND BSD-3-Clause",
            ),
            ("no-license", "0.1.0", "NOASSERTION"),
        ];

        for (name, version, license) in packages {
            let package = SpdxPackage::new(name.to_string(), &spdx_doc.document_namespace)
                .with_version(version.to_string())
                .with_license(license.to_string());
            spdx_doc.add_package(package);
        }

        let cyclonedx_bom = convert_spdx_to_cyclonedx(&spdx_doc);

        assert_eq!(cyclonedx_bom.components.len(), 4);

        // Verify each component was converted correctly
        for component in &cyclonedx_bom.components {
            assert_eq!(component.component_type, "library");
            assert_eq!(component.scope, Some("required".to_string()));

            match component.name.as_str() {
                "simple-mit" => {
                    assert!(!component.licenses.is_empty());
                    if let CycloneDxLicenseChoice::License { license } = &component.licenses[0] {
                        assert_eq!(license.id, Some("MIT".to_string()));
                    }
                }
                "dual-license" => {
                    assert!(!component.licenses.is_empty());
                    if let CycloneDxLicenseChoice::Expression { expression } =
                        &component.licenses[0]
                    {
                        assert_eq!(expression, "MIT OR Apache-2.0");
                    }
                }
                "complex-expr" => {
                    assert!(!component.licenses.is_empty());
                    if let CycloneDxLicenseChoice::Expression { expression } =
                        &component.licenses[0]
                    {
                        assert_eq!(expression, "(MIT OR Apache-2.0) AND BSD-3-Clause");
                    }
                }
                "no-license" => {
                    assert!(!component.licenses.is_empty());
                    if let CycloneDxLicenseChoice::License { license } = &component.licenses[0] {
                        assert_eq!(license.name, Some("NOASSERTION".to_string()));
                    }
                }
                _ => panic!("Unexpected component name: {}", component.name),
            }
        }
    }
}
