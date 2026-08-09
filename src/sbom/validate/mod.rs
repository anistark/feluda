use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::sbom::{detect_sbom_type_in, SbomType};
use serde_json::Value as JsonValue;
use std::fs;

mod cyclonedx_validator;
mod parser;
mod reporter;
mod spdx_validator;

fn detect_sbom_type(content: &str) -> FeludaResult<SbomType> {
    let json: JsonValue = serde_json::from_str(content)
        .map_err(|e| FeludaError::Validation(format!("Failed to parse JSON: {e}")))?;
    detect_sbom_type_in(&json)
        .ok_or_else(|| FeludaError::Validation(SbomType::DETECTION_FAILURE.to_string()))
}

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
    let json: JsonValue = serde_json::from_str(&content)
        .map_err(|e| FeludaError::Validation(format!("Invalid JSON: {e}")))?;

    log(LogLevel::Info, "Detecting SBOM type");
    let sbom_type = detect_sbom_type(&content)?;
    log(
        LogLevel::Info,
        &format!("Detected SBOM type: {sbom_type:?}"),
    );

    let validation_report = match sbom_type {
        SbomType::Spdx => {
            log(LogLevel::Info, "Running SPDX validation");
            spdx_validator::validate(&json)?
        }
        SbomType::CycloneDx => {
            log(LogLevel::Info, "Running CycloneDX validation");
            cyclonedx_validator::validate(&json)?
        }
    };

    validation_report.write_output(json_output, output)?;

    Ok(())
}
