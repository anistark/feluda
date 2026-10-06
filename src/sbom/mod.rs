pub mod cyclonedx;
pub mod ingest;
pub mod spdx;
pub mod validate;

use crate::cli::SbomFormat;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::filesystem::scan_filesystem;
use crate::image::{scan_image, ImageArchive};
use crate::licenses::LicenseCompatibility;
use crate::parser::parse_root;
use clap::ValueEnum;

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

/// The SPDX version an SBOM is written in.
///
/// feluda builds one SPDX 2.3 document and writes it down to 2.2 on the way out, so a new version
/// is a difference in `spdx::generate_spdx_output`, not a new model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum SpdxVersion {
    #[value(name = "2.2")]
    V2_2,
    #[default]
    #[value(name = "2.3")]
    V2_3,
}

impl SpdxVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            SpdxVersion::V2_2 => "2.2",
            SpdxVersion::V2_3 => "2.3",
        }
    }
}

/// The CycloneDX version an SBOM is written in.
///
/// 1.6 is the default because it is what syft, Trivy and cdxgen write, so a feluda BOM drops into
/// the same pipelines theirs do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, ValueEnum)]
pub enum CycloneDxVersion {
    #[value(name = "1.4")]
    V1_4,
    #[value(name = "1.5")]
    V1_5,
    #[default]
    #[value(name = "1.6")]
    V1_6,
    #[value(name = "1.7")]
    V1_7,
}

impl CycloneDxVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            CycloneDxVersion::V1_4 => "1.4",
            CycloneDxVersion::V1_5 => "1.5",
            CycloneDxVersion::V1_6 => "1.6",
            CycloneDxVersion::V1_7 => "1.7",
        }
    }
}

/// Both versions are named the way the specs name them (`"2.3"`, `"1.6"`), in `.feluda.toml` and
/// on the command line alike. A version written unquoted in TOML, or set through `FELUDA_SBOM_*`,
/// arrives as a number, so numbers are read as their decimal spelling.
macro_rules! spec_version_serde {
    ($version:ty, $standard:literal) => {
        impl serde::Serialize for $version {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $version {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[derive(serde::Deserialize)]
                #[serde(untagged)]
                enum Spelling {
                    Text(String),
                    Number(f64),
                }

                let spelling = match Spelling::deserialize(deserializer)? {
                    Spelling::Text(text) => text,
                    Spelling::Number(number) => number.to_string(),
                };
                <$version as ValueEnum>::from_str(spelling.trim(), true).map_err(|_| {
                    let supported: Vec<&str> = <$version as ValueEnum>::value_variants()
                        .iter()
                        .map(|version| version.as_str())
                        .collect();
                    serde::de::Error::custom(format!(
                        "unsupported {} version '{spelling}', expected one of {}",
                        $standard,
                        supported.join(", ")
                    ))
                })
            }
        }
    };
}

spec_version_serde!(SpdxVersion, "SPDX");
spec_version_serde!(CycloneDxVersion, "CycloneDX");

/// Which version of each standard an SBOM run writes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpecVersions {
    pub spdx: SpdxVersion,
    pub cyclonedx: CycloneDxVersion,
}

impl SpecVersions {
    /// Settle the versions for a run: a command line flag, then `[sbom]` in `.feluda.toml` or its
    /// `FELUDA_SBOM_*` variables, then the defaults.
    pub fn resolve(
        spdx: Option<SpdxVersion>,
        cyclonedx: Option<CycloneDxVersion>,
    ) -> FeludaResult<Self> {
        let configured = crate::config::load_config()
            .map_err(|e| {
                // Errors returned from `run()` only print under `--debug`, and a mistyped version
                // in the config is something the user has to be told about.
                eprintln!("❌ {e}");
                e
            })?
            .sbom;
        Ok(Self {
            spdx: spdx.or(configured.spdx).unwrap_or_default(),
            cyclonedx: cyclonedx.or(configured.cyclonedx).unwrap_or_default(),
        })
    }
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

/// Generate an SBOM from a project tree, from the packages installed under `filesystem`, or from
/// the image in `image_archive`.
///
/// The three sources produce the same `Vec<LicenseInfo>`, so everything below this point is
/// written once: a document describing a root filesystem or an image is built exactly like one
/// describing a project.
pub fn handle_sbom_command(
    path: String,
    filesystem: Option<String>,
    image_archive: Option<ImageArchive>,
    format: &SbomFormat,
    versions: SpecVersions,
    output_file: Option<String>,
) -> FeludaResult<()> {
    let source = filesystem
        .as_deref()
        .or(image_archive.as_ref().map(|archive| archive.path.as_str()))
        .unwrap_or(&path);
    log(
        LogLevel::Info,
        &format!("Generating SBOM for path: {source}"),
    );

    let analyzed_data = match (&filesystem, &image_archive) {
        (Some(root), _) => scan_filesystem(std::path::Path::new(root), false)?,
        (None, Some(archive)) => scan_image(archive, false)?,
        (None, None) => {
            let mut analyzed_data = parse_root(&path, None, false, false)
                .map_err(|e| FeludaError::Parser(format!("Failed to parse dependencies: {e}")))?;
            crate::clearlydefined::resolve_unknown_licenses(&mut analyzed_data, false);
            analyzed_data
        }
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
            generate_spdx_output(&spdx_doc, versions.spdx, output_file)?;
        }
        SbomFormat::Cyclonedx => {
            generate_cyclonedx_output(&spdx_doc, versions.cyclonedx, output_file)?;
        }
        SbomFormat::All => {
            generate_spdx_output(&spdx_doc, versions.spdx, output_file.clone())?;
            generate_cyclonedx_output(&spdx_doc, versions.cyclonedx, output_file)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_read_from_text_or_numbers() {
        let spdx: SpdxVersion = serde_json::from_str("\"2.2\"").unwrap();
        assert_eq!(spdx, SpdxVersion::V2_2);
        let spdx: SpdxVersion = serde_json::from_str("2.3").unwrap();
        assert_eq!(spdx, SpdxVersion::V2_3);

        let cyclonedx: CycloneDxVersion = serde_json::from_str("\" 1.7 \"").unwrap();
        assert_eq!(cyclonedx, CycloneDxVersion::V1_7);
        let cyclonedx: CycloneDxVersion = serde_json::from_str("1.4").unwrap();
        assert_eq!(cyclonedx, CycloneDxVersion::V1_4);

        assert_eq!(serde_json::to_string(&cyclonedx).unwrap(), "\"1.4\"");
    }

    #[test]
    fn test_unsupported_versions_name_the_supported_ones() {
        let error = serde_json::from_str::<CycloneDxVersion>("\"1.3\"")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(
                "unsupported CycloneDX version '1.3', expected one of 1.4, 1.5, 1.6, 1.7"
            ),
            "{error}"
        );
    }

    #[test]
    fn test_defaults_follow_the_tools_feluda_sits_next_to() {
        let versions = SpecVersions::default();
        assert_eq!(versions.spdx, SpdxVersion::V2_3);
        assert_eq!(versions.cyclonedx, CycloneDxVersion::V1_6);
    }
}
