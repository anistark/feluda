//! Language-specific parsing and license analysis modules

pub mod c;
pub mod cpp;
pub mod go;
pub mod node;
pub mod python;
pub mod rust;

// Re-export commonly used types and functions for backward compatibility
// TODO: Remove when 1.8.5 is no longer supported
pub use c::analyze_c_licenses;
pub use cpp::analyze_cpp_licenses;
pub use go::{
    analyze_go_licenses, fetch_license_for_go_dependency, get_go_dependencies, GoPackages,
};
pub use node::{analyze_js_licenses, PackageJson};
pub use python::{analyze_python_licenses, fetch_license_for_python_dependency};
pub use rust::analyze_rust_licenses;

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
