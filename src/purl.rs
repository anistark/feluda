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
    /// Debian and derivatives (`dpkg`), cataloged by [`crate::filesystem`].
    Deb,
    // TODO: RPM is reserved for https://github.com/anistark/feluda/issues/253, which adds the
    // cataloger. It is part of the identity model now so reading an rpm package out of someone
    // else's SBOM already lands in the right ecosystem.
    /// RPM-based distributions.
    #[allow(dead_code)]
    Rpm,
    /// Alpine packages (`apk`), cataloged by [`crate::filesystem`].
    Apk,
    /// Anything without a package registry of its own: C/C++ deps from Makefiles, CMake, Bazel or
    /// vcpkg, and the path-named findings the source and vendor scans produce.
    Generic,
}

impl Ecosystem {
    /// The ecosystem a purl type string names, or `None` for a type feluda has no analyzer for.
    ///
    /// Callers reading a third-party SBOM should treat `None` as [`Ecosystem::Generic`]: an
    /// unrecognised type still identifies a package, it just identifies one feluda cannot resolve
    /// a license for.
    pub fn from_purl_type(purl_type: &str) -> Option<Self> {
        let lowered = purl_type.to_ascii_lowercase();
        let ecosystem = match lowered.as_str() {
            "cargo" | "crates" => Ecosystem::Cargo,
            "npm" => Ecosystem::Npm,
            "golang" | "go" => Ecosystem::Golang,
            "pypi" => Ecosystem::Pypi,
            "maven" => Ecosystem::Maven,
            "gem" => Ecosystem::Gem,
            "nuget" => Ecosystem::Nuget,
            "cran" => Ecosystem::Cran,
            "conan" => Ecosystem::Conan,
            "deb" => Ecosystem::Deb,
            "rpm" => Ecosystem::Rpm,
            "apk" => Ecosystem::Apk,
            "generic" => Ecosystem::Generic,
            _ => return None,
        };
        Some(ecosystem)
    }

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
            // An OS package's namespace is the distro that ships it, which is part of its identity:
            // `pkg:deb/debian/libssl3` and `pkg:deb/ubuntu/libssl3` are different packages. The
            // cataloger puts it in front of the name, so the split mirrors npm's.
            Ecosystem::Deb | Ecosystem::Rpm | Ecosystem::Apk => {
                let lowered = name.to_lowercase();
                match lowered.split_once('/') {
                    Some((distro, package)) if !distro.is_empty() && !package.is_empty() => {
                        Some((Some(encode_component(distro)), encode_component(package)))
                    }
                    _ => Some((None, encode_component(&lowered))),
                }
            }
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

/// A PURL read back into the identity feluda works with.
///
/// This is the inverse of [`Ecosystem::purl`], used when the packages come from someone else's
/// SBOM rather than from a manifest feluda parsed. `name` is the display name an analyzer would
/// have produced, so `LicenseInfo::purl()` regenerates the coordinate the document carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPurl {
    pub ecosystem: Ecosystem,
    pub name: String,
    pub version: String,
}

/// Parse a PURL string into an ecosystem, a display name and a version.
///
/// Qualifiers (`?arch=amd64`) and the subpath (`#src/lib`) are dropped: they qualify which build
/// of a package this is, and feluda resolves licenses per package. Returns `None` when the string
/// is not a PURL or carries no name.
pub fn parse_purl(purl: &str) -> Option<ParsedPurl> {
    let purl = purl.trim();
    let rest = purl
        .strip_prefix("pkg:")
        .or_else(|| purl.strip_prefix("PKG:"))?;
    // Some producers write `pkg://type/name`, which the spec permits readers to accept.
    let rest = rest.trim_start_matches('/');
    let rest = rest.split('#').next()?;
    let rest = rest.split('?').next()?;

    let (purl_type, remainder) = rest.split_once('/')?;
    if purl_type.is_empty() {
        return None;
    }
    let ecosystem = Ecosystem::from_purl_type(purl_type).unwrap_or(Ecosystem::Generic);

    let mut segments: Vec<&str> = remainder.split('/').filter(|s| !s.is_empty()).collect();
    let last = segments.pop()?;
    let (name, version) = match last.rsplit_once('@') {
        Some((name, version)) => (name, decode_component(version)),
        None => (last, String::new()),
    };
    let name = decode_component(name);
    if name.is_empty() {
        return None;
    }

    let namespace: Vec<String> = segments.iter().map(|s| decode_component(s)).collect();
    let name = join_name(ecosystem, &namespace, &name);

    Some(ParsedPurl {
        ecosystem,
        name,
        version,
    })
}

/// Rejoin a PURL namespace and name into the display name the matching analyzer emits, so the
/// name round-trips through [`Ecosystem::split_name`].
///
/// Ecosystems whose namespace is not part of the package name — the channel in a Conan reference —
/// keep the bare name.
fn join_name(ecosystem: Ecosystem, namespace: &[String], name: &str) -> String {
    if namespace.is_empty() {
        return name.to_string();
    }
    match ecosystem {
        Ecosystem::Maven => format!("{}:{}", namespace.join("."), name),
        Ecosystem::Npm | Ecosystem::Golang | Ecosystem::Deb | Ecosystem::Rpm | Ecosystem::Apk => {
            format!("{}/{}", namespace.join("/"), name)
        }
        _ => name.to_string(),
    }
}

/// Percent-decode a single PURL component, leaving invalid escapes as literal text rather than
/// discarding them.
fn decode_component(component: &str) -> String {
    let bytes = component.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &component[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
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
    fn test_parse_purl_round_trips_display_names() {
        // Every case here is a PURL one of the analyzers emits, so parsing and re-emitting has to
        // land back on the same string.
        for purl in [
            "pkg:cargo/serde@1.0.219",
            "pkg:npm/%40babel/core@7.24.0",
            "pkg:golang/github.com/pkg/errors@v0.9.1",
            "pkg:maven/com.fasterxml.jackson.core/jackson-databind@2.17.0",
            "pkg:pypi/flask-sqlalchemy@3.1.1",
            "pkg:gem/rails@7.1.3",
        ] {
            let parsed = parse_purl(purl).expect("should parse");
            assert_eq!(
                parsed
                    .ecosystem
                    .purl(&parsed.name, &parsed.version)
                    .unwrap(),
                purl
            );
        }
    }

    #[test]
    fn test_parse_purl_components() {
        let parsed = parse_purl("pkg:npm/%40babel/core@7.24.0").unwrap();
        assert_eq!(parsed.ecosystem, Ecosystem::Npm);
        assert_eq!(parsed.name, "@babel/core");
        assert_eq!(parsed.version, "7.24.0");

        let parsed = parse_purl("pkg:maven/org.slf4j/slf4j-api@2.0.13").unwrap();
        assert_eq!(parsed.name, "org.slf4j:slf4j-api");
    }

    #[test]
    fn test_parse_purl_drops_qualifiers_and_subpath() {
        // syft tags OS packages with distro and architecture qualifiers. The distro namespace is
        // identity and is kept; the qualifiers say which build this is, and are not.
        let parsed = parse_purl("pkg:deb/debian/libssl3@3.0.15-1?arch=amd64&distro=debian-12")
            .expect("should parse");
        assert_eq!(parsed.ecosystem, Ecosystem::Deb);
        assert_eq!(parsed.name, "debian/libssl3");
        assert_eq!(parsed.version, "3.0.15-1");

        let parsed = parse_purl("pkg:golang/github.com/pkg/errors@v0.9.1#src/lib").unwrap();
        assert_eq!(parsed.name, "github.com/pkg/errors");
        assert_eq!(parsed.version, "v0.9.1");
    }

    #[test]
    fn test_os_package_namespace_round_trips() {
        // What the cataloger emits has to come back out of its own PURL unchanged, or a feluda
        // SBOM read back through `--sbom-input` would rename every OS package.
        for (ecosystem, name) in [
            (Ecosystem::Deb, "debian/libssl3"),
            (Ecosystem::Apk, "alpine/musl"),
            (Ecosystem::Rpm, "fedora/glibc"),
        ] {
            let purl = ecosystem.purl(name, "1.0").expect("should build");
            let parsed = parse_purl(&purl).expect("should parse");
            assert_eq!(parsed.ecosystem, ecosystem);
            assert_eq!(parsed.name, name);
        }

        // A rootfs with no /etc/os-release has no namespace to give, which is still a valid PURL.
        assert_eq!(
            Ecosystem::Apk.purl("musl", "1.2.5-r0").unwrap(),
            "pkg:apk/musl@1.2.5-r0"
        );
    }

    #[test]
    fn test_os_packages_from_different_distros_are_distinct() {
        let debian = Ecosystem::Deb.coordinates("debian/libssl3").unwrap();
        let ubuntu = Ecosystem::Deb.coordinates("ubuntu/libssl3").unwrap();
        assert_ne!(debian, ubuntu);
    }

    #[test]
    fn test_parse_purl_without_version() {
        let parsed = parse_purl("pkg:cargo/serde").unwrap();
        assert_eq!(parsed.name, "serde");
        assert_eq!(parsed.version, "");
    }

    #[test]
    fn test_parse_purl_unknown_type_is_generic() {
        let parsed = parse_purl("pkg:swift/github.com/apple/swift-log@1.5.3").unwrap();
        assert_eq!(parsed.ecosystem, Ecosystem::Generic);
        // A generic name keeps only the last segment, since the namespace is not part of it.
        assert_eq!(parsed.name, "swift-log");
    }

    #[test]
    fn test_parse_purl_rejects_non_purls() {
        assert!(parse_purl("").is_none());
        assert!(parse_purl("serde@1.0.0").is_none());
        assert!(parse_purl("pkg:cargo").is_none());
        assert!(parse_purl("pkg:/serde@1.0.0").is_none());
        assert!(parse_purl("pkg:cargo/@1.0.0").is_none());
    }

    #[test]
    fn test_decode_component_leaves_invalid_escapes() {
        assert_eq!(decode_component("%40babel"), "@babel");
        assert_eq!(decode_component("100%"), "100%");
        assert_eq!(decode_component("%zz"), "%zz");
        assert_eq!(decode_component("caf%C3%A9"), "café");
    }

    #[test]
    fn test_same_name_across_ecosystems_is_distinct() {
        let npm = Ecosystem::Npm.purl("libssl3", "3.0.0").unwrap();
        let deb = Ecosystem::Deb.purl("libssl3", "3.0.0").unwrap();
        assert_ne!(npm, deb);
    }
}
