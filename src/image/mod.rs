//! A container image as a scan source, from an archive on disk rather than a registry.
//!
//! `--filesystem` reads an extracted tree, which meant an image had to be exported first. This
//! takes the image as tools write it out and does the export itself:
//!
//! ```sh
//! docker save app:latest > app.tar
//! feluda --image-archive app.tar --fail-on-restrictive
//! ```
//!
//! Accepted are a `docker save` tarball (also what `podman save`, `nerdctl save` and `crane pull`
//! write), an OCI image layout directory (`skopeo copy ... oci:dir`, `buildx --output type=oci`
//! untarred), the same layout tarred up, and any of the tar forms gzip or zstd compressed. The
//! [`store`] hides which one it was; [`manifest`] finds the image and its layers in it, and
//! [`layers`] squashes them into a temporary root filesystem that goes through the same catalogers
//! as `--filesystem`. There is no registry client, no auth and no network in any of this, and
//! nothing below the squash knows it came from an image.
//!
//! A multi platform archive is not guessed at. One image is taken as is; more than one needs
//! `--platform`, and the error says what there is.

pub mod layers;
pub mod manifest;
pub mod store;

use std::path::Path;

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::filesystem::scan_tree;
use crate::licenses::LicenseInfo;

/// What `--image-archive` and `--platform` asked for.
#[derive(Debug, Clone)]
pub struct ImageArchive {
    /// The tarball or layout directory.
    pub path: String,
    /// Which image to take out of a multi platform archive, as `os/arch[/variant]`.
    pub platform: Option<String>,
}

/// Catalog everything installed in the image `archive` holds.
///
/// The squashed filesystem lives in a temporary directory for the duration of the scan and is
/// removed with it. An image's worth of disk is the price of reusing the filesystem catalogers as
/// they are; it is also exactly what `docker export | tar -x` costs.
pub fn scan_image(archive: &ImageArchive, strict: bool) -> FeludaResult<Vec<LicenseInfo>> {
    let path = Path::new(&archive.path);
    let store = store::Store::open(path).map_err(announce)?;
    let image = manifest::select(&store, archive.platform.as_deref()).map_err(announce)?;
    log(
        LogLevel::Info,
        &format!(
            "Selected image {} with {} layers from {}",
            image.describe(),
            image.layers.len(),
            path.display()
        ),
    );

    let rootfs = tempfile::tempdir().map_err(|error| {
        FeludaError::TempDir(format!(
            "Failed to create a temporary directory for the image filesystem: {error}"
        ))
    })?;
    layers::squash(&store, &image.layers, rootfs.path()).map_err(announce)?;

    let origin = format!("image {} ({})", path.display(), image.describe());
    scan_tree(rootfs.path(), strict, &origin)
}

/// Say what went wrong on the way to the scan.
///
/// `FeludaError::log` only prints under `--debug`, and someone whose archive could not be read
/// needs to be told why before the process exits 1.
fn announce(error: FeludaError) -> FeludaError {
    eprintln!("❌ {error}");
    error
}
