pub mod cyclonedx;
pub mod spdx;

use crate::cli::SbomFormat;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::licenses::LicenseCompatibility;
use crate::parser::parse_root;

use cyclonedx::generate_cyclonedx_output;
use spdx::{generate_spdx_output, SpdxDocument, SpdxPackage};

pub fn handle_sbom_command(
    path: String,
    format: &SbomFormat,
    output_file: Option<String>,
) -> FeludaResult<()> {
    log(LogLevel::Info, &format!("Generating SBOM for path: {path}"));

    // Parse project dependencies using existing parser
    let analyzed_data = parse_root(&path, None)
        .map_err(|e| FeludaError::Parser(format!("Failed to parse dependencies: {e}")))?;

    log(
        LogLevel::Info,
        &format!("Found {} dependencies", analyzed_data.len()),
    );

    // Extract project name from path
    let project_name = std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");

    // Convert to SPDX-compliant format
    let mut spdx_doc = SpdxDocument::new(project_name);

    for dependency in analyzed_data {
        let mut package = SpdxPackage::new(dependency.name.clone(), &spdx_doc.document_namespace)
            .with_version(dependency.version.clone());

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

        package = package.with_license(license_str.to_string());

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
