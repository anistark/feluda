//! Which image, and which of its layers: reading the OCI index and the docker save manifest.
//!
//! Two formats describe the same thing. An OCI layout has `index.json`, whose descriptors point at
//! image manifests (or at further indexes, one per platform), and each manifest lists its config
//! and layers by digest under `blobs/`. Legacy `docker save` has `manifest.json`, an array with one
//! entry per saved image naming its config file and its layer tars by path. Docker 25 and later
//! write both, and the OCI one is the one read when it is there.
//!
//! Selection is the one place the user has a say. An archive can hold several images (a multi
//! platform index, or several images saved together), and guessing which one to scan would report
//! on something the user did not ask about. One image is taken as is; more than one needs
//! `--platform`, and the error lists what there is to choose from.

use std::collections::HashMap;
use std::fmt;

use serde::Deserialize;

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};

use super::store::Store;

/// The OCI layout index, and the same structure nested one level down for a per-platform index.
#[derive(Debug, Deserialize)]
struct Index {
    manifests: Vec<Descriptor>,
}

/// A reference to a blob: its digest, and for index entries, the platform it is for.
#[derive(Debug, Deserialize)]
struct Descriptor {
    digest: String,
    #[serde(default)]
    platform: Option<Platform>,
    #[serde(default)]
    annotations: HashMap<String, String>,
}

/// An image manifest: the config blob and the layers in application order.
#[derive(Debug, Deserialize)]
struct Manifest {
    config: Descriptor,
    layers: Vec<Descriptor>,
}

/// The parts of an image config that say what the image runs on.
#[derive(Debug, Deserialize)]
struct Config {
    os: Option<String>,
    architecture: Option<String>,
    variant: Option<String>,
}

/// One entry of legacy `manifest.json`: one saved image.
#[derive(Debug, Deserialize)]
struct DockerEntry {
    #[serde(rename = "Config")]
    config: String,
    #[serde(rename = "RepoTags", default)]
    repo_tags: Option<Vec<String>>,
    #[serde(rename = "Layers")]
    layers: Vec<String>,
}

/// What an image runs on, written the way `docker --platform` takes it: `linux/arm64/v8`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub architecture: String,
    #[serde(default)]
    pub variant: Option<String>,
}

impl Platform {
    /// Parse `os/arch` or `os/arch/variant`.
    pub fn parse(spec: &str) -> Option<Platform> {
        let mut parts = spec.trim().split('/');
        let os = parts.next()?.trim();
        let architecture = parts.next()?.trim();
        let variant = parts
            .next()
            .map(str::trim)
            .filter(|variant| !variant.is_empty());
        if os.is_empty() || architecture.is_empty() || parts.next().is_some() {
            return None;
        }
        Some(Platform {
            os: os.to_string(),
            architecture: architecture.to_string(),
            variant: variant.map(str::to_string),
        })
    }

    /// Whether `self`, as the user asked for it, accepts an image's platform. A request without a
    /// variant matches any variant, so `linux/arm64` finds a `linux/arm64/v8` image.
    fn accepts(&self, actual: &Platform) -> bool {
        self.os == actual.os
            && self.architecture == actual.architecture
            && match &self.variant {
                Some(variant) => actual.variant.as_deref() == Some(variant),
                None => true,
            }
    }

    /// buildx writes provenance and SBOM attestations into the index as manifests for a platform
    /// that does not exist. They hold no layers worth scanning and are never what the user meant.
    fn is_attestation(&self) -> bool {
        self.os == "unknown" && self.architecture == "unknown"
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.os, self.architecture)?;
        if let Some(variant) = &self.variant {
            write!(f, "/{variant}")?;
        }
        Ok(())
    }
}

/// The image to scan: its layers as store names, in the order they apply.
#[derive(Debug)]
pub struct Image {
    /// The tag or reference the archive recorded for it, if any.
    pub name: Option<String>,
    pub platform: Option<Platform>,
    pub layers: Vec<String>,
}

impl Image {
    /// How the image is named in messages: `app:latest (linux/amd64)`, or whatever half is known.
    pub fn describe(&self) -> String {
        match (&self.name, &self.platform) {
            (Some(name), Some(platform)) => format!("{name} ({platform})"),
            (Some(name), None) => name.clone(),
            (None, Some(platform)) => platform.to_string(),
            (None, None) => "image".to_string(),
        }
    }
}

/// Read the archive's index and pick the image `platform` asks for.
pub fn select(store: &Store, platform: Option<&str>) -> FeludaResult<Image> {
    let wanted = match platform {
        Some(spec) => Some(Platform::parse(spec).ok_or_else(|| {
            FeludaError::Image(format!(
                "Invalid --platform '{spec}': expected os/arch or os/arch/variant, like linux/amd64 or linux/arm/v7"
            ))
        })?),
        None => None,
    };

    let candidates = if store.has("index.json") {
        log(LogLevel::Info, "Reading the archive as an OCI image layout");
        oci_candidates(store)?
    } else if store.has("manifest.json") {
        log(
            LogLevel::Info,
            "Reading the archive as a docker save tarball",
        );
        docker_candidates(store)?
    } else {
        return Err(FeludaError::Image(
            "Not an image archive: found neither index.json (OCI image layout) nor manifest.json \
             (docker save). Point --image-archive at `docker save` output or an OCI layout directory."
                .to_string(),
        ));
    };

    choose(candidates, wanted)
}

/// Pick one image out of what the archive holds.
fn choose(candidates: Vec<Image>, wanted: Option<Platform>) -> FeludaResult<Image> {
    let available = || {
        candidates
            .iter()
            .map(Image::describe)
            .collect::<Vec<_>>()
            .join(", ")
    };

    if candidates.is_empty() {
        return Err(FeludaError::Image(
            "The archive lists no image manifests".to_string(),
        ));
    }

    let Some(wanted) = wanted else {
        if candidates.len() == 1 {
            return Ok(candidates.into_iter().next().expect("one candidate"));
        }
        return Err(FeludaError::Image(format!(
            "The archive holds {} images; choose one with --platform. Available: {}",
            candidates.len(),
            available()
        )));
    };

    let matching: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, image)| {
            image
                .platform
                .as_ref()
                .is_some_and(|actual| wanted.accepts(actual))
        })
        .map(|(index, _)| index)
        .collect();
    match matching.as_slice() {
        [index] => {
            let mut candidates = candidates;
            Ok(candidates.swap_remove(*index))
        }
        [] => Err(FeludaError::Image(format!(
            "No {wanted} image in the archive. Available: {}",
            available()
        ))),
        _ => Err(FeludaError::Image(format!(
            "--platform {wanted} matches more than one image: {}. Give the variant too, or save one image at a time.",
            matching
                .iter()
                .map(|index| candidates[*index].describe())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Every image an OCI index reaches, attestations dropped.
fn oci_candidates(store: &Store) -> FeludaResult<Vec<Image>> {
    let mut candidates = Vec::new();
    let index: Index = parse_json(&store.read("index.json")?, "index.json")?;
    collect_oci(store, index, None, 0, &mut candidates)?;
    Ok(candidates)
}

/// Nesting deeper than an index of indexes is nothing any tool writes; stop before a cycle can.
const MAX_INDEX_DEPTH: usize = 3;

fn collect_oci(
    store: &Store,
    index: Index,
    inherited_name: Option<&str>,
    depth: usize,
    into: &mut Vec<Image>,
) -> FeludaResult<()> {
    for descriptor in index.manifests {
        if descriptor
            .platform
            .as_ref()
            .is_some_and(Platform::is_attestation)
            || descriptor
                .annotations
                .get("vnd.docker.reference.type")
                .is_some_and(|kind| kind == "attestation-manifest")
        {
            continue;
        }

        let name = descriptor
            .annotations
            .get("io.containerd.image.name")
            .or_else(|| {
                descriptor
                    .annotations
                    .get("org.opencontainers.image.ref.name")
            })
            .map(String::as_str)
            .or(inherited_name);

        let blob = blob_name(&descriptor.digest)?;
        let content = store.read(&blob)?;
        let json: serde_json::Value = parse_json(&content, &blob)?;

        if json.get("manifests").is_some() {
            if depth + 1 >= MAX_INDEX_DEPTH {
                return Err(FeludaError::Image(format!(
                    "Index {blob} is nested too deeply to be an image index"
                )));
            }
            let nested: Index = parse_json(&content, &blob)?;
            collect_oci(store, nested, name, depth + 1, into)?;
            continue;
        }

        let manifest: Manifest = parse_json(&content, &blob)?;
        let platform = match descriptor.platform {
            Some(platform) => Some(platform),
            None => config_platform(store, &blob_name(&manifest.config.digest)?),
        };
        let layers = manifest
            .layers
            .iter()
            .map(|layer| blob_name(&layer.digest))
            .collect::<FeludaResult<Vec<_>>>()?;
        into.push(Image {
            name: name.map(str::to_string),
            platform,
            layers,
        });
    }
    Ok(())
}

/// Every image a legacy `manifest.json` lists.
fn docker_candidates(store: &Store) -> FeludaResult<Vec<Image>> {
    let entries: Vec<DockerEntry> = parse_json(&store.read("manifest.json")?, "manifest.json")?;
    entries
        .into_iter()
        .map(|entry| {
            let layers = entry
                .layers
                .iter()
                .map(|layer| archive_path(layer))
                .collect::<FeludaResult<Vec<_>>>()?;
            Ok(Image {
                name: entry.repo_tags.and_then(|tags| tags.into_iter().next()),
                platform: config_platform(store, &archive_path(&entry.config)?),
                layers,
            })
        })
        .collect()
}

/// The platform an image config records, when the config can be read. A config that cannot be
/// read leaves the image without a platform rather than failing the scan: the layers are what
/// matter, and the platform is only needed to tell several images apart.
fn config_platform(store: &Store, config: &str) -> Option<Platform> {
    let content = store.read(config).ok()?;
    let config: Config = serde_json::from_slice(&content).ok()?;
    Some(Platform {
        os: config.os?,
        architecture: config.architecture?,
        variant: config.variant.filter(|variant| !variant.is_empty()),
    })
}

/// Where a digest's blob lives in an OCI layout: `blobs/<algorithm>/<encoded>`.
///
/// The digest comes out of a file in the archive, so it is checked before it becomes a path: an
/// algorithm and a hex string, nothing that could name a file outside `blobs/`.
fn blob_name(digest: &str) -> FeludaResult<String> {
    let invalid = || FeludaError::Image(format!("Invalid digest in the image index: {digest}"));
    let (algorithm, encoded) = digest.split_once(':').ok_or_else(invalid)?;
    let well_formed = !algorithm.is_empty()
        && algorithm
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
        && !encoded.is_empty()
        && encoded.chars().all(|c| c.is_ascii_hexdigit());
    if !well_formed {
        return Err(invalid());
    }
    Ok(format!(
        "blobs/{algorithm}/{}",
        encoded.to_ascii_lowercase()
    ))
}

/// A path out of `manifest.json` as a store name: relative, forward slashes, no climbing.
fn archive_path(path: &str) -> FeludaResult<String> {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if parts.is_empty() || parts.contains(&"..") || path.starts_with('/') {
        return Err(FeludaError::Image(format!(
            "Invalid path in manifest.json: {path}"
        )));
    }
    Ok(parts.join("/"))
}

fn parse_json<T: for<'de> Deserialize<'de>>(content: &[u8], name: &str) -> FeludaResult<T> {
    serde_json::from_slice(content)
        .map_err(|error| FeludaError::Image(format!("Failed to parse {name}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn image(name: Option<&str>, platform: &str) -> Image {
        Image {
            name: name.map(str::to_string),
            platform: Platform::parse(platform),
            layers: vec![],
        }
    }

    #[test]
    fn test_parses_platform_specs() {
        assert_eq!(
            Platform::parse("linux/amd64"),
            Some(Platform {
                os: "linux".into(),
                architecture: "amd64".into(),
                variant: None
            })
        );
        assert_eq!(
            Platform::parse("linux/arm/v7").unwrap().variant.as_deref(),
            Some("v7")
        );
        assert_eq!(Platform::parse("linux/arm64/").unwrap().variant, None);
        assert!(Platform::parse("linux").is_none());
        assert!(Platform::parse("/amd64").is_none());
        assert!(Platform::parse("linux/arm/v7/extra").is_none());
    }

    #[test]
    fn test_platform_displays_the_way_docker_writes_it() {
        assert_eq!(
            Platform::parse("linux/arm64/v8").unwrap().to_string(),
            "linux/arm64/v8"
        );
        assert_eq!(
            Platform::parse("linux/amd64").unwrap().to_string(),
            "linux/amd64"
        );
    }

    #[test]
    fn test_a_request_without_variant_accepts_any_variant() {
        let wanted = Platform::parse("linux/arm64").unwrap();
        assert!(wanted.accepts(&Platform::parse("linux/arm64/v8").unwrap()));
        assert!(wanted.accepts(&Platform::parse("linux/arm64").unwrap()));
        assert!(!wanted.accepts(&Platform::parse("linux/amd64").unwrap()));

        let exact = Platform::parse("linux/arm/v7").unwrap();
        assert!(exact.accepts(&Platform::parse("linux/arm/v7").unwrap()));
        assert!(!exact.accepts(&Platform::parse("linux/arm/v6").unwrap()));
        assert!(!exact.accepts(&Platform::parse("linux/arm").unwrap()));
    }

    #[test]
    fn test_a_single_image_is_chosen_without_a_platform() {
        let chosen = choose(vec![image(Some("app:latest"), "linux/amd64")], None).unwrap();
        assert_eq!(chosen.name.as_deref(), Some("app:latest"));
    }

    #[test]
    fn test_several_images_without_a_platform_fail_naming_them() {
        let error = choose(
            vec![
                image(Some("app:latest"), "linux/amd64"),
                image(Some("app:latest"), "linux/arm64/v8"),
            ],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("--platform"), "{error}");
        assert!(error.contains("app:latest (linux/amd64)"), "{error}");
        assert!(error.contains("app:latest (linux/arm64/v8)"), "{error}");
    }

    #[test]
    fn test_platform_picks_among_several() {
        let chosen = choose(
            vec![image(None, "linux/amd64"), image(None, "linux/arm64/v8")],
            Platform::parse("linux/arm64"),
        )
        .unwrap();
        assert_eq!(chosen.platform.unwrap().to_string(), "linux/arm64/v8");
    }

    #[test]
    fn test_platform_that_matches_nothing_lists_what_there_is() {
        let error = choose(
            vec![image(None, "linux/amd64")],
            Platform::parse("linux/s390x"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("No linux/s390x image"), "{error}");
        assert!(error.contains("linux/amd64"), "{error}");
    }

    #[test]
    fn test_platform_that_matches_several_is_an_error() {
        let error = choose(
            vec![image(None, "linux/arm/v6"), image(None, "linux/arm/v7")],
            Platform::parse("linux/arm"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("more than one"), "{error}");
    }

    #[test]
    fn test_no_candidates_is_an_error() {
        assert!(choose(vec![], None).is_err());
    }

    #[test]
    fn test_digests_become_blob_paths_and_bad_ones_are_refused() {
        assert_eq!(
            blob_name("sha256:ABCDEF0123").unwrap(),
            "blobs/sha256/abcdef0123"
        );
        assert!(blob_name("sha256:../../etc/passwd").is_err());
        assert!(blob_name("sha256").is_err());
        assert!(blob_name(":abc").is_err());
        assert!(blob_name("sha256:").is_err());
    }

    #[test]
    fn test_manifest_paths_are_kept_inside_the_archive() {
        assert_eq!(archive_path("abc/layer.tar").unwrap(), "abc/layer.tar");
        assert_eq!(archive_path("./blobs/sha256/x").unwrap(), "blobs/sha256/x");
        assert!(archive_path("../x").is_err());
        assert!(archive_path("/etc/passwd").is_err());
        assert!(archive_path("").is_err());
    }

    #[test]
    fn test_a_store_with_neither_index_is_not_an_image() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        let error = select(&store, None).unwrap_err().to_string();
        assert!(error.contains("index.json"), "{error}");
        assert!(error.contains("manifest.json"), "{error}");
    }

    #[test]
    fn test_an_invalid_platform_spec_is_rejected_before_reading() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        let error = select(&store, Some("amd64")).unwrap_err().to_string();
        assert!(error.contains("Invalid --platform"), "{error}");
    }

    #[test]
    fn test_reads_an_oci_layout_through_a_nested_index_and_drops_attestations() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let blobs = root.join("blobs/sha256");
        std::fs::create_dir_all(&blobs).unwrap();
        std::fs::write(blobs.join("c0"), r#"{"os":"linux","architecture":"amd64"}"#).unwrap();
        std::fs::write(
            blobs.join("c1"),
            r#"{"os":"linux","architecture":"arm64","variant":"v8"}"#,
        )
        .unwrap();
        std::fs::write(
            blobs.join("b0"),
            r#"{"config":{"digest":"sha256:c0"},"layers":[{"digest":"sha256:d0"},{"digest":"sha256:d1"}]}"#,
        )
        .unwrap();
        std::fs::write(
            blobs.join("b1"),
            r#"{"config":{"digest":"sha256:c1"},"layers":[{"digest":"sha256:d2"}]}"#,
        )
        .unwrap();
        // The nested index carries platforms on its descriptors, and an attestation manifest.
        std::fs::write(
            blobs.join("a0"),
            r#"{"manifests":[
                {"digest":"sha256:b0","platform":{"os":"linux","architecture":"amd64"}},
                {"digest":"sha256:b1","platform":{"os":"linux","architecture":"arm64","variant":"v8"}},
                {"digest":"sha256:b1","platform":{"os":"unknown","architecture":"unknown"},
                 "annotations":{"vnd.docker.reference.type":"attestation-manifest"}}
            ]}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("index.json"),
            r#"{"manifests":[{"digest":"sha256:a0","annotations":{"io.containerd.image.name":"docker.io/library/app:latest"}}]}"#,
        )
        .unwrap();

        let store = Store::open(root).unwrap();
        let candidates = oci_candidates(&store).unwrap();
        assert_eq!(candidates.len(), 2, "attestation dropped: {candidates:?}");
        assert_eq!(
            candidates[0].name.as_deref(),
            Some("docker.io/library/app:latest"),
            "name inherited from the outer index"
        );
        assert_eq!(
            candidates[0].platform.as_ref().unwrap().to_string(),
            "linux/amd64"
        );
        assert_eq!(
            candidates[0].layers,
            vec!["blobs/sha256/d0", "blobs/sha256/d1"]
        );
        assert_eq!(
            candidates[1].platform.as_ref().unwrap().to_string(),
            "linux/arm64/v8"
        );

        let chosen = select(&store, Some("linux/arm64")).unwrap();
        assert_eq!(chosen.layers, vec!["blobs/sha256/d2"]);

        // A single manifest index with no platform on the descriptor reads it from the config.
        std::fs::write(
            root.join("index.json"),
            r#"{"manifests":[{"digest":"sha256:b0"}]}"#,
        )
        .unwrap();
        let chosen = select(&Store::open(root).unwrap(), None).unwrap();
        assert_eq!(chosen.platform.unwrap().to_string(), "linux/amd64");
        assert_eq!(chosen.name, None);
    }

    #[test]
    fn test_reads_a_legacy_docker_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("aaa")).unwrap();
        std::fs::write(
            root.join("cfg.json"),
            r#"{"os":"linux","architecture":"amd64","variant":""}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("manifest.json"),
            r#"[{"Config":"cfg.json","RepoTags":["app:1.0","app:latest"],"Layers":["aaa/layer.tar","bbb/layer.tar"]}]"#,
        )
        .unwrap();

        let chosen = select(&Store::open(root).unwrap(), None).unwrap();
        assert_eq!(chosen.name.as_deref(), Some("app:1.0"));
        assert_eq!(chosen.platform.unwrap().to_string(), "linux/amd64");
        assert_eq!(chosen.layers, vec!["aaa/layer.tar", "bbb/layer.tar"]);

        // Two images saved together need choosing between.
        std::fs::write(
            root.join("manifest.json"),
            r#"[{"Config":"cfg.json","RepoTags":["app:latest"],"Layers":["aaa/layer.tar"]},
                {"Config":"cfg.json","RepoTags":null,"Layers":["bbb/layer.tar"]}]"#,
        )
        .unwrap();
        let error = select(&Store::open(root).unwrap(), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("holds 2 images"), "{error}");
        assert!(error.contains("app:latest (linux/amd64)"), "{error}");

        // A config that cannot be read costs the platform, not the scan.
        std::fs::write(
            root.join("manifest.json"),
            r#"[{"Config":"missing.json","RepoTags":["app:latest"],"Layers":["aaa/layer.tar"]}]"#,
        )
        .unwrap();
        let chosen = select(&Store::open(root).unwrap(), None).unwrap();
        assert_eq!(chosen.platform, None);
        assert_eq!(chosen.describe(), "app:latest");
        assert!(!Path::new("missing.json").exists());
    }
}
