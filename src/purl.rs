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
//!
//! Qualifiers (`?arch=amd64&distro=debian-12`) are written out but are not identity. They say
//! which build of a package this is, and a license belongs to the package, so duplicate
//! suppression, caches and SPDX identifiers all work from the PURL without them.

use std::collections::BTreeMap;

use serde::Serialize;

/// PURL qualifiers, keyed by their lowercase name. A `BTreeMap` because the spec's canonical form
/// sorts them by key.
pub type Qualifiers = BTreeMap<String, String>;

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
    /// RPM-based distributions, cataloged by [`crate::filesystem`].
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
    /// trailing `@`. Findings build theirs through [`Self::purl_with`], since they may carry
    /// qualifiers.
    #[cfg(test)]
    pub fn purl(self, name: &str, version: &str) -> Option<String> {
        self.purl_with(name, version, &Qualifiers::new())
    }

    /// The package's full PURL with its qualifiers: `pkg:<type>/<namespace>/<name>@<version>?<k>=<v>`.
    ///
    /// Qualifiers with an empty value are left out, as the spec requires. An rpm version written
    /// the way rpm prints it, `1:3.0.7-27.el9`, has its epoch moved into an `epoch` qualifier,
    /// which is where the purl spec puts it and where every other rpm PURL producer does.
    pub fn purl_with(self, name: &str, version: &str, qualifiers: &Qualifiers) -> Option<String> {
        let mut purl = self.coordinates(name)?;
        let mut version = version.trim();
        let mut qualifiers = std::borrow::Cow::Borrowed(qualifiers);

        if self == Ecosystem::Rpm {
            if let Some((epoch, rest)) = split_rpm_epoch(version) {
                version = rest;
                qualifiers
                    .to_mut()
                    .entry("epoch".to_string())
                    .or_insert_with(|| epoch.to_string());
            }
        }

        if !version.is_empty() {
            purl.push('@');
            purl.push_str(&encode_component(version));
        }

        let mut separator = '?';
        for (key, value) in qualifiers.iter() {
            let (key, value) = (key.trim(), value.trim());
            if key.is_empty() || value.is_empty() {
                continue;
            }
            purl.push(separator);
            purl.push_str(&key.to_ascii_lowercase());
            purl.push('=');
            purl.push_str(&encode_component(value));
            separator = '&';
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
            // cataloger puts it in front of the name, so the split mirrors npm's. The namespace is
            // always lowercased; the name is too for deb and apk, but an rpm name is case
            // sensitive (`openSUSE-build-key`) and the spec keeps it as written.
            Ecosystem::Deb | Ecosystem::Rpm | Ecosystem::Apk => {
                let name = if self == Ecosystem::Rpm {
                    name.to_string()
                } else {
                    name.to_lowercase()
                };
                match name.split_once('/') {
                    Some((distro, package)) if !distro.is_empty() && !package.is_empty() => Some((
                        Some(encode_component(&distro.to_lowercase())),
                        encode_component(package),
                    )),
                    _ => Some((None, encode_component(&name))),
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
    /// Carried through so a PURL read from someone else's SBOM is written back out whole.
    pub qualifiers: Qualifiers,
}

/// Parse a PURL string into an ecosystem, a display name, a version and its qualifiers.
///
/// The subpath (`#src/lib`) is dropped: it points inside a package, and feluda resolves licenses
/// per package. Returns `None` when the string is not a PURL or carries no name.
pub fn parse_purl(purl: &str) -> Option<ParsedPurl> {
    let purl = purl.trim();
    let rest = purl
        .strip_prefix("pkg:")
        .or_else(|| purl.strip_prefix("PKG:"))?;
    // Some producers write `pkg://type/name`, which the spec permits readers to accept.
    let rest = rest.trim_start_matches('/');
    let rest = rest.split('#').next()?;
    let (rest, qualifiers) = match rest.split_once('?') {
        Some((rest, qualifiers)) => (rest, parse_qualifiers(qualifiers)),
        None => (rest, Qualifiers::new()),
    };

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
        qualifiers,
    })
}

/// A PURL with its qualifiers and subpath removed: the package's identity.
///
/// What an SPDX identifier is derived from, so that describing a package more fully does not
/// change the identifier a document already gave it.
pub fn without_qualifiers(purl: &str) -> &str {
    purl.split(['?', '#']).next().unwrap_or(purl)
}

/// Read a qualifier string, `arch=amd64&distro=debian-12`. Keys are case insensitive and lowered;
/// a qualifier with no value says nothing and is dropped.
fn parse_qualifiers(qualifiers: &str) -> Qualifiers {
    qualifiers
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), decode_component(value)))
        .filter(|(key, value)| !key.is_empty() && !value.is_empty())
        .collect()
}

/// Split rpm's `epoch:version-release` into its epoch and the rest.
fn split_rpm_epoch(version: &str) -> Option<(&str, &str)> {
    let (epoch, rest) = version.split_once(':')?;
    (!epoch.is_empty() && epoch.bytes().all(|byte| byte.is_ascii_digit()) && !rest.is_empty())
        .then_some((epoch, rest))
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
    fn test_parse_purl_keeps_qualifiers_and_drops_subpath() {
        // syft tags OS packages with distro and architecture qualifiers. They are kept so they can
        // be written back out, but the name and version do not change because of them.
        let parsed = parse_purl("pkg:deb/debian/libssl3@3.0.15-1?distro=debian-12&ARCH=amd64")
            .expect("should parse");
        assert_eq!(parsed.ecosystem, Ecosystem::Deb);
        assert_eq!(parsed.name, "debian/libssl3");
        assert_eq!(parsed.version, "3.0.15-1");
        assert_eq!(
            parsed.qualifiers.get("arch").map(String::as_str),
            Some("amd64")
        );
        assert_eq!(
            parsed.qualifiers.get("distro").map(String::as_str),
            Some("debian-12")
        );

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
    fn test_rpm_names_keep_their_case() {
        // The purl spec lowercases deb and apk names, but an rpm name is case sensitive.
        assert_eq!(
            Ecosystem::Rpm
                .purl("OpenSUSE/openSUSE-build-key", "1.0-lp156.8.2")
                .unwrap(),
            "pkg:rpm/opensuse/openSUSE-build-key@1.0-lp156.8.2"
        );
        assert_eq!(
            Ecosystem::Deb.purl("debian/LibFoo", "1.0").unwrap(),
            "pkg:deb/debian/libfoo@1.0"
        );
        assert_eq!(
            Ecosystem::Apk.purl("alpine/LibFoo", "1.0").unwrap(),
            "pkg:apk/alpine/libfoo@1.0"
        );
        let parsed = parse_purl("pkg:rpm/opensuse/openSUSE-build-key@1.0").unwrap();
        assert_eq!(parsed.name, "opensuse/openSUSE-build-key");
    }

    #[test]
    fn test_os_packages_from_different_distros_are_distinct() {
        let debian = Ecosystem::Deb.coordinates("debian/libssl3").unwrap();
        let ubuntu = Ecosystem::Deb.coordinates("ubuntu/libssl3").unwrap();
        assert_ne!(debian, ubuntu);
    }

    #[test]
    fn test_qualifiers_are_sorted_encoded_and_skipped_when_empty() {
        let qualifiers: Qualifiers = [
            ("distro", "debian-12"),
            ("arch", "amd64"),
            ("upstream", "openssl 3"),
            ("empty", " "),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
        assert_eq!(
            Ecosystem::Deb
                .purl_with("debian/libssl3", "3.0.15-1", &qualifiers)
                .unwrap(),
            "pkg:deb/debian/libssl3@3.0.15-1?arch=amd64&distro=debian-12&upstream=openssl%203"
        );
    }

    #[test]
    fn test_qualified_purls_round_trip() {
        for purl in [
            "pkg:deb/debian/libssl3@3.0.15-1?arch=amd64&distro=debian-12&upstream=openssl",
            "pkg:apk/alpine/musl@1.2.5-r0?arch=x86_64&distro=alpine-3.20.3",
            "pkg:rpm/fedora/openssl-libs@3.2.2-9.fc41?arch=x86_64&epoch=1&upstream=openssl-3.2.2-9.fc41.src.rpm",
        ] {
            let parsed = parse_purl(purl).expect("should parse");
            assert_eq!(
                parsed
                    .ecosystem
                    .purl_with(&parsed.name, &parsed.version, &parsed.qualifiers)
                    .unwrap(),
                purl
            );
        }
    }

    #[test]
    fn test_rpm_epoch_moves_into_a_qualifier() {
        // rpm prints `1:3.2.2-9.fc41`; the purl spec keeps the epoch out of the version.
        assert_eq!(
            Ecosystem::Rpm
                .purl("fedora/openssl-libs", "1:3.2.2-9.fc41")
                .unwrap(),
            "pkg:rpm/fedora/openssl-libs@3.2.2-9.fc41?epoch=1"
        );
        // An epoch the qualifiers already name wins over one left in the version.
        let qualifiers: Qualifiers = [("epoch".to_string(), "2".to_string())].into();
        assert_eq!(
            Ecosystem::Rpm
                .purl_with("bash", "1:5.2-1", &qualifiers)
                .unwrap(),
            "pkg:rpm/bash@5.2-1?epoch=2"
        );
        // Debian keeps its epoch in the version, which is what its PURLs do.
        assert_eq!(
            Ecosystem::Deb.purl("debian/tar", "1:1.34-1").unwrap(),
            "pkg:deb/debian/tar@1%3A1.34-1"
        );
        // A colon that is not an epoch is left alone.
        assert_eq!(
            Ecosystem::Rpm.purl("odd", "v:1").unwrap(),
            "pkg:rpm/odd@v%3A1"
        );
    }

    #[test]
    fn test_without_qualifiers() {
        assert_eq!(
            without_qualifiers("pkg:deb/debian/libssl3@3.0.15-1?arch=amd64"),
            "pkg:deb/debian/libssl3@3.0.15-1"
        );
        assert_eq!(
            without_qualifiers("pkg:golang/x/y@v1#sub"),
            "pkg:golang/x/y@v1"
        );
        assert_eq!(without_qualifiers("pkg:cargo/serde"), "pkg:cargo/serde");
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
