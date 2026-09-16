//! Where an image archive keeps its files: a directory, or a tar file indexed once.
//!
//! An OCI layout on disk is a directory of blobs. A `docker save` tarball is the same set of files
//! inside one tar. Both are read through [`Store`], so the manifest reader and the layer squasher
//! never know which one they were given.
//!
//! A tar file is indexed by walking its headers once, recording where each entry's bytes start and
//! how long they are, and after that every blob is read by seeking. That is what keeps an image of
//! several gigabytes from being held in memory: the index is a few hundred names, and a layer is a
//! `Take<File>` that the tar reader streams through. A gzip or zstd compressed archive
//! (`docker save app | gzip > app.tar.gz`) is decompressed into an anonymous temp file first, since
//! `docker save` writes the manifest last and a compressed stream cannot seek to it.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use tar::EntryType;

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};

/// The gzip magic, the first two bytes of every member.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
/// The zstd frame magic, little endian `0xFD2FB528`.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// How a stream of bytes is compressed, from its first bytes rather than from what a manifest
/// claims about it. Media types are wrong often enough (a `+gzip` layer that is plain tar, a
/// docker save layer with no media type at all) that sniffing is the only reliable answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Zstd,
}

impl Compression {
    /// Sniff the compression from the first bytes of a stream.
    pub fn sniff(head: &[u8]) -> Self {
        if head.starts_with(&GZIP_MAGIC) {
            Compression::Gzip
        } else if head.starts_with(&ZSTD_MAGIC) {
            Compression::Zstd
        } else {
            Compression::None
        }
    }
}

/// Wrap a raw stream in whichever decoder its first bytes call for.
///
/// The bytes read for the sniff are put back in front, so the caller sees the whole stream. An
/// empty stream comes back as an empty plain stream, which a tar reader takes as a tar with no
/// entries; that is what an "empty layer" blob looks like.
pub fn decompress(mut raw: Box<dyn Read>) -> io::Result<Box<dyn Read>> {
    let mut head = [0u8; 4];
    let mut filled = 0;
    while filled < head.len() {
        let read = raw.read(&mut head[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    let compression = Compression::sniff(&head[..filled]);
    let stream: Box<dyn Read> = Box::new(io::Cursor::new(head[..filled].to_vec()).chain(raw));
    Ok(match compression {
        Compression::None => stream,
        Compression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(stream)),
        Compression::Zstd => Box::new(ZstdFrames::new(stream)?),
    })
}

/// A zstd stream of any number of frames.
///
/// `ruzstd`'s `StreamingDecoder` stops at the end of the first frame, and a layer written by
/// zstd:chunked or by any tool that flushes per chunk is a sequence of frames, some of them
/// skippable metadata. This reads frame after frame until the source runs out.
struct ZstdFrames<R: Read> {
    source: R,
    decoder: ruzstd::decoding::FrameDecoder,
    /// Set once the source has no further frame; the decode buffer may still hold bytes.
    exhausted: bool,
}

impl<R: Read> ZstdFrames<R> {
    fn new(mut source: R) -> io::Result<Self> {
        let mut decoder = ruzstd::decoding::FrameDecoder::new();
        let exhausted = !Self::start_frame(&mut decoder, &mut source)?;
        Ok(Self {
            source,
            decoder,
            exhausted,
        })
    }

    /// Position the decoder on the next real frame, skipping skippable ones. `Ok(false)` means the
    /// source has ended cleanly.
    fn start_frame(
        decoder: &mut ruzstd::decoding::FrameDecoder,
        source: &mut R,
    ) -> io::Result<bool> {
        use ruzstd::decoding::errors::{FrameDecoderError, ReadFrameHeaderError};
        loop {
            match decoder.reset(&mut *source) {
                Ok(()) => return Ok(true),
                Err(FrameDecoderError::ReadFrameHeaderError(ReadFrameHeaderError::SkipFrame {
                    length,
                    ..
                })) => {
                    io::copy(&mut source.take(u64::from(length)), &mut io::sink())?;
                }
                Err(FrameDecoderError::ReadFrameHeaderError(
                    ReadFrameHeaderError::MagicNumberReadError(error),
                )) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(error) => return Err(io::Error::other(error)),
            }
        }
    }
}

impl<R: Read> Read for ZstdFrames<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use ruzstd::decoding::BlockDecodingStrategy;
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if !self.exhausted {
                while self.decoder.can_collect() < buf.len() && !self.decoder.is_finished() {
                    let wanted = buf.len() - self.decoder.can_collect();
                    self.decoder
                        .decode_blocks(&mut self.source, BlockDecodingStrategy::UptoBytes(wanted))
                        .map_err(io::Error::other)?;
                }
            }
            let read = self.decoder.read(buf)?;
            if read > 0 || self.exhausted {
                return Ok(read);
            }
            // This frame is finished and drained; move on to the next one, if there is one.
            self.exhausted = !Self::start_frame(&mut self.decoder, &mut self.source)?;
        }
    }
}

/// Where one file's bytes sit inside a tar: offset of the first data byte, and length.
type Span = (u64, u64);

/// An image archive's files, however they are stored.
pub enum Store {
    /// An OCI layout directory: files are files.
    Directory(PathBuf),
    /// A tar file (or a compressed one, already decompressed into `file`): files are spans of it.
    Archive {
        file: File,
        entries: HashMap<String, Span>,
    },
}

impl Store {
    /// Open an archive path: a directory as is, a tar file by indexing it.
    pub fn open(path: &Path) -> FeludaResult<Store> {
        if path.is_dir() {
            return Ok(Store::Directory(path.to_path_buf()));
        }
        if !path.is_file() {
            return Err(FeludaError::Image(format!(
                "Not found: {}. --image-archive takes a docker save tarball or an OCI image layout directory.",
                path.display()
            )));
        }

        let mut file = File::open(path).map_err(|error| {
            FeludaError::Image(format!("Failed to open {}: {error}", path.display()))
        })?;
        let mut head = [0u8; 4];
        let filled = file.read(&mut head).map_err(|error| {
            FeludaError::Image(format!("Failed to read {}: {error}", path.display()))
        })?;
        file.rewind()
            .map_err(|error| FeludaError::Image(format!("Failed to rewind archive: {error}")))?;

        let file = match Compression::sniff(&head[..filled]) {
            Compression::None => file,
            compression => {
                log(
                    LogLevel::Info,
                    &format!("Decompressing {compression:?} archive to a temporary file"),
                );
                let mut plain = tempfile::tempfile().map_err(|error| {
                    FeludaError::Image(format!(
                        "Failed to create a temporary file for the decompressed archive: {error}"
                    ))
                })?;
                let mut reader = decompress(Box::new(file)).map_err(|error| {
                    FeludaError::Image(format!("Failed to decompress {}: {error}", path.display()))
                })?;
                io::copy(&mut reader, &mut plain).map_err(|error| {
                    FeludaError::Image(format!("Failed to decompress {}: {error}", path.display()))
                })?;
                plain.rewind().map_err(|error| {
                    FeludaError::Image(format!("Failed to rewind decompressed archive: {error}"))
                })?;
                plain
            }
        };

        let entries = index_tar(&file).map_err(|error| {
            FeludaError::Image(format!(
                "Failed to read {} as a tar archive: {error}",
                path.display()
            ))
        })?;
        Ok(Store::Archive { file, entries })
    }

    /// Whether the store holds a file by this name.
    pub fn has(&self, name: &str) -> bool {
        match self {
            Store::Directory(root) => root.join(name).is_file(),
            Store::Archive { entries, .. } => entries.contains_key(name),
        }
    }

    /// Read a whole file. For the small ones: indexes, manifests, configs.
    pub fn read(&self, name: &str) -> FeludaResult<Vec<u8>> {
        let mut content = Vec::new();
        self.stream(name)?
            .read_to_end(&mut content)
            .map_err(|error| FeludaError::Image(format!("Failed to read {name}: {error}")))?;
        Ok(content)
    }

    /// Stream a file's raw bytes. Layers go through here, and through [`decompress`] after.
    pub fn stream(&self, name: &str) -> FeludaResult<Box<dyn Read>> {
        match self {
            Store::Directory(root) => {
                let path = root.join(name);
                File::open(&path)
                    .map(|file| Box::new(file) as Box<dyn Read>)
                    .map_err(|error| {
                        FeludaError::Image(format!("Failed to open {}: {error}", path.display()))
                    })
            }
            Store::Archive { file, entries } => {
                let (offset, size) = entries
                    .get(name)
                    .copied()
                    .ok_or_else(|| FeludaError::Image(format!("The archive has no {name}")))?;
                let mut file = file.try_clone().map_err(|error| {
                    FeludaError::Image(format!("Failed to reopen the archive: {error}"))
                })?;
                file.seek(SeekFrom::Start(offset)).map_err(|error| {
                    FeludaError::Image(format!("Failed to seek to {name}: {error}"))
                })?;
                Ok(Box::new(file.take(size)))
            }
        }
    }
}

/// Walk a tar's headers and record where each regular file's bytes are.
///
/// Hard links are recorded as the file they point at, which is how a shared layer appears when
/// several images are saved into one archive. Directories and everything else are not files and
/// are not recorded.
fn index_tar(file: &File) -> io::Result<HashMap<String, Span>> {
    let mut archive = tar::Archive::new(file.try_clone()?);
    let mut entries: HashMap<String, Span> = HashMap::new();
    for entry in archive.entries_with_seek()? {
        let entry = entry?;
        let Some(name) = entry_name(&entry.path()?) else {
            continue;
        };
        match entry.header().entry_type() {
            EntryType::Regular | EntryType::Continuous => {
                entries.insert(name, (entry.raw_file_position(), entry.size()));
            }
            EntryType::Link => {
                let target = entry.link_name()?.and_then(|target| entry_name(&target));
                if let Some(span) = target.and_then(|target| entries.get(&target).copied()) {
                    entries.insert(name, span);
                }
            }
            _ => {}
        }
    }
    Ok(entries)
}

/// A tar entry's path as a store name: forward slashes, no leading `./`, nothing that climbs.
fn entry_name(path: &Path) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A tar in memory holding the given files, with `link` as a hard link to the last one.
    fn tar_with(files: &[(&str, &[u8])], link: Option<&str>) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *content).unwrap();
        }
        if let Some(link) = link {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(EntryType::Link);
            header.set_size(0);
            header.set_cksum();
            builder
                .append_link(&mut header, link, files[files.len() - 1].0)
                .unwrap();
        }
        builder.into_inner().unwrap()
    }

    fn write_temp(content: &[u8]) -> tempfile::NamedTempFile {
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        temp.write_all(content).unwrap();
        temp.flush().unwrap();
        temp
    }

    #[test]
    fn test_sniffs_compression_from_magic_bytes() {
        assert_eq!(
            Compression::sniff(&[0x1f, 0x8b, 0x08, 0x00]),
            Compression::Gzip
        );
        assert_eq!(
            Compression::sniff(&[0x28, 0xb5, 0x2f, 0xfd, 0x00]),
            Compression::Zstd
        );
        assert_eq!(Compression::sniff(b"ustar"), Compression::None);
        assert_eq!(Compression::sniff(&[]), Compression::None);
    }

    #[test]
    fn test_decompress_passes_plain_bytes_through_including_the_sniffed_head() {
        let mut out = Vec::new();
        decompress(Box::new(io::Cursor::new(b"hello world".to_vec())))
            .unwrap()
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, b"hello world");

        let mut out = Vec::new();
        decompress(Box::new(io::Cursor::new(b"hi".to_vec())))
            .unwrap()
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, b"hi");

        let mut out = Vec::new();
        decompress(Box::new(io::empty()))
            .unwrap()
            .read_to_end(&mut out)
            .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn test_decompress_inflates_gzip() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"layer bytes").unwrap();
        let compressed = encoder.finish().unwrap();

        let mut out = Vec::new();
        decompress(Box::new(io::Cursor::new(compressed)))
            .unwrap()
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, b"layer bytes");
    }

    #[test]
    fn test_decompress_reads_every_zstd_frame_and_skips_skippable_ones() {
        // Two real frames with a skippable frame between them, as zstd:chunked layers are written.
        let mut stream = ruzstd::encoding::compress_to_vec(
            b"first frame ".as_slice(),
            ruzstd::encoding::CompressionLevel::Uncompressed,
        );
        stream.extend_from_slice(&0x184D2A50u32.to_le_bytes());
        stream.extend_from_slice(&4u32.to_le_bytes());
        stream.extend_from_slice(b"meta");
        stream.extend(ruzstd::encoding::compress_to_vec(
            b"second frame".as_slice(),
            ruzstd::encoding::CompressionLevel::Uncompressed,
        ));

        let mut out = Vec::new();
        decompress(Box::new(io::Cursor::new(stream)))
            .unwrap()
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, b"first frame second frame");
    }

    #[test]
    fn test_indexes_a_tar_and_reads_entries_by_seeking() {
        let temp = write_temp(&tar_with(
            &[("index.json", b"{}"), ("blobs/sha256/abc", b"layer")],
            Some("blobs/sha256/def"),
        ));
        let store = Store::open(temp.path()).unwrap();

        assert!(store.has("index.json"));
        assert!(store.has("blobs/sha256/abc"));
        assert!(
            store.has("blobs/sha256/def"),
            "hard link resolves to its target"
        );
        assert!(!store.has("manifest.json"));
        assert_eq!(store.read("blobs/sha256/abc").unwrap(), b"layer");
        assert_eq!(store.read("blobs/sha256/def").unwrap(), b"layer");
        assert_eq!(store.read("index.json").unwrap(), b"{}");
    }

    #[test]
    fn test_opens_a_gzipped_tar() {
        let plain = tar_with(&[("./manifest.json", b"[]")], None);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&plain).unwrap();
        let temp = write_temp(&encoder.finish().unwrap());

        let store = Store::open(temp.path()).unwrap();
        assert!(
            store.has("manifest.json"),
            "leading ./ is dropped from names"
        );
        assert_eq!(store.read("manifest.json").unwrap(), b"[]");
    }

    #[test]
    fn test_opens_a_directory_as_is() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("oci-layout"), "{}").unwrap();
        let store = Store::open(temp.path()).unwrap();
        assert!(store.has("oci-layout"));
        assert!(!store.has("index.json"));
        assert_eq!(store.read("oci-layout").unwrap(), b"{}");
        assert!(store.read("missing").is_err());
    }

    #[test]
    fn test_a_missing_path_is_an_error_that_names_it() {
        let Err(error) = Store::open(Path::new("/nonexistent/app.tar")) else {
            panic!("a missing path should not open");
        };
        assert!(error.to_string().contains("/nonexistent/app.tar"));
    }

    #[test]
    fn test_something_that_is_not_a_tar_is_an_error() {
        let temp = write_temp(b"this is not a tar archive at all, just some text");
        assert!(Store::open(temp.path()).is_err());
    }

    #[test]
    fn test_entry_names_are_normalised_and_climbing_ones_rejected() {
        assert_eq!(entry_name(Path::new("./a/b")).as_deref(), Some("a/b"));
        assert_eq!(entry_name(Path::new("a/./b/")).as_deref(), Some("a/b"));
        assert_eq!(entry_name(Path::new("./")), None);
        assert_eq!(entry_name(Path::new("../a")), None);
        assert_eq!(entry_name(Path::new("/a")), None);
    }
}
