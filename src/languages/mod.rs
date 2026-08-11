//! Language-specific parsing and license analysis modules

pub mod c;
pub mod cpp;
pub mod dotnet;
pub mod go;
pub mod java;
pub mod node;
pub mod python;
pub mod r;
pub mod ruby;
pub mod rust;

use crate::debug::{log, LogLevel};
use crate::licenses::{is_unresolved_license, LicenseInfo};
use crate::purl::Ecosystem;
use std::path::Path;

/// Resolve a license from a package's coordinates alone, with no manifest or project tree behind
/// it.
///
/// The manifest path never needs this: an analyzer reads the license out of the file it is already
/// parsing. Packages that arrive from someone else's SBOM have no such file, so a component the
/// document left as NOASSERTION can only be resolved by asking that ecosystem's registry. Each
/// arm delegates to the lookup the matching analyzer already uses, so there is one implementation
/// of "ask npm" and one of "ask Maven Central".
///
/// Returns `None` for ecosystems with no registry to ask — OS packages, Conan, and the generic
/// bucket — and for any lookup that came back without a real license.
pub fn resolve_license_for(ecosystem: Ecosystem, name: &str, version: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Resolving license for {ecosystem} package {name} ({version})"),
    );

    let license = match ecosystem {
        Ecosystem::Cargo => rust::fetch_license_from_crates_io(name, version),
        Ecosystem::Npm => node::get_license_from_npm_registry_api(name, version),
        Ecosystem::Golang => Some(go::fetch_license_for_go_dependency(name, version)),
        Ecosystem::Pypi => Some(python::fetch_license_for_python_dependency(name, version)),
        Ecosystem::Maven => {
            // Maven display names are `groupId:artifactId`; without a group there is nothing to
            // build a repository path from.
            let (group_id, artifact_id) = name.split_once(':')?;
            Some(java::fetch_maven_license(group_id, artifact_id, version))
        }
        Ecosystem::Gem => Some(ruby::fetch_ruby_license(name, version)),
        Ecosystem::Nuget => Some(dotnet::fetch_license_for_nuget_package(name, version)),
        Ecosystem::Cran => Some(r::fetch_license_for_r_dependency(name, version)),
        // Conan Center answers by name and version, but only for packages it hosts; every other
        // ecosystem here has no registry at all.
        Ecosystem::Conan => Some(cpp::fetch_license_from_conan_center(name, version)),
        Ecosystem::Deb | Ecosystem::Rpm | Ecosystem::Apk | Ecosystem::Generic => None,
    };

    // Analyzers spell a failed lookup several ways ("Unknown", "Unknown license for x: 1.0").
    // Anything that does not name a license is no better than what the document already said.
    license
        .map(|license| license.trim().to_string())
        .filter(|license| !is_unresolved_license(Some(license)))
        .filter(|license| !license.to_ascii_lowercase().starts_with("unknown license"))
}

/// Common trait for language-specific dependency parsers
#[allow(dead_code)]
pub trait LanguageParser {
    /// Parse dependencies from a project file and return license information
    fn parse_dependencies(
        &self,
        project_path: &Path,
    ) -> crate::debug::FeludaResult<Vec<LicenseInfo>>;

    /// Get the name of the language
    fn language_name(&self) -> &'static str;

    /// Get the typical project files
    fn supported_files(&self) -> &'static [&'static str];
}

/// Language identification
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Language {
    C(&'static [&'static str]),
    Cpp(&'static [&'static str]),
    DotNet(&'static [&'static str]),
    Java(&'static [&'static str]),
    Rust(&'static str),
    Node(&'static str),
    Go(&'static str),
    Python(&'static [&'static str]),
    R(&'static [&'static str]),
    Ruby(&'static [&'static str]),
}

impl Language {
    pub fn from_file_name(file_name: &str) -> Option<Self> {
        match file_name {
            "Cargo.toml" => Some(Language::Rust("Cargo.toml")),
            "package.json" => Some(Language::Node("package.json")),
            "go.mod" => Some(Language::Go("go.mod")),
            "go.work" => Some(Language::Go("go.work")),
            "pom.xml" => Some(Language::Java(&JAVA_PATHS[..])),
            "build.gradle" | "build.gradle.kts" => Some(Language::Java(&JAVA_PATHS[..])),
            "vcpkg.json" => Some(Language::Cpp(&CPP_PATHS[..])),
            "conanfile.txt" | "conanfile.py" => Some(Language::Cpp(&CPP_PATHS[..])),
            "MODULE.bazel" => Some(Language::Cpp(&CPP_PATHS[..])),
            "configure.ac" | "configure.in" | "Makefile" => Some(Language::C(&C_PATHS[..])),
            "CMakeLists.txt" => Some(Language::Cpp(&CPP_PATHS[..])),
            "Gemfile" | "Gemfile.lock" => Some(Language::Ruby(&RUBY_PATHS[..])),
            _ => {
                if file_name.ends_with(".csproj")
                    || file_name.ends_with(".fsproj")
                    || file_name.ends_with(".vbproj")
                    || file_name.ends_with(".slnx")
                {
                    Some(Language::DotNet(&DOTNET_PATHS[..]))
                } else if PYTHON_PATHS.contains(&file_name) {
                    Some(Language::Python(&PYTHON_PATHS[..]))
                } else if R_PATHS.contains(&file_name) {
                    Some(Language::R(&R_PATHS[..]))
                } else {
                    None
                }
            }
        }
    }
}

/// Java project file patterns (Maven and Gradle)
pub const JAVA_PATHS: [&str; 3] = ["pom.xml", "build.gradle", "build.gradle.kts"];

/// C project file patterns
pub const C_PATHS: [&str; 3] = ["configure.ac", "configure.in", "Makefile"];

/// C++ project file patterns
pub const CPP_PATHS: [&str; 5] = [
    "vcpkg.json",
    "conanfile.txt",
    "conanfile.py",
    "CMakeLists.txt",
    "MODULE.bazel",
];

/// Python project file patterns
pub const PYTHON_PATHS: [&str; 4] = [
    "requirements.txt",
    "Pipfile.lock",
    "pip_freeze.txt",
    "pyproject.toml",
];

/// R project file patterns
pub const R_PATHS: [&str; 2] = ["DESCRIPTION", "renv.lock"];

/// Ruby project file patterns
pub const RUBY_PATHS: [&str; 2] = ["Gemfile.lock", "Gemfile"];

/// .NET project file patterns
pub const DOTNET_PATHS: [&str; 4] = [".csproj", ".fsproj", ".vbproj", ".slnx"];
