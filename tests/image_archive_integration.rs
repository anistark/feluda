//! Integration tests for scanning image archives (#265).
//!
//! Each test writes the same two layer image out the way a different tool would (an OCI layout
//! directory, that layout tarred, a legacy `docker save` tarball, and that gzipped) and drives the
//! real `feluda` binary against it. The layers carry everything the catalogers need, so nothing
//! here touches the network: OS packages have their licenses in the apk database and the installed
//! artifacts state theirs in their metadata.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

/// Layer one: an Alpine base with two Python distributions installed, one of which the next layer removes.
const APK_INSTALLED: &str = "P:musl\n\
    V:1.2.5-r0\n\
    A:x86_64\n\
    L:MIT\n\
    \n\
    P:zlib\n\
    V:1.3.1-r1\n\
    A:x86_64\n\
    L:Zlib\n\
    \n";

/// The arm64 image in the multi platform archive carries one package the amd64 one does not, so
/// the report says which was scanned.
const APK_INSTALLED_ARM64: &str = "P:musl\n\
    V:1.2.5-r0\n\
    A:aarch64\n\
    L:MIT\n\
    \n\
    P:arm-only\n\
    V:1.0.0-r0\n\
    A:aarch64\n\
    L:MIT\n\
    \n";

const SITE_PACKAGES: &str = "usr/lib/python3.12/site-packages";

/// A tar layer: `(path, Some(content))` is a file, `(path, None)` a directory.
fn layer(entries: &[(&str, Option<&str>)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, content) in entries {
        let mut header = tar::Header::new_gnu();
        match content {
            Some(content) => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, path, content.as_bytes())
                    .expect("failed to add file to layer");
            }
            None => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                header.set_cksum();
                builder
                    .append_data(&mut header, path, std::io::empty())
                    .expect("failed to add directory to layer");
            }
        }
    }
    builder.into_inner().expect("failed to finish layer")
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).expect("failed to gzip");
    encoder.finish().expect("failed to finish gzip")
}

fn metadata(name: &str, version: &str, license: &str) -> String {
    format!(
        "Metadata-Version: 2.4\nName: {name}\nVersion: {version}\nLicense-Expression: {license}\n"
    )
}

/// The base layer: os-release, the apk database, and two Python distributions.
fn base_layer(apk_installed: &str) -> Vec<u8> {
    let removed = format!("{SITE_PACKAGES}/removed-1.0.dist-info/METADATA");
    let bar = format!("{SITE_PACKAGES}/bar-2.0.dist-info/METADATA");
    let removed_metadata = metadata("removed", "1.0", "MIT");
    let bar_metadata = metadata("bar", "2.0", "Apache-2.0");
    layer(&[
        ("etc/", None),
        (
            "etc/os-release",
            Some("NAME=\"Alpine Linux\"\nID=alpine\nVERSION_ID=3.20.3\n"),
        ),
        ("lib/apk/db/installed", Some(apk_installed)),
        (&removed, Some(&removed_metadata)),
        (&bar, Some(&bar_metadata)),
    ])
}

/// The application layer: deletes `removed`, installs a Node package.
fn app_layer() -> Vec<u8> {
    let whiteout = format!("{SITE_PACKAGES}/.wh.removed-1.0.dist-info");
    layer(&[
        (&whiteout, Some("")),
        (
            "app/node_modules/leftpad/package.json",
            Some(r#"{"name":"leftpad","version":"0.0.1","license":"MIT"}"#),
        ),
    ])
}

fn sha256_hex(bytes: &[u8]) -> String {
    // Digests only have to be consistent within the fixture; the reader looks blobs up by name and
    // does not verify content. A short stable hash keeps the fixture free of a hashing dependency.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}{hash:016x}{hash:016x}{hash:016x}")
}

/// Write an OCI image layout into `root` holding one image per `(platform, layers)`.
///
/// Layer blobs are given as they should sit on disk (compressed or not), since media types are
/// not what the reader trusts. With several images the index carries a platform per descriptor,
/// the way `buildx` writes a multi platform build.
fn write_oci_layout(root: &Path, images: &[(&str, &str, Vec<Vec<u8>>)]) {
    let blobs = root.join("blobs/sha256");
    fs::create_dir_all(&blobs).expect("failed to create blobs directory");
    let put = |content: &[u8]| -> String {
        let digest = sha256_hex(content);
        fs::write(blobs.join(&digest), content).expect("failed to write blob");
        digest
    };

    let mut descriptors = Vec::new();
    for (os, arch, layers) in images {
        let config = put(format!(
            r#"{{"os":"{os}","architecture":"{arch}","rootfs":{{"type":"layers"}}}}"#
        )
        .as_bytes());
        let layer_descriptors: Vec<String> = layers
            .iter()
            .map(|layer| {
                let digest = put(layer);
                format!(
                    r#"{{"mediaType":"application/vnd.oci.image.layer.v1.tar","digest":"sha256:{digest}","size":{}}}"#,
                    layer.len()
                )
            })
            .collect();
        let manifest = put(
            format!(
                r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:{config}","size":0}},"layers":[{}]}}"#,
                layer_descriptors.join(",")
            )
            .as_bytes(),
        );
        let platform = if images.len() > 1 {
            format!(r#","platform":{{"os":"{os}","architecture":"{arch}"}}"#)
        } else {
            String::new()
        };
        descriptors.push(format!(
            r#"{{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"sha256:{manifest}","size":0{platform},"annotations":{{"org.opencontainers.image.ref.name":"app:latest"}}}}"#
        ));
    }

    fs::write(root.join("oci-layout"), r#"{"imageLayoutVersion":"1.0.0"}"#)
        .expect("failed to write oci-layout");
    fs::write(
        root.join("index.json"),
        format!(
            r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{}]}}"#,
            descriptors.join(",")
        ),
    )
    .expect("failed to write index.json");
}

/// Tar a directory up the way `skopeo copy ... oci-archive:` does.
fn tar_directory(root: &Path) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append_dir_all(".", root)
        .expect("failed to tar the layout");
    builder.into_inner().expect("failed to finish the archive")
}

/// A legacy `docker save` tarball: `manifest.json`, a config file, and `<id>/layer.tar` per layer.
fn docker_save(layers: &[Vec<u8>]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut add = |path: &str, content: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, content)
            .expect("failed to add to docker save archive");
    };

    let mut layer_paths = Vec::new();
    for (index, layer) in layers.iter().enumerate() {
        let path = format!("layer{index}/layer.tar");
        add(&path, layer);
        layer_paths.push(format!("\"{path}\""));
    }
    add(
        "config.json",
        br#"{"os":"linux","architecture":"amd64","rootfs":{"type":"layers"}}"#,
    );
    // Docker writes manifest.json last; a reader that streams would have to reach the end.
    add(
        "manifest.json",
        format!(
            r#"[{{"Config":"config.json","RepoTags":["app:latest"],"Layers":[{}]}}]"#,
            layer_paths.join(",")
        )
        .as_bytes(),
    );
    builder.into_inner().expect("failed to finish the archive")
}

/// Every test drives the binary with the ClearlyDefined fallback off, since the fixtures resolve
/// every license locally and a network lookup would tie the suite to a third party service.
fn feluda(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(args)
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .output()
        .expect("failed to run feluda binary")
}

fn report(output: &Output) -> Vec<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected a JSON report, got {stdout:?}: {e}"))
}

/// The report reduced to what should be identical however the image was packaged.
fn findings(output: &Output) -> BTreeSet<(String, String, String)> {
    report(output)
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().unwrap_or_default().to_string(),
                entry["version"].as_str().unwrap_or_default().to_string(),
                entry["license"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn expected_findings() -> BTreeSet<(String, String, String)> {
    [
        ("alpine/musl", "1.2.5-r0", "MIT"),
        ("alpine/zlib", "1.3.1-r1", "Zlib"),
        ("bar", "2.0", "Apache-2.0"),
        ("leftpad", "0.0.1", "MIT"),
    ]
    .into_iter()
    .map(|(name, version, license)| (name.to_string(), version.to_string(), license.to_string()))
    .collect()
}

#[test]
fn oci_layout_directory_is_scanned_and_whiteouts_hide_deleted_files() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let layout = temp.path().join("app");
    write_oci_layout(
        &layout,
        &[(
            "linux",
            "amd64",
            vec![gzip(&base_layer(APK_INSTALLED)), app_layer()],
        )],
    );

    let output = feluda(&["--image-archive", layout.to_str().unwrap(), "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let found = findings(&output);
    assert_eq!(found, expected_findings());
    assert!(
        !found.iter().any(|(name, _, _)| name == "removed"),
        "removed was deleted in the second layer and must not be reported"
    );
}

#[test]
fn every_packaging_of_the_same_image_reports_the_same_findings() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let layers = vec![gzip(&base_layer(APK_INSTALLED)), app_layer()];

    let layout = temp.path().join("layout");
    write_oci_layout(&layout, &[("linux", "amd64", layers.clone())]);
    let oci_archive = temp.path().join("app.oci.tar");
    fs::write(&oci_archive, tar_directory(&layout)).expect("failed to write oci archive");
    let saved = temp.path().join("app.tar");
    fs::write(&saved, docker_save(&layers)).expect("failed to write docker save archive");
    let saved_gz = temp.path().join("app.tar.gz");
    fs::write(&saved_gz, gzip(&docker_save(&layers))).expect("failed to write gzipped archive");

    let expected = expected_findings();
    for archive in [&layout, &oci_archive, &saved, &saved_gz] {
        let output = feluda(&["--image-archive", archive.to_str().unwrap(), "--json"]);
        assert!(
            output.status.success(),
            "{}: stderr: {}",
            archive.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(findings(&output), expected, "{}", archive.display());
    }
}

#[test]
fn multi_platform_index_needs_a_platform_and_names_the_choices() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let layout = temp.path().join("multi");
    write_oci_layout(
        &layout,
        &[
            (
                "linux",
                "amd64",
                vec![base_layer(APK_INSTALLED), app_layer()],
            ),
            ("linux", "arm64", vec![base_layer(APK_INSTALLED_ARM64)]),
        ],
    );
    let path = layout.to_str().unwrap();

    let output = feluda(&["--image-archive", path, "--json"]);
    assert!(
        !output.status.success(),
        "a multi platform archive must not be guessed at"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--platform"), "stderr: {stderr}");
    assert!(stderr.contains("linux/amd64"), "stderr: {stderr}");
    assert!(stderr.contains("linux/arm64"), "stderr: {stderr}");

    let output = feluda(&[
        "--image-archive",
        path,
        "--platform",
        "linux/arm64",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let names: BTreeSet<String> = findings(&output)
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();
    assert!(names.contains("alpine/arm-only"), "{names:?}");
    assert!(
        !names.contains("leftpad"),
        "the amd64 app layer is not in the arm64 image: {names:?}"
    );

    let output = feluda(&[
        "--image-archive",
        path,
        "--platform",
        "linux/s390x",
        "--json",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("linux/s390x"), "stderr: {stderr}");
    assert!(stderr.contains("linux/amd64"), "stderr: {stderr}");
}

#[test]
fn sbom_is_generated_from_an_image_archive() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let saved = temp.path().join("app.tar");
    fs::write(
        &saved,
        docker_save(&[base_layer(APK_INSTALLED), app_layer()]),
    )
    .expect("failed to write docker save archive");
    let sbom = temp.path().join("app.spdx.json");

    let output = feluda(&[
        "sbom",
        "spdx",
        "--image-archive",
        saved.to_str().unwrap(),
        "--output",
        sbom.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_str(&fs::read_to_string(&sbom).expect("SBOM should be written"))
            .expect("SBOM should be JSON");
    let purls: BTreeSet<String> = document["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .flat_map(|package| package["externalRefs"].as_array().into_iter().flatten())
        .filter_map(|reference| reference["referenceLocator"].as_str())
        .map(str::to_string)
        .collect();
    assert!(purls.contains("pkg:apk/alpine/musl@1.2.5-r0"), "{purls:?}");
    assert!(purls.contains("pkg:npm/leftpad@0.0.1"), "{purls:?}");
    assert!(
        !purls.iter().any(|purl| purl.contains("pkg:pypi/removed")),
        "{purls:?}"
    );
}

#[test]
fn fail_on_restrictive_gates_an_image_with_no_other_tool() {
    // Which licenses count as restrictive comes from configuration, so the gate is exercised by
    // naming one the image carries rather than by depending on the GitHub license table.
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let saved = temp.path().join("app.tar");
    fs::write(
        &saved,
        docker_save(&[base_layer(APK_INSTALLED), app_layer()]),
    )
    .expect("failed to write docker save archive");
    let path = saved.to_str().unwrap();

    let passing = feluda(&["--image-archive", path, "--fail-on-restrictive", "--json"]);
    assert!(
        passing.status.success(),
        "nothing restrictive by default; stderr: {}",
        String::from_utf8_lossy(&passing.stderr)
    );

    let failing = Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(["--image-archive", path, "--fail-on-restrictive", "--json"])
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .env("FELUDA_LICENSES_RESTRICTIVE", r#"["Zlib"]"#)
        .output()
        .expect("failed to run feluda binary");
    assert!(
        !failing.status.success(),
        "zlib is in the image and configured restrictive, so the gate must fail"
    );
    let zlib = report(&failing)
        .into_iter()
        .find(|entry| entry["name"] == "alpine/zlib")
        .expect("zlib should be reported");
    assert_eq!(zlib["is_restrictive"], Value::Bool(true));
}

#[test]
fn something_that_is_not_an_image_archive_is_refused_with_a_reason() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let output = feluda(&["--image-archive", temp.path().to_str().unwrap(), "--json"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("index.json"), "stderr: {stderr}");
    assert!(stderr.contains("manifest.json"), "stderr: {stderr}");

    let missing = temp.path().join("nope.tar");
    let output = feluda(&["--image-archive", missing.to_str().unwrap(), "--json"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nope.tar"), "stderr: {stderr}");
}

#[test]
fn image_archive_conflicts_with_the_other_sources_and_watch() {
    let output = feluda(&["--image-archive", "app.tar", "--filesystem", "rootfs"]);
    assert!(!output.status.success());
    let output = feluda(&["--image-archive", "app.tar", "--sbom-input", "sbom.json"]);
    assert!(!output.status.success());
    let output = feluda(&["--platform", "linux/amd64"]);
    assert!(
        !output.status.success(),
        "--platform without --image-archive should be rejected"
    );
    let output = feluda(&["--image-archive", "app.tar", "watch"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--image-archive"), "stderr: {stderr}");
}
