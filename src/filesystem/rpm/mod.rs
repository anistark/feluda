//! Cataloging installed RPM packages.
//!
//! rpm keeps its installed packages in `/var/lib/rpm`, and unlike apk and dpkg that is a binary
//! store rather than a text file. Which store depends on the rpm that built the image, so the
//! backend is identified by the file it leaves behind:
//!
//! | File | Backend | Where it turns up |
//! |---|---|---|
//! | `rpmdb.sqlite` | sqlite | Fedora 33+, RHEL 9+, and everything built from them |
//! | `Packages.db` | ndb | SUSE and openSUSE |
//! | `Packages` | Berkeley DB | CentOS 7, RHEL 8, Amazon Linux 2 |
//!
//! sqlite and ndb are read. sqlite is the rpm default since 4.16 and covers every Fedora and RHEL
//! derived distribution still in support; ndb is what SUSE builds with. Both hand back the same
//! header blobs, so only the storage layer differs and `header.rs` serves both. Berkeley DB is
//! detected and reported by name rather than as an empty scan, so an image this cannot read never
//! looks like an image with nothing installed.
//!
//! The license side is easier than dpkg's: an rpm header carries a `License` tag, so there is no
//! copyright file to find and nothing to match against free text.

mod header;
mod license;
mod ndb;
mod sqlite;

use std::path::Path;

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::purl::Ecosystem;

use super::{package_finding, Catalog};

/// Where rpm keeps its database, relative to the root of the filesystem being scanned.
pub const DATABASE_PATH: &str = "var/lib/rpm";

/// Where it has moved to. Fedora 36+ and SUSE keep the database under `/usr` so that `/var` can be
/// wiped and `/var/lib/rpm` is a symlink to it. A `docker export` preserves that link and it is
/// relative, so the first path still resolves inside the tree; a tree copied without its symlinks,
/// or one made by `rpm --root`, only has the second.
const SYSIMAGE_PATH: &str = "usr/lib/sysimage/rpm";

/// The table rpm stores headers in, and the column that holds them.
const PACKAGES_TABLE: &str = "Packages";
const BLOB_COLUMN: usize = 1;

/// The stores rpm can leave behind, each identified by the file it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Sqlite,
    Ndb,
    BerkeleyDb,
}

impl Backend {
    /// In the order they are looked for. sqlite first: an image upgraded in place can carry a stale
    /// Berkeley DB alongside the sqlite one rpm actually uses, and the same goes for ndb.
    const ALL: [Backend; 3] = [Backend::Sqlite, Backend::Ndb, Backend::BerkeleyDb];

    fn file(self) -> &'static str {
        match self {
            Backend::Sqlite => "rpmdb.sqlite",
            Backend::Ndb => "Packages.db",
            Backend::BerkeleyDb => "Packages",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Backend::Sqlite => "sqlite",
            Backend::Ndb => "ndb",
            Backend::BerkeleyDb => "Berkeley DB",
        }
    }
}

/// Read every installed package out of an RPM root filesystem.
///
/// `Ok(None)` means the tree has no rpm database at all, which is the normal answer for an Alpine
/// or Debian image. A database that is there in a backend this cannot read is an error, because
/// silently reporting nothing would read as a clean scan of a machine full of packages.
pub fn catalog(root: &Path, namespace: Option<&str>) -> FeludaResult<Option<Catalog>> {
    let Some((file, backend)) = [DATABASE_PATH, SYSIMAGE_PATH]
        .iter()
        .map(|directory| root.join(directory))
        .flat_map(|directory| {
            Backend::ALL
                .iter()
                .map(move |backend| (directory.join(backend.file()), *backend))
        })
        .find(|(path, _)| path.is_file())
    else {
        return Ok(None);
    };

    let blobs = match backend {
        Backend::Sqlite => {
            warn_on_pending_wal(&file);
            sqlite::Database::open(&file)?.column_values(PACKAGES_TABLE, BLOB_COLUMN)?
        }
        Backend::Ndb => ndb::Database::open(&file)?.blobs()?,
        Backend::BerkeleyDb => {
            return Err(backend_error(format!(
                "The rpm database at {} uses the {} backend, which feluda cannot read \
                 (the sqlite and ndb backends are supported). Catalog this image with syft and \
                 scan the result with --sbom-input instead.",
                file.display(),
                backend.name()
            )));
        }
    };
    let catalog = build(&blobs, namespace);

    log(
        LogLevel::Info,
        &format!(
            "Cataloged {} rpm packages from {} {} headers",
            catalog.packages.len(),
            blobs.len(),
            backend.name()
        ),
    );
    Ok(Some(catalog))
}

/// Turn header blobs into findings, and collect the artifact metadata they claim.
fn build(blobs: &[Vec<u8>], namespace: Option<&str>) -> Catalog {
    let mut catalog = Catalog::default();

    for blob in blobs {
        let Some(header) = header::parse(blob) else {
            log(LogLevel::Warn, "Skipped an unreadable rpm header");
            continue;
        };
        if is_pseudo_package(&header.name) {
            continue;
        }

        catalog.owned.extend(header.owned);
        catalog.packages.push(package_finding(
            Ecosystem::Rpm,
            namespace,
            &header.name,
            &header.version,
            header
                .license
                .as_deref()
                .and_then(license::normalize)
                .as_deref(),
        ));
    }

    catalog
}

/// Entries rpm records as packages that are not software.
///
/// `gpg-pubkey` is an imported signing key: its version is a key id and its `License` tag reads
/// literally `pubkey`. Every RPM image has a few, and reporting them would put rows with an
/// unresolvable license in every scan.
fn is_pseudo_package(name: &str) -> bool {
    name == "gpg-pubkey"
}

/// Warn when the database has an unmerged write ahead log.
///
/// rpm runs its database in WAL mode, so a committed change can be sitting in `rpmdb.sqlite-wal`
/// rather than the main file. An exported image has a checkpointed and empty WAL, which is the case
/// that matters, but scanning a live root filesystem can catch one mid-transaction. Saying so beats
/// under-reporting silently.
fn warn_on_pending_wal(database: &Path) {
    let wal = database.with_extension("sqlite-wal");
    if std::fs::metadata(&wal).is_ok_and(|metadata| metadata.len() > 0) {
        log(
            LogLevel::Warn,
            &format!(
                "{} holds uncheckpointed writes; recently installed packages may be missing",
                wal.display()
            ),
        );
    }
}

/// Report an unreadable database and turn it into an error.
///
/// `FeludaError::log` only prints under `--debug`, and an image whose backend is not supported has
/// to say so where the user will see it, not vanish into a zero package report.
pub(super) fn backend_error(message: String) -> FeludaError {
    eprintln!("❌ {message}");
    FeludaError::Parser(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root filesystem holding one rpm database file with the given name.
    fn rootfs(file: &str, content: &[u8]) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join(DATABASE_PATH);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(file), content).unwrap();
        temp
    }

    /// A checked in fixture, copied into a root filesystem shaped tree.
    fn fixture_rootfs(file: &str) -> tempfile::TempDir {
        let fixture = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/rpm")
                .join(file),
        )
        .expect("fixture should exist");
        rootfs(file, &fixture)
    }

    /// Seven sqlite headers taken from `fedora:41`.
    fn fedora_rootfs() -> tempfile::TempDir {
        fixture_rootfs("rpmdb.sqlite")
    }

    /// An ndb store rpm itself wrote inside `opensuse/leap:15.6`: eight packages installed with
    /// `--justdb`, plus the two signing keys the image imports.
    fn opensuse_rootfs() -> tempfile::TempDir {
        fixture_rootfs("Packages.db")
    }

    #[test]
    fn test_no_database_is_not_an_error() {
        let temp = tempfile::tempdir().unwrap();
        assert!(catalog(temp.path(), Some("fedora")).unwrap().is_none());
    }

    #[test]
    fn test_reads_the_fixture() {
        let temp = fedora_rootfs();
        let catalog = catalog(temp.path(), Some("fedora"))
            .unwrap()
            .expect("database is present");

        // Seven headers, less the gpg-pubkey pseudo package.
        assert_eq!(catalog.packages.len(), 6);

        let bzip2 = catalog
            .packages
            .iter()
            .find(|package| package.name == "fedora/bzip2-libs")
            .expect("bzip2-libs missing");
        assert_eq!(bzip2.version, "1.0.8-19.fc41");
        assert_eq!(bzip2.license.as_deref(), Some("BSD-4-Clause"));
        assert_eq!(bzip2.ecosystem, Ecosystem::Rpm);
        assert_eq!(
            bzip2.purl().as_deref(),
            Some("pkg:rpm/fedora/bzip2-libs@1.0.8-19.fc41")
        );
    }

    #[test]
    fn test_every_fixture_package_resolves_a_license() {
        let temp = fedora_rootfs();
        let catalog = catalog(temp.path(), Some("fedora")).unwrap().unwrap();
        for package in &catalog.packages {
            assert!(package.license.is_some(), "{} has no license", package.name);
        }
    }

    #[test]
    fn test_an_and_expression_survives() {
        let temp = fedora_rootfs();
        let catalog = catalog(temp.path(), Some("fedora")).unwrap().unwrap();
        let lz4 = catalog
            .packages
            .iter()
            .find(|package| package.name == "fedora/lz4-libs")
            .expect("lz4-libs missing");
        assert_eq!(
            lz4.license.as_deref(),
            Some("GPL-2.0-or-later AND BSD-2-Clause")
        );
    }

    #[test]
    fn test_the_gpg_pubkey_pseudo_package_is_skipped() {
        let temp = fedora_rootfs();
        let catalog = catalog(temp.path(), Some("fedora")).unwrap().unwrap();
        assert!(
            !catalog
                .packages
                .iter()
                .any(|package| package.name.contains("gpg-pubkey")),
            "gpg-pubkey should not be reported as a package"
        );
    }

    #[test]
    fn test_without_os_release_the_name_has_no_namespace() {
        let temp = fedora_rootfs();
        let catalog = catalog(temp.path(), None).unwrap().unwrap();
        assert!(catalog
            .packages
            .iter()
            .any(|package| package.name == "bzip2-libs"));
    }

    #[test]
    fn test_berkeley_db_names_its_backend() {
        // A CentOS 7 image. Reporting nothing would look like a clean scan.
        let temp = rootfs("Packages", b"\x00\x05\x16\x61 not really bdb");
        let error = catalog(temp.path(), Some("centos")).unwrap_err();
        assert!(
            error.to_string().contains("Berkeley DB"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_reads_the_ndb_fixture() {
        let temp = opensuse_rootfs();
        let catalog = catalog(temp.path(), Some("opensuse-leap"))
            .unwrap()
            .expect("database is present");

        // Ten headers, less the two gpg-pubkey pseudo packages.
        assert_eq!(catalog.packages.len(), 8);
        assert!(
            !catalog
                .packages
                .iter()
                .any(|package| package.name.contains("gpg-pubkey")),
            "gpg-pubkey should not be reported as a package"
        );

        let pam = catalog
            .packages
            .iter()
            .find(|package| package.name == "opensuse-leap/pam")
            .expect("pam missing");
        assert_eq!(pam.version, "1.3.0-150000.6.86.1");
        // SUSE writes SPDX with lowercase operators; only the operator is rewritten.
        assert_eq!(pam.license.as_deref(), Some("GPL-2.0+ OR BSD-3-Clause"));
        assert_eq!(
            pam.purl().as_deref(),
            Some("pkg:rpm/opensuse-leap/pam@1.3.0-150000.6.86.1")
        );

        for package in &catalog.packages {
            assert!(package.license.is_some(), "{} has no license", package.name);
        }
    }

    #[test]
    fn test_ndb_headers_carry_their_file_lists() {
        // The ownership dedupe needs the file list, and it comes out of the same header parser
        // whichever store the blob came from.
        let temp = opensuse_rootfs();
        let catalog = catalog(temp.path(), Some("opensuse-leap"))
            .unwrap()
            .unwrap();
        // Nothing in the fixture ships Python or Node metadata, so nothing is claimed...
        assert!(catalog.owned.is_empty());
        // ...but the headers were read whole, which the largest package's version shows.
        assert!(catalog
            .packages
            .iter()
            .any(|package| package.version == "15.2.0+git10201-150000.1.9.1"));
    }

    #[test]
    fn test_the_database_is_found_under_usr_lib_sysimage() {
        // `rpm --root` and a tree copied without its symlinks have no /var/lib/rpm at all.
        let fixture = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/rpm/Packages.db"
        ))
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join(SYSIMAGE_PATH);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("Packages.db"), fixture).unwrap();

        let catalog = catalog(temp.path(), Some("opensuse-leap"))
            .unwrap()
            .unwrap();
        assert_eq!(catalog.packages.len(), 8);
    }

    #[test]
    fn test_a_corrupt_ndb_store_is_an_error() {
        let temp = rootfs("Packages.db", b"RpmP not really ndb");
        let error = catalog(temp.path(), Some("opensuse")).unwrap_err();
        assert!(
            error.to_string().contains("ndb"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_sqlite_wins_over_a_stale_berkeley_db() {
        // An in-place upgrade can leave the old database behind next to the new one.
        let temp = fedora_rootfs();
        std::fs::write(temp.path().join(DATABASE_PATH).join("Packages"), b"stale").unwrap();

        let catalog = catalog(temp.path(), Some("fedora")).unwrap().unwrap();
        assert_eq!(catalog.packages.len(), 6);
    }

    #[test]
    fn test_a_corrupt_sqlite_database_is_an_error() {
        let temp = rootfs("rpmdb.sqlite", b"not a database at all");
        let error = catalog(temp.path(), Some("fedora")).unwrap_err();
        assert!(
            error.to_string().contains("not a SQLite database"),
            "unexpected error: {error}"
        );
    }
}
