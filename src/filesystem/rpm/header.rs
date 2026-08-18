//! Reading an rpm header blob.
//!
//! What the database stores is the imported header, not the on-disk `.rpm` one: there is no lead
//! and no magic, just
//!
//! ```text
//! [nindex: u32be][hsize: u32be][index: nindex * 16][data: hsize]
//! ```
//!
//! where each index entry is `[tag][type][offset][count]` and the offset points into the data
//! store that follows. Tags are a flat namespace of integers, so reading a header means looking up
//! the handful this tool cares about and ignoring the rest of what a package records about itself.

use std::path::PathBuf;

use super::super::artifacts::is_artifact_metadata;

const TAG_NAME: u32 = 1000;
const TAG_VERSION: u32 = 1001;
const TAG_RELEASE: u32 = 1002;
const TAG_EPOCH: u32 = 1003;
const TAG_LICENSE: u32 = 1014;
const TAG_DIRINDEXES: u32 = 1116;
const TAG_BASENAMES: u32 = 1117;
const TAG_DIRNAMES: u32 = 1118;

const TYPE_INT32: u32 = 4;
const TYPE_STRING: u32 = 6;
const TYPE_STRING_ARRAY: u32 = 8;
const TYPE_I18NSTRING: u32 = 9;

/// A header's index entry: what a tag is, and where its value lives in the data store.
struct Entry {
    kind: u32,
    offset: usize,
    count: usize,
}

/// What one installed package's header says about it.
pub struct Header {
    pub name: String,
    /// `version-release`, with an `epoch:` prefix when the package carries a non-zero one, which is
    /// how rpm itself writes a version that has one.
    pub version: String,
    pub license: Option<String>,
    /// The installed artifact metadata this package owns, relative to the scan root.
    ///
    /// Filtered to what the artifact catalogers key on as the header is read, so a package that
    /// installs thousands of files costs a handful of paths rather than all of them.
    pub owned: Vec<PathBuf>,
}

/// Parse a header blob, or `None` when it is too malformed to name a package.
///
/// A single unreadable header should cost that one package, not the scan, so this returns an option
/// rather than an error.
pub fn parse(blob: &[u8]) -> Option<Header> {
    let nindex = read_u32(blob, 0)? as usize;
    let hsize = read_u32(blob, 4)? as usize;

    let index_start = 8;
    let data_start = index_start + nindex * 16;
    let data = blob.get(data_start..data_start.checked_add(hsize)?)?;

    let mut entries = std::collections::HashMap::new();
    for position in 0..nindex {
        let at = index_start + position * 16;
        let tag = read_u32(blob, at)?;
        let kind = read_u32(blob, at + 4)?;
        // Signed in the format, but a negative offset would point outside the data store.
        let offset = read_u32(blob, at + 8)? as usize;
        let count = read_u32(blob, at + 12)? as usize;
        entries.insert(
            tag,
            Entry {
                kind,
                offset,
                count,
            },
        );
    }

    let name = string(data, entries.get(&TAG_NAME))?;
    let version = string(data, entries.get(&TAG_VERSION)).unwrap_or_default();
    let release = string(data, entries.get(&TAG_RELEASE)).unwrap_or_default();

    Some(Header {
        name,
        version: full_version(&version, &release, epoch(data, entries.get(&TAG_EPOCH))),
        license: string(data, entries.get(&TAG_LICENSE)),
        owned: owned_paths(data, &entries),
    })
}

/// Assemble the version rpm would print.
fn full_version(version: &str, release: &str, epoch: Option<i32>) -> String {
    let base = if release.is_empty() {
        version.to_string()
    } else {
        format!("{version}-{release}")
    };
    // Epoch 0 is the default and rpm leaves it off.
    match epoch.filter(|epoch| *epoch != 0) {
        Some(epoch) => format!("{epoch}:{base}"),
        None => base,
    }
}

/// The installed artifact metadata files this package claims.
///
/// rpm splits paths into a directory table and a per-file index into it, so a path is
/// `DIRNAMES[DIRINDEXES[i]] + BASENAMES[i]`. Directories are absolute and end in a slash; the
/// leading slash comes off because the ownership set is keyed relative to the scan root.
fn owned_paths(data: &[u8], entries: &std::collections::HashMap<u32, Entry>) -> Vec<PathBuf> {
    let basenames = string_array(data, entries.get(&TAG_BASENAMES));
    let dirnames = string_array(data, entries.get(&TAG_DIRNAMES));
    let dirindexes = int32_array(data, entries.get(&TAG_DIRINDEXES));

    basenames
        .iter()
        .zip(dirindexes.iter())
        .filter_map(|(basename, index)| {
            let directory = dirnames.get(usize::try_from(*index).ok()?)?;
            let path = PathBuf::from(format!("{directory}{basename}").trim_start_matches('/'));
            is_artifact_metadata(&path).then_some(path)
        })
        .collect()
}

/// A null terminated string from the data store.
fn string(data: &[u8], entry: Option<&Entry>) -> Option<String> {
    let entry = entry?;
    if entry.kind != TYPE_STRING && entry.kind != TYPE_I18NSTRING {
        return None;
    }
    read_string(data, entry.offset).map(str::to_string)
}

/// A run of `count` null terminated strings.
fn string_array(data: &[u8], entry: Option<&Entry>) -> Vec<String> {
    let Some(entry) = entry.filter(|entry| entry.kind == TYPE_STRING_ARRAY) else {
        return Vec::new();
    };

    let mut values = Vec::with_capacity(entry.count.min(1024));
    let mut offset = entry.offset;
    for _ in 0..entry.count {
        let Some(value) = read_string(data, offset) else {
            break;
        };
        offset += value.len() + 1;
        values.push(value.to_string());
    }
    values
}

fn int32_array(data: &[u8], entry: Option<&Entry>) -> Vec<i32> {
    let Some(entry) = entry.filter(|entry| entry.kind == TYPE_INT32) else {
        return Vec::new();
    };
    (0..entry.count)
        .map(|index| read_u32(data, entry.offset + index * 4).map(|value| value as i32))
        .take_while(Option::is_some)
        .flatten()
        .collect()
}

/// The single int32 an epoch is stored as.
fn epoch(data: &[u8], entry: Option<&Entry>) -> Option<i32> {
    int32_array(data, entry).first().copied()
}

fn read_string(data: &[u8], offset: usize) -> Option<&str> {
    let rest = data.get(offset..)?;
    let end = rest.iter().position(|byte| *byte == 0)?;
    std::str::from_utf8(&rest[..end]).ok()
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a header blob the way rpm lays one out, so the tests exercise the real offsets rather
    /// than a convenient stand-in.
    struct Builder {
        index: Vec<u8>,
        data: Vec<u8>,
        count: u32,
    }

    impl Builder {
        fn new() -> Self {
            Self {
                index: Vec::new(),
                data: Vec::new(),
                count: 0,
            }
        }

        fn entry(&mut self, tag: u32, kind: u32, count: usize) -> &mut Self {
            self.index.extend_from_slice(&tag.to_be_bytes());
            self.index.extend_from_slice(&kind.to_be_bytes());
            self.index
                .extend_from_slice(&(self.data.len() as u32).to_be_bytes());
            self.index.extend_from_slice(&(count as u32).to_be_bytes());
            self.count += 1;
            self
        }

        fn string(&mut self, tag: u32, value: &str) -> &mut Self {
            self.entry(tag, TYPE_STRING, 1);
            self.data.extend_from_slice(value.as_bytes());
            self.data.push(0);
            self
        }

        fn strings(&mut self, tag: u32, values: &[&str]) -> &mut Self {
            self.entry(tag, TYPE_STRING_ARRAY, values.len());
            for value in values {
                self.data.extend_from_slice(value.as_bytes());
                self.data.push(0);
            }
            self
        }

        fn ints(&mut self, tag: u32, values: &[i32]) -> &mut Self {
            self.entry(tag, TYPE_INT32, values.len());
            for value in values {
                self.data.extend_from_slice(&value.to_be_bytes());
            }
            self
        }

        fn build(&self) -> Vec<u8> {
            let mut blob = Vec::new();
            blob.extend_from_slice(&self.count.to_be_bytes());
            blob.extend_from_slice(&(self.data.len() as u32).to_be_bytes());
            blob.extend_from_slice(&self.index);
            blob.extend_from_slice(&self.data);
            blob
        }
    }

    #[test]
    fn test_reads_name_version_and_license() {
        let blob = Builder::new()
            .string(TAG_NAME, "bash")
            .string(TAG_VERSION, "5.2.32")
            .string(TAG_RELEASE, "1.fc41")
            .string(TAG_LICENSE, "GPL-3.0-or-later")
            .build();

        let header = parse(&blob).expect("header should parse");
        assert_eq!(header.name, "bash");
        assert_eq!(header.version, "5.2.32-1.fc41");
        assert_eq!(header.license.as_deref(), Some("GPL-3.0-or-later"));
    }

    #[test]
    fn test_a_non_zero_epoch_prefixes_the_version() {
        let blob = Builder::new()
            .string(TAG_NAME, "vim")
            .string(TAG_VERSION, "9.1")
            .string(TAG_RELEASE, "3.fc41")
            .ints(TAG_EPOCH, &[2])
            .build();
        assert_eq!(parse(&blob).unwrap().version, "2:9.1-3.fc41");
    }

    #[test]
    fn test_epoch_zero_is_left_off() {
        // rpm treats a missing epoch and an epoch of 0 as the same thing.
        let blob = Builder::new()
            .string(TAG_NAME, "vim")
            .string(TAG_VERSION, "9.1")
            .string(TAG_RELEASE, "3.fc41")
            .ints(TAG_EPOCH, &[0])
            .build();
        assert_eq!(parse(&blob).unwrap().version, "9.1-3.fc41");
    }

    #[test]
    fn test_a_header_without_a_name_is_not_a_package() {
        let blob = Builder::new().string(TAG_VERSION, "1.0").build();
        assert!(parse(&blob).is_none());
    }

    #[test]
    fn test_a_missing_license_stays_unset() {
        let blob = Builder::new()
            .string(TAG_NAME, "mystery")
            .string(TAG_VERSION, "1.0")
            .build();
        assert_eq!(parse(&blob).unwrap().license, None);
    }

    #[test]
    fn test_file_paths_reassemble_from_the_directory_table() {
        // Only the dist-info metadata is kept; the other two files are not artifact metadata.
        let blob = Builder::new()
            .string(TAG_NAME, "python3-pyyaml")
            .string(TAG_VERSION, "6.0")
            .strings(
                TAG_DIRNAMES,
                &[
                    "/usr/lib64/python3.13/site-packages/PyYAML-6.0.dist-info/",
                    "/usr/bin/",
                ],
            )
            .strings(TAG_BASENAMES, &["METADATA", "RECORD", "python3"])
            .ints(TAG_DIRINDEXES, &[0, 0, 1])
            .build();

        assert_eq!(
            parse(&blob).unwrap().owned,
            vec![PathBuf::from(
                "usr/lib64/python3.13/site-packages/PyYAML-6.0.dist-info/METADATA"
            )]
        );
    }

    #[test]
    fn test_a_package_with_no_files_owns_nothing() {
        let blob = Builder::new()
            .string(TAG_NAME, "basesystem")
            .string(TAG_VERSION, "11")
            .build();
        assert!(parse(&blob).unwrap().owned.is_empty());
    }

    #[test]
    fn test_a_directory_index_out_of_range_is_skipped() {
        // Corrupt rather than merely unusual, and it must not panic.
        let blob = Builder::new()
            .string(TAG_NAME, "broken")
            .string(TAG_VERSION, "1.0")
            .strings(TAG_DIRNAMES, &["/usr/lib/"])
            .strings(TAG_BASENAMES, &["METADATA"])
            .ints(TAG_DIRINDEXES, &[7])
            .build();
        assert!(parse(&blob).unwrap().owned.is_empty());
    }

    #[test]
    fn test_truncated_blobs_do_not_panic() {
        let blob = Builder::new()
            .string(TAG_NAME, "bash")
            .string(TAG_VERSION, "5.2")
            .build();
        for length in 0..blob.len() {
            let _ = parse(&blob[..length]);
        }
    }
}
