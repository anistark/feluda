pub mod cyclonedx;
pub mod ingest;
pub mod spdx;
pub mod validate;

use crate::cli::SbomFormat;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::filesystem::scan_filesystem;
use crate::licenses::LicenseCompatibility;
use crate::parser::parse_root;

use cyclonedx::generate_cyclonedx_output;
use serde_json::Value as JsonValue;
use spdx::{generate_spdx_output, SpdxDocument, SpdxPackage};

/// Which SBOM standard a document follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SbomType {
    Spdx,
    CycloneDx,
}

impl SbomType {
    /// The message shown when a document matches neither standard. Shared so `sbom validate` and
    /// `--sbom-input` fail the same way on the same file.
    pub const DETECTION_FAILURE: &'static str =
        "Could not detect SBOM type. File is neither SPDX nor CycloneDX.";
}

/// Detect which standard a parsed JSON document follows, by the keys only that standard defines.
pub fn detect_sbom_type_in(json: &JsonValue) -> Option<SbomType> {
    let obj = json.as_object()?;
    if obj.contains_key("spdxVersion") || obj.contains_key("SPDXID") {
        return Some(SbomType::Spdx);
    }
    if obj.contains_key("bomFormat") || obj.contains_key("specVersion") {
        return Some(SbomType::CycloneDx);
    }
    None
}

/// Generate an SBOM from a project tree, or from the packages installed under `filesystem`.
///
/// The two sources produce the same `Vec<LicenseInfo>`, so everything below this point is written
/// once: a document describing a root filesystem is built exactly like one describing a project.
pub fn handle_sbom_command(
    path: String,
    filesystem: Option<String>,
    format: &SbomFormat,
    output_file: Option<String>,
) -> FeludaResult<()> {
    let source = filesystem.as_deref().unwrap_or(&path);
    log(
        LogLevel::Info,
        &format!("Generating SBOM for path: {source}"),
    );

    let analyzed_data = match &filesystem {
        Some(root) => scan_filesystem(std::path::Path::new(root), false)?,
        None => parse_root(&path, None, false, false)
            .map_err(|e| FeludaError::Parser(format!("Failed to parse dependencies: {e}")))?,
    };

    log(
        LogLevel::Info,
        &format!("Found {} dependencies", analyzed_data.len()),
    );

    // Extract project name from path
    let project_name = std::path::Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");

    // Convert to SPDX-compliant format
    let mut spdx_doc = SpdxDocument::new(project_name);

    for dependency in analyzed_data {
        let mut package = SpdxPackage::new(dependency.name.clone(), &spdx_doc.document_namespace)
            .with_version(dependency.version.clone());

        // The PURL is what keeps packages distinct across ecosystems, so it also supplies the
        // package's SPDX identifier.
        if let Some(purl) = dependency.purl() {
            package = package.with_purl(purl);
        }

        let force_noassertion = std::env::var("FELUDA_FORCE_NOASSERTION_LICENSES")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let license_str = if force_noassertion {
            log(
                LogLevel::Info,
                "Forcing all licenses to NOASSERTION due to environment variable",
            );
            "NOASSERTION"
        } else {
            dependency.license.as_deref().unwrap_or("NOASSERTION")
        };

        package = package.with_license(license_str);

        // TODO: Store Feluda-specific data as SPDX annotations
        let _compatibility_info = format!(
            "License compatibility: {}, Restrictive: {}",
            match dependency.compatibility {
                LicenseCompatibility::Compatible => "compatible",
                LicenseCompatibility::Incompatible => "incompatible",
                LicenseCompatibility::Unknown => "unknown",
            },
            dependency.is_restrictive
        );

        // TODO: Add dependency relationships to SPDX when LicenseInfo supports it

        spdx_doc.add_package(package);
    }

    log(
        LogLevel::Info,
        &format!(
            "Generated SPDX document with {} packages",
            spdx_doc.packages.len()
        ),
    );

    // Generate output based on format
    match format {
        SbomFormat::Spdx => {
            generate_spdx_output(&spdx_doc, output_file)?;
        }
        SbomFormat::Cyclonedx => {
            generate_cyclonedx_output(&spdx_doc, output_file)?;
        }
        SbomFormat::All => {
            generate_spdx_output(&spdx_doc, output_file.clone())?;
            generate_cyclonedx_output(&spdx_doc, output_file)?;
        }
    }

    Ok(())
}
