//! Package ecosystem identity and PURL derivation.
//!
//! A package name and version identify a dependency only as long as every finding comes from one
//! project's manifests. The moment findings can arrive from more than one ecosystem at a time, a
//! Debian `libssl3` and an npm package of the same name are indistinguishable — to a reader of the
//! report and, worse, to a consumer of the SBOM. [`Ecosystem`] records where a package came from,
//! and the PURL built from it gives every package a coordinate that is unique across ecosystems.
//!
//! PURLs follow the [package-url spec](https://github.com/package-url/purl-spec):
//! `pkg:<type>/<namespace>/<name>@<version>`. The namespace and the per-type name normalization
//! are derived from the display name each analyzer already produces, so a Maven `group:artifact`,
//! an npm `@scope/name`, and a Go module path all land in the right components.

use serde::Serialize;

/// The packaging ecosystem a package was resolved from.
///
/// The variant determines the PURL type and, with it, how the package's name is normalized and
/// split into namespace and name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    /// Rust crates (`Cargo.toml`).
    Cargo,
    /// Node.js packages (`package.json`).
    Npm,
    /// Go modules (`go.mod`).
    Golang,
    /// Python distributions (`requirements.txt`, `pyproject.toml`, ...).
    Pypi,
    /// Java artifacts (`pom.xml`, `build.gradle`).
    Maven,
    /// Ruby gems (`Gemfile`).
    Gem,
    /// .NET packages (`.csproj`, ...).
    Nuget,
    /// R packages (`DESCRIPTION`, `renv.lock`).
    Cran,
    /// C/C++ packages declared through Conan.
    Conan,
    // TODO: The three OS-package ecosystems below are reserved for the filesystem and container
    // scanning phase of https://github.com/anistark/feluda/issues/247. They are part of the
    // identity model now so the PURL type mapping does not have to change when that lands.
    /// Debian and derivatives (`dpkg`).
    #[allow(dead_code)]
    Deb,
    /// RPM-based distributions.
    #[allow(dead_code)]
    Rpm,
    /// Alpine packages (`apk`).
    #[allow(dead_code)]
    Apk,
    /// Anything without a package registry of its own: C/C++ deps from Makefiles, CMake, Bazel or
    /// vcpkg, and the path-named findings the source and vendor scans produce.
    Generic,
}

impl Ecosystem {
    /// The PURL type string for this ecosystem, as registered in the purl spec.
    pub fn purl_type(self) -> &'static str {
        match self {
            Ecosystem::Cargo => "cargo",
            Ecosystem::Npm => "npm",
            Ecosystem::Golang => "golang",
            Ecosystem::Pypi => "pypi",
            Ecosystem::Maven => "maven",
            Ecosystem::Gem => "gem",
            Ecosystem::Nuget => "nuget",
            Ecosystem::Cran => "cran",
            Ecosystem::Conan => "conan",
            Ecosystem::Deb => "deb",
            Ecosystem::Rpm => "rpm",
            Ecosystem::Apk => "apk",
            Ecosystem::Generic => "generic",
        }
    }

    /// The package's PURL without a version: `pkg:<type>/<namespace>/<name>`.
    ///
    /// This is the package's identity independent of which version is installed, which is what
    /// duplicate suppression compares. Returns `None` when the name carries nothing usable.
    pub fn coordinates(self, name: &str) -> Option<String> {
        let (namespace, name) = self.split_name(name)?;
        let mut purl = format!("pkg:{}/", self.purl_type());
        if let Some(namespace) = namespace {
            purl.push_str(&namespace);
            purl.push('/');
        }
        purl.push_str(&name);
        Some(purl)
    }

    /// The package's full PURL: `pkg:<type>/<namespace>/<name>@<version>`.
    ///
    /// The version is dropped when it is empty, leaving a valid version-less PURL rather than a
    /// trailing `@`.
    pub fn purl(self, name: &str, version: &str) -> Option<String> {
        let mut purl = self.coordinates(name)?;
        let version = version.trim();
        if !version.is_empty() {
            purl.push('@');
            purl.push_str(&encode_component(version));
        }
        Some(purl)
    }

    /// Split an analyzer's display name into an encoded `(namespace, name)` pair, applying the
    /// name normalization the purl spec defines for this type.
    fn split_name(self, name: &str) -> Option<(Option<String>, String)> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }

        match self {
            // Maven names are reported as `groupId:artifactId`; the group is the namespace.
            Ecosystem::Maven => match name.split_once(':') {
                Some((group, artifact)) if !group.is_empty() && !artifact.is_empty() => {
                    Some((Some(encode_component(group)), encode_component(artifact)))
                }
                _ => Some((None, encode_component(name))),
            },
            // An npm scope is the namespace, and keeps its `@` (percent-encoded in canonical
            // form): `@babel/core` becomes `pkg:npm/%40babel/core`.
            Ecosystem::Npm => {
                let lowered = name.to_lowercase();
                match lowered.split_once('/') {
                    Some((scope, package)) if !scope.is_empty() && !package.is_empty() => {
                        Some((Some(encode_component(scope)), encode_component(package)))
                    }
                    _ => Some((None, encode_component(&lowered))),
                }
            }
            // A Go module path is a namespace of path segments plus a final name, all lowercase.
            Ecosystem::Golang => {
                let lowered = name.to_lowercase();
                let lowered = lowered.trim_matches('/');
                match lowered.rsplit_once('/') {
                    Some((namespace, package)) if !namespace.is_empty() && !package.is_empty() => {
                        Some((Some(encode_path(namespace)), encode_component(package)))
                    }
                    _ => Some((None, encode_component(lowered))),
                }
            }
            // PEP 503: lowercase, and every run of `-`, `_` or `.` collapses to a single `-`.
            Ecosystem::Pypi => Some((None, encode_component(&normalize_pypi_name(name)))),
            // Everything else is a flat, case-preserving name. Generic names are often paths, and
            // encoding the whole string keeps a path from being mistaken for a namespace.
            _ => Some((None, encode_component(name))),
        }
    }
}

impl std::fmt::Display for Ecosystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.purl_type())
    }
}

/// Whether a byte may appear literally in a PURL component (RFC 3986 unreserved characters).
fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// Percent-encode a single PURL component. Operating on bytes keeps multi-byte UTF-8 correct.
fn encode_component(component: &str) -> String {
    let mut encoded = String::with_capacity(component.len());
    for byte in component.bytes() {
        if is_unreserved(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Percent-encode a multi-segment namespace, leaving the `/` separators intact.
fn encode_path(path: &str) -> String {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(encode_component)
        .collect::<Vec<_>>()
        .join("/")
}

/// Normalize a Python distribution name per PEP 503.
fn normalize_pypi_name(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    let mut last_was_separator = false;
    for ch in name.to_lowercase().chars() {
        if matches!(ch, '-' | '_' | '.') {
            if !last_was_separator {
                normalized.push('-');
            }
            last_was_separator = true;
        } else {
            normalized.push(ch);
            last_was_separator = false;
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_purl_type_strings() {
        assert_eq!(Ecosystem::Cargo.purl_type(), "cargo");
        assert_eq!(Ecosystem::Golang.purl_type(), "golang");
        assert_eq!(Ecosystem::Deb.purl_type(), "deb");
        assert_eq!(Ecosystem::Generic.to_string(), "generic");
    }

    #[test]
    fn test_simple_purls() {
        assert_eq!(
            Ecosystem::Cargo.purl("serde", "1.0.219").unwrap(),
            "pkg:cargo/serde@1.0.219"
        );
        assert_eq!(
            Ecosystem::Gem.purl("rails", "7.1.3").unwrap(),
            "pkg:gem/rails@7.1.3"
        );
        assert_eq!(
            Ecosystem::Nuget.purl("Newtonsoft.Json", "13.0.3").unwrap(),
            "pkg:nuget/Newtonsoft.Json@13.0.3"
        );
        assert_eq!(
            Ecosystem::Cran.purl("ggplot2", "3.5.1").unwrap(),
            "pkg:cran/ggplot2@3.5.1"
        );
    }

    #[test]
    fn test_npm_scope_becomes_namespace() {
        assert_eq!(
            Ecosystem::Npm.purl("@babel/core", "7.24.0").unwrap(),
            "pkg:npm/%40babel/core@7.24.0"
        );
        assert_eq!(
            Ecosystem::Npm.purl("LeftPad", "1.0.0").unwrap(),
            "pkg:npm/leftpad@1.0.0"
        );
    }

    #[test]
    fn test_golang_module_path_splits() {
        assert_eq!(
            Ecosystem::Golang
                .purl("github.com/pkg/errors", "v0.9.1")
                .unwrap(),
            "pkg:golang/github.com/pkg/errors@v0.9.1"
        );
        assert_eq!(
            Ecosystem::Golang
                .purl("Gopkg.in/Yaml.v2", "v2.4.0")
                .unwrap(),
            "pkg:golang/gopkg.in/yaml.v2@v2.4.0"
        );
    }

    #[test]
    fn test_maven_coordinates_split_on_colon() {
        assert_eq!(
            Ecosystem::Maven
                .purl("com.fasterxml.jackson.core:jackson-databind", "2.17.0")
                .unwrap(),
            "pkg:maven/com.fasterxml.jackson.core/jackson-databind@2.17.0"
        );
        // A name without a group still produces a usable PURL.
        assert_eq!(
            Ecosystem::Maven.purl("junit", "4.13.2").unwrap(),
            "pkg:maven/junit@4.13.2"
        );
    }

    #[test]
    fn test_pypi_name_normalization() {
        assert_eq!(
            Ecosystem::Pypi.purl("Flask_SQLAlchemy", "3.1.1").unwrap(),
            "pkg:pypi/flask-sqlalchemy@3.1.1"
        );
        assert_eq!(
            Ecosystem::Pypi.purl("zope.interface", "6.2").unwrap(),
            "pkg:pypi/zope-interface@6.2"
        );
    }

    #[test]
    fn test_percent_encoding() {
        // Path-shaped generic names encode their separators, so a path is never read as a
        // namespace.
        assert_eq!(
            Ecosystem::Generic
                .purl("vendor/leftpad", "vendored")
                .unwrap(),
            "pkg:generic/vendor%2Fleftpad@vendored"
        );
        // Version constraints survive verbatim once encoded.
        assert_eq!(
            Ecosystem::Pypi.purl("requests", ">=2.0").unwrap(),
            "pkg:pypi/requests@%3E%3D2.0"
        );
    }

    #[test]
    fn test_version_is_optional() {
        assert_eq!(
            Ecosystem::Cargo.purl("serde", "  ").unwrap(),
            "pkg:cargo/serde"
        );
        assert_eq!(
            Ecosystem::Cargo.coordinates("serde").unwrap(),
            "pkg:cargo/serde"
        );
    }

    #[test]
    fn test_empty_name_has_no_purl() {
        assert!(Ecosystem::Cargo.purl("", "1.0.0").is_none());
        assert!(Ecosystem::Cargo.purl("   ", "1.0.0").is_none());
    }

    #[test]
    fn test_same_name_across_ecosystems_is_distinct() {
        let npm = Ecosystem::Npm.purl("libssl3", "3.0.0").unwrap();
        let deb = Ecosystem::Deb.purl("libssl3", "3.0.0").unwrap();
        assert_ne!(npm, deb);
    }
}
