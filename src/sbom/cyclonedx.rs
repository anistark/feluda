use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::sbom::spdx::{SbomKind, SpdxDocument};
use crate::sbom::{CycloneDxFormat, CycloneDxVersion};

/// CycloneDX BOM structure, for every version feluda writes (1.4 to 1.7)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycloneDxBom {
    /// BOM format identifier (required)
    pub bom_format: String, // "CycloneDX"

    /// Specification version (required)
    pub spec_version: String, // "1.6"

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

    /// Which stage of the product's life the BOM describes, from 1.5
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lifecycles: Vec<CycloneDxLifecycle>,

    /// Tools used to create the BOM (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<CycloneDxToolsChoice>,

    /// Authors of the BOM (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<CycloneDxContact>,

    /// Component that represents the BOM (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<CycloneDxComponent>,
}

/// A lifecycle phase the BOM was made in, from 1.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CycloneDxLifecycle {
    /// `design`, `pre-build`, `build`, `post-build`, `operations`, `discovery` or `decommission`
    pub phase: String,
}

/// The two shapes `metadata.tools` has taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CycloneDxToolsChoice {
    /// 1.5 and later: tools are components and services
    Nested(CycloneDxTools),
    /// 1.4 and earlier: a plain list, deprecated from 1.5
    Legacy(Vec<CycloneDxLegacyTool>),
}

/// A tool as CycloneDX 1.4 and earlier list it
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycloneDxLegacyTool {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,

    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// CycloneDX tools structure (1.5 and later)
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
    Expression {
        expression: String,
        /// `declared` or `concluded`, from 1.6
        #[serde(skip_serializing_if = "Option::is_none")]
        acknowledgement: Option<String>,
    },
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

    /// `declared` or `concluded`, from 1.6
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledgement: Option<String>,
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
        Self::for_version(CycloneDxVersion::default())
    }

    pub fn for_version(spec_version: CycloneDxVersion) -> Self {
        let serial_number = format!("urn:uuid:{}", Uuid::new_v4());
        let feluda_version = Some(env!("CARGO_PKG_VERSION").to_string());

        // 1.5 deprecated the plain tool list for components and services.
        let tools = if spec_version >= CycloneDxVersion::V1_5 {
            CycloneDxToolsChoice::Nested(CycloneDxTools {
                components: vec![CycloneDxTool {
                    component_type: "application".to_string(),
                    name: "feluda".to_string(),
                    version: feluda_version,
                }],
                services: vec![],
            })
        } else {
            CycloneDxToolsChoice::Legacy(vec![CycloneDxLegacyTool {
                vendor: None,
                name: "feluda".to_string(),
                version: feluda_version,
            }])
        };

        Self {
            bom_format: "CycloneDX".to_string(),
            spec_version: spec_version.as_str().to_string(),
            serial_number: Some(serial_number),
            version: Some(1),
            metadata: Some(CycloneDxMetadata {
                timestamp: Some(Utc::now()),
                lifecycles: Vec::new(),
                tools: Some(tools),
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
///
/// CycloneDX validates `license.id` against the SPDX license list, so only a listed id is written
/// as one, in the list's spelling. Anything else feluda resolved (a registry title, `SEE LICENSE
/// IN LICENSE.txt`, an id newer than the bundled list) is written as `license.name`, which takes
/// any text. An expression is written as one only when every license in it is listed, for the
/// same reason.
///
/// `acknowledgement` is CycloneDX 1.6's `declared` or `concluded`, or `None` for earlier versions.
/// `NOASSERTION` never carries one, since it states that no license was found.
pub fn convert_spdx_license_to_cyclonedx(
    spdx_license: &str,
    acknowledgement: Option<&str>,
) -> CycloneDxLicenseChoice {
    let acknowledgement = acknowledgement.map(str::to_string);
    let named = |name: &str, acknowledgement: Option<String>| CycloneDxLicenseChoice::License {
        license: CycloneDxLicense {
            id: None,
            name: Some(name.to_string()),
            url: None,
            acknowledgement,
        },
    };

    let expression = crate::spdx::is_compound(spdx_license)
        .then(|| crate::spdx::listed_expression(spdx_license))
        .flatten();

    if spdx_license == "NOASSERTION" {
        named("NOASSERTION", None)
    } else if let Some(expression) = expression {
        CycloneDxLicenseChoice::Expression {
            expression,
            acknowledgement,
        }
    } else if let Some(id) = crate::spdx::listed_id(spdx_license) {
        CycloneDxLicenseChoice::License {
            license: CycloneDxLicense {
                id: Some(id.to_string()),
                name: None,
                url: None,
                acknowledgement,
            },
        }
    } else {
        named(spdx_license, acknowledgement)
    }
}

/// Read a package's PURL back out of its SPDX package-manager external reference.
///
/// CycloneDX documents are produced by converting the SPDX document rather than from the analyzed
/// dependencies directly, so the external reference written by `SpdxPackage::with_purl` is where
/// the coordinate lives by the time we get here.
fn purl_of(spdx_package: &crate::sbom::spdx::SpdxPackage) -> Option<String> {
    spdx_package
        .external_refs
        .iter()
        .find(|reference| reference.reference_type == "purl")
        .map(|reference| reference.reference_locator.clone())
}

/// Convert SPDX document to CycloneDX BOM
pub fn convert_spdx_to_cyclonedx(
    spdx_doc: &SpdxDocument,
    spec_version: CycloneDxVersion,
) -> CycloneDxBom {
    let mut bom = CycloneDxBom::for_version(spec_version);
    let acknowledges = spec_version >= CycloneDxVersion::V1_6;

    // CISA's SBOM types map onto CycloneDX phases: a source SBOM is made before the build, an
    // analyzed one by looking inside what the build produced.
    if let (Some(kind), Some(metadata)) = (spdx_doc.sbom_type, bom.metadata.as_mut()) {
        if spec_version >= CycloneDxVersion::V1_5 {
            let phase = match kind {
                SbomKind::Source => "pre-build",
                SbomKind::Analyzed => "post-build",
            };
            metadata.lifecycles.push(CycloneDxLifecycle {
                phase: phase.to_string(),
            });
        }
    }

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
            purl: purl_of(spdx_package),
            external_references: Vec::new(),
        };

        // The declared license comes first: what feluda reports is what the package states in
        // its manifest, registry entry or license file, which is CycloneDX's `declared`. A
        // package with only a conclusion says so.
        let stated = spdx_package
            .license_declared
            .as_deref()
            .map(|license| (license, "declared"))
            .or_else(|| {
                spdx_package
                    .license_concluded
                    .as_deref()
                    .map(|license| (license, "concluded"))
            });
        if let Some((license, acknowledgement)) = stated {
            component.licenses.push(convert_spdx_license_to_cyclonedx(
                license,
                acknowledges.then_some(acknowledgement),
            ));
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
    spec_version: CycloneDxVersion,
    format: CycloneDxFormat,
    output_file: Option<String>,
) -> FeludaResult<()> {
    log(
        LogLevel::Info,
        &format!(
            "Generating CycloneDX {} BOM output as {format:?}",
            spec_version.as_str()
        ),
    );

    // Convert SPDX document to CycloneDX BOM
    let cyclonedx_bom = convert_spdx_to_cyclonedx(spdx_doc, spec_version);

    let (output, extension) = match format {
        CycloneDxFormat::Json => (
            serde_json::to_string_pretty(&cyclonedx_bom).map_err(|e| {
                FeludaError::Serialization(format!("Failed to serialize CycloneDX BOM: {e}"))
            })?,
            ".json",
        ),
        CycloneDxFormat::Xml => (super::cyclonedx_xml::write(&cyclonedx_bom), ".xml"),
    };

    // Output to file or stdout
    if let Some(file_path) = output_file {
        let cyclonedx_file = if file_path.ends_with(extension) {
            file_path
        } else {
            format!(
                "{}.cyclonedx{extension}",
                file_path.trim_end_matches(".cyclonedx")
            )
        };

        std::fs::write(&cyclonedx_file, &output)
            .map_err(|e| FeludaError::FileWrite(format!("Failed to write CycloneDX file: {e}")))?;

        println!("🧪 CycloneDX BOM written to: {cyclonedx_file} (EXPERIMENTAL)");
        log(
            LogLevel::Info,
            &format!("CycloneDX BOM written to: {cyclonedx_file}"),
        );
    } else {
        println!("=== CycloneDX BOM (EXPERIMENTAL) ===");
        println!("{output}");
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
        assert_eq!(bom.spec_version, "1.6");
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
        let license = convert_spdx_license_to_cyclonedx("MIT", None);
        match license {
            CycloneDxLicenseChoice::License { license } => {
                assert_eq!(license.id, Some("MIT".to_string()));
                assert_eq!(license.name, None);
                assert_eq!(license.url, None);
            }
            _ => panic!("Expected License variant"),
        }

        // Test SPDX expression
        let license = convert_spdx_license_to_cyclonedx("MIT OR Apache-2.0", None);
        match license {
            CycloneDxLicenseChoice::Expression { expression, .. } => {
                assert_eq!(expression, "MIT OR Apache-2.0");
            }
            _ => panic!("Expected Expression variant"),
        }

        // Test NOASSERTION
        let license = convert_spdx_license_to_cyclonedx("NOASSERTION", None);
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
        let package = SpdxPackage::new("test-package", &spdx_doc.document_namespace)
            .with_version("1.0.0")
            .with_license("MIT");

        spdx_doc.add_package(package);

        let cyclonedx_bom = convert_spdx_to_cyclonedx(&spdx_doc, CycloneDxVersion::V1_6);

        assert_eq!(cyclonedx_bom.bom_format, "CycloneDX");
        assert_eq!(cyclonedx_bom.spec_version, "1.6");
        assert_eq!(cyclonedx_bom.components.len(), 1);

        let component = &cyclonedx_bom.components[0];
        assert_eq!(component.name, "test-package");
        assert_eq!(component.version, Some("1.0.0".to_string()));
        assert_eq!(component.component_type, "library");
        assert_eq!(component.scope, Some("required".to_string()));
        assert!(!component.licenses.is_empty());
    }

    #[test]
    fn test_long_expression_survives_into_cyclonedx() {
        // #257: CycloneDX converts from the SPDX document, so a length cap there reached here as
        // a license named NOASSERTION.
        let expression = "MIT AND Apache-2.0 AND BSD-3-Clause AND ISC AND Zlib AND MPL-2.0 AND GPL-2.0-or-later AND LGPL-2.1-or-later AND CC0-1.0";
        let mut spdx_doc = SpdxDocument::new("test-project");
        spdx_doc.add_package(
            SpdxPackage::new("longlic", &spdx_doc.document_namespace)
                .with_version("1.0.0")
                .with_license(expression),
        );

        let bom = convert_spdx_to_cyclonedx(&spdx_doc, CycloneDxVersion::V1_6);

        match &bom.components[0].licenses[..] {
            [CycloneDxLicenseChoice::Expression {
                expression: written,
                ..
            }] => {
                assert_eq!(written, expression)
            }
            other => panic!("expected the expression, got {other:?}"),
        }
    }

    #[test]
    fn test_component_carries_the_package_purl() {
        let mut spdx_doc = SpdxDocument::new("test-project");
        spdx_doc.add_package(
            SpdxPackage::new("errors", &spdx_doc.document_namespace)
                .with_version("v0.9.1")
                .with_purl("pkg:golang/github.com/pkg/errors@v0.9.1")
                .with_license("BSD-2-Clause"),
        );

        let bom = convert_spdx_to_cyclonedx(&spdx_doc, CycloneDxVersion::V1_6);

        assert_eq!(
            bom.components[0].purl.as_deref(),
            Some("pkg:golang/github.com/pkg/errors@v0.9.1")
        );

        let json = serde_json::to_string(&bom).unwrap();
        assert!(json.contains("\"purl\":\"pkg:golang/github.com/pkg/errors@v0.9.1\""));
    }

    #[test]
    fn test_component_without_a_purl_omits_the_field() {
        let mut spdx_doc = SpdxDocument::new("test-project");
        spdx_doc.add_package(
            SpdxPackage::new("mystery", &spdx_doc.document_namespace).with_version("1.0.0"),
        );

        let bom = convert_spdx_to_cyclonedx(&spdx_doc, CycloneDxVersion::V1_6);

        assert_eq!(bom.components[0].purl, None);
        assert!(!serde_json::to_string(&bom).unwrap().contains("\"purl\""));
    }

    #[test]
    fn test_cyclonedx_serialization() {
        let bom = CycloneDxBom::new();
        let json = serde_json::to_string_pretty(&bom).unwrap();

        // Verify it contains required fields
        assert!(json.contains("\"bomFormat\": \"CycloneDX\""));
        assert!(json.contains("\"specVersion\": \"1.6\""));
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
                    acknowledgement: None,
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
                acknowledgement: None,
            },
        };

        let json = serde_json::to_string(&license_variant).unwrap();
        assert!(json.contains("\"id\":\"Apache-2.0\""));
        assert!(json.contains("\"url\":\"https://opensource.org/licenses/Apache-2.0\""));

        // Test Expression variant
        let expression_variant = CycloneDxLicenseChoice::Expression {
            expression: "MIT AND Apache-2.0".to_string(),
            acknowledgement: None,
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

        let Some(CycloneDxToolsChoice::Nested(tools)) = metadata.tools else {
            panic!("1.6 nests tools as components");
        };
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
            let package = SpdxPackage::new(name, &spdx_doc.document_namespace)
                .with_version(version)
                .with_license(license);
            spdx_doc.add_package(package);
        }

        let cyclonedx_bom = convert_spdx_to_cyclonedx(&spdx_doc, CycloneDxVersion::V1_6);

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
                    if let CycloneDxLicenseChoice::Expression { expression, .. } =
                        &component.licenses[0]
                    {
                        assert_eq!(expression, "MIT OR Apache-2.0");
                    }
                }
                "complex-expr" => {
                    assert!(!component.licenses.is_empty());
                    if let CycloneDxLicenseChoice::Expression { expression, .. } =
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

    fn single_package_document(license: &str) -> SpdxDocument {
        let mut spdx_doc = SpdxDocument::new("test-project");
        spdx_doc.add_package(
            SpdxPackage::new("lib", &spdx_doc.document_namespace)
                .with_version("1.0.0")
                .with_license(license),
        );
        spdx_doc
    }

    #[test]
    fn test_spec_version_is_the_one_asked_for() {
        for version in [
            CycloneDxVersion::V1_4,
            CycloneDxVersion::V1_5,
            CycloneDxVersion::V1_6,
            CycloneDxVersion::V1_7,
        ] {
            let bom = convert_spdx_to_cyclonedx(&single_package_document("MIT"), version);
            assert_eq!(bom.spec_version, version.as_str());
        }
    }

    #[test]
    fn test_tools_take_the_shape_of_their_version() {
        let legacy =
            serde_json::to_value(CycloneDxBom::for_version(CycloneDxVersion::V1_4)).unwrap();
        let tools = &legacy["metadata"]["tools"];
        assert!(tools.is_array(), "1.4 lists tools directly: {tools}");
        assert_eq!(tools[0]["name"], "feluda");
        assert!(tools[0].get("type").is_none());

        let nested =
            serde_json::to_value(CycloneDxBom::for_version(CycloneDxVersion::V1_5)).unwrap();
        let tools = &nested["metadata"]["tools"];
        assert_eq!(tools["components"][0]["name"], "feluda");
        assert_eq!(tools["components"][0]["type"], "application");
    }

    #[test]
    fn test_acknowledgement_is_written_from_1_6() {
        for (license, field) in [("MIT", "license"), ("MIT OR Apache-2.0", "expression")] {
            let document = single_package_document(license);

            for version in [CycloneDxVersion::V1_4, CycloneDxVersion::V1_5] {
                let bom =
                    serde_json::to_value(convert_spdx_to_cyclonedx(&document, version)).unwrap();
                assert!(
                    !bom.to_string().contains("acknowledgement"),
                    "{} has no acknowledgement: {bom}",
                    version.as_str()
                );
            }

            for version in [CycloneDxVersion::V1_6, CycloneDxVersion::V1_7] {
                let bom =
                    serde_json::to_value(convert_spdx_to_cyclonedx(&document, version)).unwrap();
                let entry = &bom["components"][0]["licenses"][0];
                let acknowledgement = if field == "license" {
                    &entry["license"]["acknowledgement"]
                } else {
                    &entry["acknowledgement"]
                };
                assert_eq!(acknowledgement, "declared", "{}: {entry}", version.as_str());
            }
        }
    }

    #[test]
    fn test_noassertion_is_never_acknowledged() {
        let bom = convert_spdx_to_cyclonedx(
            &single_package_document("NOASSERTION"),
            CycloneDxVersion::V1_6,
        );
        match &bom.components[0].licenses[..] {
            [CycloneDxLicenseChoice::License { license }] => {
                assert_eq!(license.name.as_deref(), Some("NOASSERTION"));
                assert_eq!(license.acknowledgement, None);
            }
            other => panic!("expected a NOASSERTION license, got {other:?}"),
        }
    }

    #[test]
    fn test_a_concluded_only_license_says_so() {
        let mut spdx_doc = single_package_document("MIT");
        spdx_doc.packages[0].license_declared = None;

        let bom = convert_spdx_to_cyclonedx(&spdx_doc, CycloneDxVersion::V1_6);
        match &bom.components[0].licenses[..] {
            [CycloneDxLicenseChoice::License { license }] => {
                assert_eq!(license.acknowledgement.as_deref(), Some("concluded"));
            }
            other => panic!("expected one license, got {other:?}"),
        }
    }

    #[test]
    fn test_only_listed_ids_are_written_as_ids() {
        let id_of = |license: &str| match convert_spdx_license_to_cyclonedx(license, None) {
            CycloneDxLicenseChoice::License { license } => (license.id, license.name),
            other => panic!("expected a license for {license}, got {other:?}"),
        };

        // The list's spelling, whatever case the package used.
        assert_eq!(id_of("mit"), (Some("MIT".to_string()), None));
        // Free form text and ids the list does not carry are names, which take any text.
        for free_form in [
            "SEE LICENSE IN LICENSE.txt",
            "Custom-1.0",
            "LicenseRef-acme",
        ] {
            assert_eq!(id_of(free_form), (None, Some(free_form.to_string())));
        }
    }

    #[test]
    fn test_an_expression_over_unlisted_licenses_is_a_name() {
        match convert_spdx_license_to_cyclonedx("Apache License 2.0 OR MIT", Some("declared")) {
            CycloneDxLicenseChoice::License { license } => {
                assert_eq!(license.id, None);
                assert_eq!(license.name.as_deref(), Some("Apache License 2.0 OR MIT"));
                assert_eq!(license.acknowledgement.as_deref(), Some("declared"));
            }
            other => panic!("expected a named license, got {other:?}"),
        }
    }

    #[test]
    fn test_lifecycle_follows_what_the_sbom_was_made_from() {
        let mut document = single_package_document("MIT");
        let lifecycles = |document: &SpdxDocument, version| {
            serde_json::to_value(convert_spdx_to_cyclonedx(document, version)).unwrap()["metadata"]
                .get("lifecycles")
                .cloned()
        };

        // Nothing said what the inventory was made from, so nothing is claimed.
        assert_eq!(lifecycles(&document, CycloneDxVersion::V1_6), None);

        document.sbom_type = Some(SbomKind::Source);
        assert_eq!(
            lifecycles(&document, CycloneDxVersion::V1_6),
            Some(serde_json::json!([{ "phase": "pre-build" }]))
        );
        document.sbom_type = Some(SbomKind::Analyzed);
        assert_eq!(
            lifecycles(&document, CycloneDxVersion::V1_5),
            Some(serde_json::json!([{ "phase": "post-build" }]))
        );
        // 1.4 has no lifecycles.
        assert_eq!(lifecycles(&document, CycloneDxVersion::V1_4), None);
    }
}
