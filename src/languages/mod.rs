//! Language-specific parsing and license analysis modules

pub mod c;
pub mod cpp;
pub mod go;
pub mod node;
pub mod python;
pub mod r;
pub mod rust;

use crate::licenses::LicenseInfo;
use std::path::Path;

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
    Rust(&'static str),
    Node(&'static str),
    Go(&'static str),
    Python(&'static [&'static str]),
    R(&'static [&'static str]),
}

impl Language {
    pub fn from_file_name(file_name: &str) -> Option<Self> {
        match file_name {
            "Cargo.toml" => Some(Language::Rust("Cargo.toml")),
            "package.json" => Some(Language::Node("package.json")),
            "go.mod" => Some(Language::Go("go.mod")),
            "vcpkg.json" => Some(Language::Cpp(&CPP_PATHS[..])),
            "conanfile.txt" | "conanfile.py" => Some(Language::Cpp(&CPP_PATHS[..])),
            "MODULE.bazel" => Some(Language::Cpp(&CPP_PATHS[..])),
            "configure.ac" | "configure.in" | "Makefile" => Some(Language::C(&C_PATHS[..])),
            "CMakeLists.txt" => {
                // CMake can be used for both C and C++, default to C++
                Some(Language::Cpp(&CPP_PATHS[..]))
            }
            _ => {
                if PYTHON_PATHS.contains(&file_name) {
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
