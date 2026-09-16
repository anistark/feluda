//! Squashing an image's layers into one root filesystem.
//!
//! An image is a stack of tar archives. Applying them in order into one directory, with each
//! layer's whiteouts removing what earlier layers put there, gives the filesystem a container
//! would see, and that is what the `--filesystem` catalogers read.
//!
//! Entries are written by hand rather than through `tar::Entry::unpack`, for two reasons. Modes are
//! never applied: a directory a lower layer made read-only would otherwise block the next layer
//! from writing into it, and nothing downstream cares about permissions. And every write is
//! checked to land inside the root after symlinks resolve, with anything already at the
//! destination removed first, so a lower layer's symlink to `/etc` cannot turn a later layer's
//! `etc/passwd` into a write on the host.
//!
//! Whiteouts (`.wh.<name>` hides a sibling, `.wh..wh..opq` hides everything under the directory)
//! only hide what lower layers put there; a file in the same layer stays. Tar order is arbitrary,
//! so the whiteouts are noted during the pass and applied at the end against the set of paths the
//! layer wrote, which comes out right without reading the layer twice.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use tar::EntryType;

use crate::cli::with_spinner;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};

use super::store::{decompress, Store};

/// The prefix a whiteout entry's file name carries.
const WHITEOUT_PREFIX: &str = ".wh.";
/// The whiteout that hides a whole directory's lower contents.
const OPAQUE_WHITEOUT: &str = ".wh..wh..opq";

/// Apply `layers`, in order, into `root`.
pub fn squash(store: &Store, layers: &[String], root: &Path) -> FeludaResult<()> {
    let total = layers.len();
    for (index, layer) in layers.iter().enumerate() {
        let label = format!("🐳: layer {}/{total}", index + 1);
        with_spinner(&label, |indicator| {
            let raw = store.stream(layer)?;
            let reader = decompress(raw).map_err(|error| {
                FeludaError::Image(format!("Failed to read layer {layer}: {error}"))
            })?;
            let mut archive = tar::Archive::new(reader);
            archive.set_ignore_zeros(true);
            let applied = apply(&mut archive, root).map_err(|error| {
                FeludaError::Image(format!("Failed to apply layer {layer}: {error}"))
            })?;
            indicator.update_progress(&format!(
                "{} entr{}",
                applied.written,
                if applied.written == 1 { "y" } else { "ies" }
            ));
            log(
                LogLevel::Info,
                &format!(
                    "Layer {}/{total}: {} entries written, {} skipped, {} whiteouts",
                    index + 1,
                    applied.written,
                    applied.skipped,
                    applied.whiteouts
                ),
            );
            Ok::<(), FeludaError>(())
        })?;
    }
    Ok(())
}

/// What applying one layer did, for the log.
#[derive(Debug, Default, PartialEq, Eq)]
struct Applied {
    written: usize,
    skipped: usize,
    whiteouts: usize,
}

/// A whiteout noted during the pass: hide `name` under `dir`, or everything under `dir`.
struct Whiteout {
    dir: PathBuf,
    name: Option<PathBuf>,
}

/// Extract one layer's entries into `root`, then apply its whiteouts.
fn apply<R: io::Read>(archive: &mut tar::Archive<R>, root: &Path) -> io::Result<Applied> {
    let root = Root::new(root)?;
    let mut applied = Applied::default();
    let mut written: HashSet<PathBuf> = HashSet::new();
    let mut whiteouts: Vec<Whiteout> = Vec::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let Some(relative) = normalize(&entry.path()?) else {
            applied.skipped += 1;
            continue;
        };

        if let Some(whiteout) = whiteout(&relative) {
            whiteouts.push(whiteout);
            applied.whiteouts += 1;
            continue;
        }

        let destination = root.path.join(&relative);
        let kind = entry.header().entry_type();
        let outcome = match kind {
            EntryType::Directory => write_directory(&root, &destination),
            EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse => {
                write_file(&root, &destination, &mut entry)
            }
            EntryType::Symlink => match entry.link_name()? {
                Some(target) => write_symlink(&root, &destination, &target, kind),
                None => Ok(false),
            },
            EntryType::Link => match entry.link_name()?.as_deref().and_then(normalize) {
                Some(target) => write_hard_link(&root, &destination, &root.path.join(target)),
                None => Ok(false),
            },
            // Devices, fifos and the rest are not files a cataloger reads.
            _ => Ok(false),
        };

        match outcome {
            Ok(true) => {
                written.insert(relative);
                applied.written += 1;
            }
            Ok(false) => applied.skipped += 1,
            Err(error) => {
                log(
                    LogLevel::Warn,
                    &format!("Skipping {}: {error}", relative.display()),
                );
                applied.skipped += 1;
            }
        }
    }

    for whiteout in whiteouts {
        match whiteout.name {
            Some(name) => hide(&root.path, &whiteout.dir.join(name), &written)?,
            None => prune(&root.path, &whiteout.dir, &written)?,
        }
    }

    Ok(applied)
}

/// A tar path as a path under the root: no `.`, and nothing absolute or climbing. `None` is an
/// entry to skip, which includes the `./` entry for the root itself.
fn normalize(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if normalized.as_os_str().is_empty() {
        return None;
    }
    Some(normalized)
}

/// What a whiteout entry asks for, if the path is one.
fn whiteout(relative: &Path) -> Option<Whiteout> {
    let name = relative.file_name()?.to_str()?;
    let dir = relative.parent().map(Path::to_path_buf).unwrap_or_default();
    if name == OPAQUE_WHITEOUT {
        return Some(Whiteout { dir, name: None });
    }
    let hidden = name.strip_prefix(WHITEOUT_PREFIX)?;
    if hidden.is_empty() {
        return None;
    }
    Some(Whiteout {
        dir,
        name: Some(PathBuf::from(hidden)),
    })
}

/// The directory layers are squashed into, with its canonical form kept so the check every write
/// makes against it does not resolve it again.
struct Root {
    path: PathBuf,
    canonical: PathBuf,
}

impl Root {
    fn new(path: &Path) -> io::Result<Root> {
        Ok(Root {
            path: path.to_path_buf(),
            canonical: path.canonicalize()?,
        })
    }

    /// Whether an existing `path` is inside the root once symlinks are followed.
    fn contains(&self, path: &Path) -> io::Result<bool> {
        Ok(path.canonicalize()?.starts_with(&self.canonical))
    }

    /// Make the parent directory of `destination`, if it lands inside the root.
    ///
    /// The check is made on the deepest ancestor that already exists, before anything is created:
    /// a lower layer's symlink to the host would otherwise have `create_dir_all` building
    /// directories out there.
    fn prepare(&self, destination: &Path) -> io::Result<bool> {
        let parent = destination.parent().unwrap_or(&self.path);
        let mut existing = parent;
        while fs::symlink_metadata(existing).is_err() {
            existing = existing.parent().unwrap_or(&self.path);
        }
        if !self.contains(existing)? {
            return Ok(false);
        }
        fs::create_dir_all(parent)?;
        Ok(true)
    }
}

/// Remove whatever is at `destination` when it is not a directory, or when `keep_directory` is
/// false and it is one. A symlink is removed as a symlink, never followed.
fn clear(destination: &Path, keep_directory: bool) -> io::Result<()> {
    let Ok(metadata) = fs::symlink_metadata(destination) else {
        return Ok(());
    };
    if metadata.is_dir() {
        if !keep_directory {
            fs::remove_dir_all(destination)?;
        }
    } else {
        fs::remove_file(destination)?;
    }
    Ok(())
}

fn write_directory(root: &Root, destination: &Path) -> io::Result<bool> {
    if !root.prepare(destination)? {
        return Ok(false);
    }
    clear(destination, true)?;
    fs::create_dir_all(destination)?;
    Ok(true)
}

fn write_file<R: io::Read>(root: &Root, destination: &Path, content: &mut R) -> io::Result<bool> {
    if !root.prepare(destination)? {
        return Ok(false);
    }
    clear(destination, false)?;
    let mut file = fs::File::create(destination)?;
    io::copy(content, &mut file)?;
    Ok(true)
}

fn write_symlink(
    root: &Root,
    destination: &Path,
    target: &Path,
    kind: EntryType,
) -> io::Result<bool> {
    if !root.prepare(destination)? {
        return Ok(false);
    }
    clear(destination, false)?;
    symlink(target, destination, kind)?;
    Ok(true)
}

#[cfg(unix)]
fn symlink(target: &Path, destination: &Path, _kind: EntryType) -> io::Result<()> {
    std::os::unix::fs::symlink(target, destination)
}

#[cfg(windows)]
fn symlink(target: &Path, destination: &Path, _kind: EntryType) -> io::Result<()> {
    // Windows needs to know which kind it is making; a target that is not there yet is a file.
    if destination
        .parent()
        .map(|parent| parent.join(target))
        .is_some_and(|resolved| resolved.is_dir())
    {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
}

/// A hard link points at a file earlier in the same layer. Linking is the cheap way; a copy is
/// the fallback when the filesystem will not link, and there is nothing to do when the target
/// was itself skipped.
fn write_hard_link(root: &Root, destination: &Path, target: &Path) -> io::Result<bool> {
    if !root.prepare(destination)? {
        return Ok(false);
    }
    if !target.is_file() {
        return Ok(false);
    }
    clear(destination, false)?;
    if fs::hard_link(target, destination).is_err() {
        fs::copy(target, destination)?;
    }
    Ok(true)
}

/// Apply a `.wh.<name>` whiteout: remove `path` unless this layer wrote it, or wrote into it.
fn hide(root: &Path, path: &Path, written: &HashSet<PathBuf>) -> io::Result<()> {
    let absolute = root.join(path);
    let Ok(metadata) = fs::symlink_metadata(&absolute) else {
        return Ok(());
    };
    if written.contains(path) {
        // The layer replaced it with something of its own; a whiteout cannot hide that. What it
        // can still hide is lower content underneath a replaced directory.
        if metadata.is_dir() {
            prune(root, path, written)?;
        }
        return Ok(());
    }
    if metadata.is_dir() {
        prune(root, path, written)?;
        if fs::read_dir(&absolute)?.next().is_none() {
            fs::remove_dir(&absolute)?;
        }
    } else {
        fs::remove_file(&absolute)?;
    }
    Ok(())
}

/// Apply an opaque whiteout: remove everything under `dir` that this layer did not write.
/// Directories the layer did not write stay only while something written remains inside them.
fn prune(root: &Path, dir: &Path, written: &HashSet<PathBuf>) -> io::Result<()> {
    let absolute = root.join(dir);
    let Ok(metadata) = fs::symlink_metadata(&absolute) else {
        return Ok(());
    };
    if !metadata.is_dir() {
        return Ok(());
    }
    for child in fs::read_dir(&absolute)? {
        let child = child?;
        let relative = dir.join(child.file_name());
        let metadata = child.metadata()?;
        if metadata.is_dir() && !metadata.is_symlink() {
            prune(root, &relative, written)?;
            if !written.contains(&relative) && fs::read_dir(child.path())?.next().is_none() {
                fs::remove_dir(child.path())?;
            }
        } else if !written.contains(&relative) {
            fs::remove_file(child.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A layer tar built from a description: (`path`, `Some(content)` for a file, `None` for a
    /// directory). Whiteouts are files like any other.
    fn layer(entries: &[(&str, Option<&str>)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, content) in entries {
            let mut header = tar::Header::new_gnu();
            match content {
                Some(content) => {
                    header.set_entry_type(EntryType::Regular);
                    header.set_size(content.len() as u64);
                    header.set_mode(0o644);
                    header.set_cksum();
                    builder
                        .append_data(&mut header, path, content.as_bytes())
                        .unwrap();
                }
                None => {
                    header.set_entry_type(EntryType::Directory);
                    header.set_size(0);
                    header.set_mode(0o555);
                    header.set_cksum();
                    builder.append_data(&mut header, path, io::empty()).unwrap();
                }
            }
        }
        builder.into_inner().unwrap()
    }

    fn symlink_layer(link: &str, target: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        builder.append_link(&mut header, link, target).unwrap();
        builder.into_inner().unwrap()
    }

    fn apply_bytes(bytes: Vec<u8>, root: &Path) -> Applied {
        let mut archive = tar::Archive::new(Cursor::new(bytes));
        apply(&mut archive, root).unwrap()
    }

    fn read(root: &Path, relative: &str) -> Option<String> {
        fs::read_to_string(root.join(relative)).ok()
    }

    #[test]
    fn test_later_layers_overwrite_earlier_ones() {
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(
            layer(&[("etc/", None), ("etc/os-release", Some("ID=alpine\n"))]),
            temp.path(),
        );
        apply_bytes(
            layer(&[("etc/os-release", Some("ID=debian\n"))]),
            temp.path(),
        );
        assert_eq!(
            read(temp.path(), "etc/os-release").as_deref(),
            Some("ID=debian\n")
        );
    }

    #[test]
    fn test_read_only_directory_from_a_lower_layer_does_not_block_the_next() {
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(layer(&[("locked/", None)]), temp.path());
        let applied = apply_bytes(layer(&[("locked/new", Some("x"))]), temp.path());
        assert_eq!(applied.written, 1);
        assert_eq!(read(temp.path(), "locked/new").as_deref(), Some("x"));
    }

    #[test]
    fn test_whiteout_removes_a_lower_file_and_is_not_extracted_itself() {
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(
            layer(&[("app/a.txt", Some("a")), ("app/b.txt", Some("b"))]),
            temp.path(),
        );
        let applied = apply_bytes(layer(&[("app/.wh.a.txt", Some(""))]), temp.path());
        assert_eq!(applied.whiteouts, 1);
        assert!(!temp.path().join("app/a.txt").exists());
        assert!(!temp.path().join("app/.wh.a.txt").exists());
        assert_eq!(read(temp.path(), "app/b.txt").as_deref(), Some("b"));
    }

    #[test]
    fn test_whiteout_removes_a_lower_directory_tree() {
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(
            layer(&[
                (
                    "site-packages/foo-1.0.dist-info/METADATA",
                    Some("Name: foo"),
                ),
                ("site-packages/keep.py", Some("")),
            ]),
            temp.path(),
        );
        apply_bytes(
            layer(&[("site-packages/.wh.foo-1.0.dist-info", Some(""))]),
            temp.path(),
        );
        assert!(!temp.path().join("site-packages/foo-1.0.dist-info").exists());
        assert!(temp.path().join("site-packages/keep.py").exists());
    }

    #[test]
    fn test_whiteout_does_not_hide_a_file_from_its_own_layer() {
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(layer(&[("app/a.txt", Some("lower"))]), temp.path());
        // Same layer: the whiteout and a replacement, in either order.
        apply_bytes(
            layer(&[("app/.wh.a.txt", Some("")), ("app/a.txt", Some("upper"))]),
            temp.path(),
        );
        assert_eq!(read(temp.path(), "app/a.txt").as_deref(), Some("upper"));

        apply_bytes(
            layer(&[("app/a.txt", Some("upper2")), ("app/.wh.a.txt", Some(""))]),
            temp.path(),
        );
        assert_eq!(read(temp.path(), "app/a.txt").as_deref(), Some("upper2"));
    }

    #[test]
    fn test_whiteout_of_a_directory_keeps_what_the_same_layer_put_inside() {
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(layer(&[("d/old", Some("old"))]), temp.path());
        apply_bytes(
            layer(&[("d/new", Some("new")), (".wh.d", Some(""))]),
            temp.path(),
        );
        assert!(!temp.path().join("d/old").exists());
        assert_eq!(read(temp.path(), "d/new").as_deref(), Some("new"));
    }

    #[test]
    fn test_opaque_whiteout_hides_lower_contents_but_not_the_layers_own() {
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(
            layer(&[
                ("d/lower.txt", Some("")),
                ("d/sub/deep.txt", Some("")),
                ("d/shared/lower.txt", Some("")),
                ("d/emptydir/", None),
            ]),
            temp.path(),
        );
        let applied = apply_bytes(
            layer(&[
                ("d/.wh..wh..opq", Some("")),
                ("d/upper.txt", Some("")),
                ("d/shared/upper.txt", Some("")),
                ("d/kept/", None),
            ]),
            temp.path(),
        );
        assert_eq!(applied.whiteouts, 1);
        let root = temp.path();
        assert!(!root.join("d/lower.txt").exists());
        assert!(
            !root.join("d/sub").exists(),
            "unwritten dir emptied and removed"
        );
        assert!(!root.join("d/emptydir").exists());
        assert!(!root.join("d/shared/lower.txt").exists());
        assert!(root.join("d/shared/upper.txt").exists());
        assert!(root.join("d/upper.txt").exists());
        assert!(
            root.join("d/kept").is_dir(),
            "a directory the layer wrote stays even if empty"
        );
    }

    #[test]
    fn test_whiteout_of_something_absent_is_fine() {
        let temp = tempfile::tempdir().unwrap();
        let applied = apply_bytes(
            layer(&[("x/.wh.nothing", Some("")), ("y/.wh..wh..opq", Some(""))]),
            temp.path(),
        );
        assert_eq!(applied.whiteouts, 2);
        assert!(!temp.path().join("x/.wh.nothing").exists());
    }

    #[test]
    fn test_climbing_and_absolute_paths_are_skipped() {
        let temp = tempfile::tempdir().unwrap();
        // The tar builder refuses to write `..` itself, so the header is filled in by hand.
        let mut builder = tar::Builder::new(Vec::new());
        for name in ["../escape", "/escape"] {
            let mut header = tar::Header::new_gnu();
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
            header.set_size(0);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, io::empty()).unwrap();
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, "ok", io::empty()).unwrap();
        let applied = apply_bytes(builder.into_inner().unwrap(), temp.path());
        assert_eq!(applied.written, 1);
        assert_eq!(applied.skipped, 2);
        assert!(!temp.path().parent().unwrap().join("escape").exists());
        assert!(!Path::new("/escape").exists());
    }

    #[test]
    fn test_a_lower_symlink_out_of_the_root_is_not_written_through() {
        let outside = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(
            symlink_layer("etc", outside.path().to_str().unwrap()),
            temp.path(),
        );
        // A file under the symlinked directory would land outside the root: skipped, and so is
        // one deeper down, without its parent being created out there first.
        let applied = apply_bytes(
            layer(&[
                ("etc/passwd", Some("root:x")),
                ("etc/ssl/certs/ca.pem", Some("cert")),
            ]),
            temp.path(),
        );
        assert_eq!(applied.skipped, 2);
        assert!(!outside.path().join("passwd").exists());
        assert!(!outside.path().join("ssl").exists());

        // A file replacing the symlink itself is fine: the symlink is removed, not followed.
        apply_bytes(
            symlink_layer("hosts", outside.path().join("hosts").to_str().unwrap()),
            temp.path(),
        );
        fs::write(outside.path().join("hosts"), "host copy").unwrap();
        apply_bytes(layer(&[("hosts", Some("image copy"))]), temp.path());
        assert_eq!(
            fs::read_to_string(outside.path().join("hosts")).unwrap(),
            "host copy"
        );
        assert_eq!(read(temp.path(), "hosts").as_deref(), Some("image copy"));
    }

    #[test]
    fn test_a_relative_symlink_inside_the_root_is_written_through() {
        // Debian's merged /usr: `lib -> usr/lib`, and a later layer adds `lib/x`.
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(layer(&[("usr/lib/", None)]), temp.path());
        apply_bytes(symlink_layer("lib", "usr/lib"), temp.path());
        let applied = apply_bytes(layer(&[("lib/x", Some("x"))]), temp.path());
        assert_eq!(applied.written, 1);
        assert_eq!(read(temp.path(), "usr/lib/x").as_deref(), Some("x"));
    }

    #[test]
    fn test_a_directory_replaces_a_lower_file_and_a_file_replaces_a_lower_directory() {
        let temp = tempfile::tempdir().unwrap();
        apply_bytes(
            layer(&[("was-file", Some("")), ("was-dir/child", Some(""))]),
            temp.path(),
        );
        apply_bytes(
            layer(&[("was-file/", None), ("was-dir", Some("now a file"))]),
            temp.path(),
        );
        assert!(temp.path().join("was-file").is_dir());
        assert_eq!(read(temp.path(), "was-dir").as_deref(), Some("now a file"));
    }

    #[test]
    fn test_hard_links_point_at_the_target_in_the_same_layer() {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "bin/busybox", b"abc".as_slice())
            .unwrap();
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(EntryType::Link);
        link.set_size(0);
        link.set_cksum();
        builder
            .append_link(&mut link, "bin/sh", "bin/busybox")
            .unwrap();
        let applied = apply_bytes(builder.into_inner().unwrap(), temp.path());
        assert_eq!(applied.written, 2);
        assert_eq!(read(temp.path(), "bin/sh").as_deref(), Some("abc"));
    }

    #[test]
    fn test_normalize_and_whiteout_recognition() {
        assert_eq!(normalize(Path::new("./a/b")), Some(PathBuf::from("a/b")));
        assert_eq!(normalize(Path::new("./")), None);
        assert_eq!(normalize(Path::new("a/../b")), None);
        assert_eq!(normalize(Path::new("/a")), None);

        let opaque = whiteout(Path::new("a/b/.wh..wh..opq")).unwrap();
        assert_eq!(opaque.dir, PathBuf::from("a/b"));
        assert!(opaque.name.is_none());
        let single = whiteout(Path::new("a/.wh.file")).unwrap();
        assert_eq!(single.dir, PathBuf::from("a"));
        assert_eq!(single.name, Some(PathBuf::from("file")));
        let top = whiteout(Path::new(".wh.file")).unwrap();
        assert_eq!(top.dir, PathBuf::new());
        assert!(whiteout(Path::new("a/.wh.")).is_none());
        assert!(whiteout(Path::new("a/file")).is_none());
        assert!(whiteout(Path::new("a/.whatever")).is_none());
    }
}
