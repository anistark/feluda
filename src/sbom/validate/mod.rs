use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::sbom::input::{read_sbom, Original, Serialization};
use std::fs;

mod cyclonedx_validator;
mod parser;
mod reporter;
mod spdx3_validator;
mod spdx_validator;

pub fn handle_sbom_validate_command(
    sbom_file: String,
    output: Option<String>,
    json_output: bool,
) -> FeludaResult<()> {
    log(
        LogLevel::Info,
        &format!("Validating SBOM file: {sbom_file}"),
    );

    let content = fs::read_to_string(&sbom_file)
        .map_err(|_| FeludaError::Validation(format!("Failed to read SBOM file: {sbom_file}")))?;

    log(LogLevel::Info, "Parsing SBOM file");
    // Errors returned from `run()` only print under `--debug`.
    let document = read_sbom(&content).map_err(|e| {
        eprintln!("❌ {e}");
        FeludaError::Validation(e)
    })?;
    log(
        LogLevel::Info,
        &format!(
            "Detected SBOM serialization: {}",
            document.serialization.describe()
        ),
    );

    // Tag:value and XML are checked in the JSON shape they were read into, which carries the
    // same fields; SPDX 3.0 is a graph, so it has its own checks.
    let mut validation_report = match (&document.original, document.serialization) {
        (Original::Spdx3(graph), _) => spdx3_validator::validate(graph)?,
        (_, Serialization::SpdxJson | Serialization::SpdxTagValue) => {
            spdx_validator::validate(&document.model)?
        }
        _ => cyclonedx_validator::validate(&document.model)?,
    };
    if matches!(
        document.serialization,
        Serialization::SpdxTagValue | Serialization::CycloneDxXml
    ) {
        validation_report.sbom_type = document.serialization.describe().to_string();
    }

    validation_report.write_output(json_output, output)?;

    Ok(())
}
