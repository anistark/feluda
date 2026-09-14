//! Cataloging the Go modules a compiled binary was built from.
//!
//! A distroless Go image has no package database and no metadata files, so every other cataloger
//! finds nothing in it. The binary itself is the record: the Go linker writes a build info blob
//! into every executable it produces, naming the main module and each dependency at the exact
//! version compiled in. `go version -m` reads it back, and so does this.
//!
//! This is the one cataloger that reads a container format rather than a file, so it is also the
//! one whose recognition is in two steps. By path alone it can only say that a file *could* be an
//! executable, which is what the ownership filter needs; the first bytes then say whether it is one,
//! and only then is it opened for real.
//!
//! Build info carries no license, so every module here leaves with `license: None` and goes to
//! pkg.go.dev through the same registry pass the other artifacts use.

use std::path::Path;

use crate::debug::{log, LogLevel};
use crate::purl::Ecosystem;

use super::exe::{self, Executable};
use super::Artifact;

/// What the linker writes ahead of the build info header.
const MAGIC: &[u8] = b"\xff Go buildinf:";

/// The header is aligned to this within the data segment, so a match anywhere else is a string
/// that happens to contain the magic rather than the header.
const ALIGN: u64 = 16;

/// Magic, pointer size, flags, and either two pointers or padding.
const HEADER_SIZE: u64 = 32;

/// Flag bit: the strings follow the header inline rather than through pointers (Go 1.18+).
const FLAG_INLINE_STRINGS: u8 = 0x2;

/// Flag bit: pointers in the header are big-endian.
const FLAG_BIG_ENDIAN: u8 = 0x1;

/// The module info is wrapped in these so the runtime can find it by content.
const MODINFO_START: &[u8] = b"\x30\x77\xaf\x0c\x92\x74\x08\x02\x41\xe1\xc1\x07\xe6\xd6\x18\xe6";
const MODINFO_END: &[u8] = b"\xf9\x32\x43\x31\x86\x18\x20\x72\x00\x82\x42\x10\x41\x16\xd8\xf2";

/// How much of the data region is read at a time while looking for the header.
///
/// The linker puts the blob at the front of the data segment, so the first chunk is almost always
/// the only one; the rest of the region is only read for binaries that turn out not to be Go.
const CHUNK: u64 = 64 * 1024;

/// The most a build info string is allowed to claim to be.
///
/// A module list for a large program runs to tens of kilobytes; anything past this is a corrupt
/// length rather than a program.
const MAX_STRING: u64 = 16 * 1024 * 1024;

/// Whether `path` could be an executable, judged by the name alone.
///
/// Go binaries have no extension on Unix and `.exe` on Windows, and nothing else about their path
/// is predictable: `/app`, `/usr/local/bin/app` and `/ko-app/app` are all common. So the test is
/// deliberately loose, and [`is_executable`] narrows it once there is a file to open. Being loose
/// here is what lets the ownership filter keep the paths of distro-shipped Go binaries, which is
/// what stops Debian's `docker` from being cataloged twice.
pub fn is_metadata(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    match name.rsplit_once('.') {
        None => true,
        Some((stem, extension)) => !stem.is_empty() && extension.eq_ignore_ascii_case("exe"),
    }
}

/// Whether the file at `path` is an executable in a format the build info can be read from.
pub fn is_executable(path: &Path) -> bool {
    exe::is_executable(path)
}

/// Every module compiled into the executable at `path`, or nothing when it carries no build info.
pub fn read(path: &Path) -> Vec<Artifact> {
    let Some(mut executable) = Executable::open(path) else {
        log(
            LogLevel::Info,
            &format!("Skipping {}: not a readable executable", path.display()),
        );
        return Vec::new();
    };
    let Some(modinfo) = read_modinfo(&mut executable) else {
        log(
            LogLevel::Info,
            &format!("Skipping {}: no Go build info", path.display()),
        );
        return Vec::new();
    };

    let modules = parse_modinfo(&modinfo);
    log(
        LogLevel::Info,
        &format!(
            "Read {} Go modules from build info in {}",
            modules.len(),
            path.display()
        ),
    );

    modules
        .into_iter()
        .map(|module| Artifact {
            ecosystem: Ecosystem::Golang,
            name: module.path,
            version: module.version,
            license: None,
        })
        .collect()
}

/// A dependency as build info records it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Module {
    path: String,
    /// Empty when the build info says `(devel)`, which is a module built from a working tree
    /// rather than any release.
    version: String,
}

/// Find the build info header and read the module info string it points at.
fn read_modinfo(executable: &mut Executable) -> Option<String> {
    let (data_addr, data_size) = executable.data_start()?;
    let header_addr = find_header(executable, data_addr, data_size)?;
    let header = executable.read_data(header_addr, HEADER_SIZE)?;
    if header.len() < HEADER_SIZE as usize {
        return None;
    }

    let pointer_size = u64::from(header[14]);
    let flags = header[15];

    // Two strings, version then module info. Only the second is wanted here, but the first has
    // to be walked past.
    let modinfo = if flags & FLAG_INLINE_STRINGS != 0 {
        let (_, next) = read_inline_string(executable, header_addr + HEADER_SIZE)?;
        let (modinfo, _) = read_inline_string(executable, next)?;
        modinfo
    } else {
        if pointer_size != 4 && pointer_size != 8 {
            return None;
        }
        let big_endian = flags & FLAG_BIG_ENDIAN != 0;
        let pointers = 16 + pointer_size as usize;
        let modinfo_ptr = read_word(
            &header[pointers..pointers + pointer_size as usize],
            big_endian,
        )?;
        read_pointed_string(executable, modinfo_ptr, pointer_size, big_endian)?
    };

    // The sentinels are not UTF-8, so the string only becomes text once they are off.
    unwrap_modinfo(&modinfo).map(|inner| String::from_utf8_lossy(inner).into_owned())
}

/// The address of the build info header within the data region, if there is one.
///
/// The region is read a chunk at a time, each overlapping the last by a header's width so a header
/// straddling a boundary is still seen whole. Only aligned matches count.
fn find_header(executable: &mut Executable, start: u64, size: u64) -> Option<u64> {
    let end = start.checked_add(size)?;
    let mut position = start;
    while position < end {
        let want = CHUNK.min(end - position);
        let chunk = executable.read_data(position, want)?;
        if chunk.len() < MAGIC.len() {
            return None;
        }

        let mut offset = 0usize;
        while offset + HEADER_SIZE as usize <= chunk.len() {
            if chunk[offset..].starts_with(MAGIC) {
                return Some(position + offset as u64);
            }
            offset += ALIGN as usize;
        }

        // The read may have come up short of the chunk, at the end of what the file backs.
        if (chunk.len() as u64) < want || chunk.len() as u64 <= HEADER_SIZE {
            return None;
        }
        position += chunk.len() as u64 - HEADER_SIZE;
    }
    None
}

/// A varint-prefixed string at `addr`, and the address just past it.
fn read_inline_string(executable: &mut Executable, addr: u64) -> Option<(Vec<u8>, u64)> {
    let prefix = executable.read_data(addr, 10)?;
    let (length, consumed) = decode_uvarint(&prefix)?;
    if length > MAX_STRING {
        return None;
    }
    let start = addr + consumed as u64;
    let bytes = executable.read_data(start, length)?;
    if bytes.len() as u64 != length {
        return None;
    }
    Some((bytes, start + length))
}

/// A Go string whose header (pointer, length) sits at `addr`.
fn read_pointed_string(
    executable: &mut Executable,
    addr: u64,
    pointer_size: u64,
    big_endian: bool,
) -> Option<Vec<u8>> {
    let header = executable.read_data(addr, 2 * pointer_size)?;
    if header.len() as u64 != 2 * pointer_size {
        return None;
    }
    let data_addr = read_word(&header[..pointer_size as usize], big_endian)?;
    let length = read_word(&header[pointer_size as usize..], big_endian)?;
    if length > MAX_STRING {
        return None;
    }
    let bytes = executable.read_data(data_addr, length)?;
    if bytes.len() as u64 != length {
        return None;
    }
    Some(bytes)
}

/// A 4- or 8-byte word, in the byte order the header flags name.
fn read_word(bytes: &[u8], big_endian: bool) -> Option<u64> {
    match bytes.len() {
        4 => {
            let bytes: [u8; 4] = bytes.try_into().ok()?;
            Some(u64::from(if big_endian {
                u32::from_be_bytes(bytes)
            } else {
                u32::from_le_bytes(bytes)
            }))
        }
        8 => {
            let bytes: [u8; 8] = bytes.try_into().ok()?;
            Some(if big_endian {
                u64::from_be_bytes(bytes)
            } else {
                u64::from_le_bytes(bytes)
            })
        }
        _ => None,
    }
}

/// Decode an unsigned LEB128 varint, returning the value and the bytes it occupied.
fn decode_uvarint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    for (index, &byte) in bytes.iter().enumerate() {
        if shift >= 64 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
        shift += 7;
    }
    None
}

/// Strip the sentinels around the module info, as the runtime does.
///
/// The runtime's own test is exactly this: a newline seventeen bytes from the end, with sixteen
/// bytes of marker on each side. A string that fails it is not module info at all.
fn unwrap_modinfo(modinfo: &[u8]) -> Option<&[u8]> {
    if modinfo.len() < 33 || modinfo[modinfo.len() - 17] != b'\n' {
        return None;
    }
    if !modinfo.starts_with(MODINFO_START) || !modinfo.ends_with(MODINFO_END) {
        return None;
    }
    modinfo.get(16..modinfo.len() - 16)
}

/// The dependencies out of the module info text.
///
/// The format is one tab-separated record per line: `dep` names a dependency and the `=>` line
/// that may follow it names what replaced it. A replacement by another module is what was actually
/// compiled in, so it takes over. A replacement by a directory is a local fork of the original,
/// which has no coordinates of its own, so the original's stay. The main module is not a
/// dependency and is left out.
fn parse_modinfo(modinfo: &str) -> Vec<Module> {
    let mut modules: Vec<Module> = Vec::new();
    for line in modinfo.lines() {
        let mut fields = line.split('\t');
        let (Some(kind), Some(path), Some(version)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        match kind {
            "dep" => modules.push(Module::new(path, version)),
            "=>" if !is_directory_replacement(path) => {
                if let Some(last) = modules.last_mut() {
                    *last = Module::new(path, version);
                }
            }
            _ => {}
        }
    }
    modules
}

impl Module {
    fn new(path: &str, version: &str) -> Module {
        Module {
            path: path.to_string(),
            version: if version == "(devel)" {
                String::new()
            } else {
                version.to_string()
            },
        }
    }
}

/// Whether a replacement names a directory rather than a module.
///
/// go.mod requires a directory replacement to start with `./`, `../` or be rooted, which is what
/// keeps it distinguishable from a module path.
fn is_directory_replacement(path: &str) -> bool {
    path.starts_with('.')
        || path.starts_with('/')
        || path.starts_with('\\')
        || path
            .as_bytes()
            .get(1..3)
            .is_some_and(|drive| drive == b":\\" || drive == b":/")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::filesystem::artifacts::exe::tests::{elf, macho, pe, Synth};

    /// Module info text for a small program, with the sentinels the linker adds.
    pub fn modinfo(body: &str) -> Vec<u8> {
        let mut out = MODINFO_START.to_vec();
        out.extend_from_slice(body.as_bytes());
        out.extend_from_slice(MODINFO_END);
        out
    }

    pub const SAMPLE_MODINFO: &str = "path\texample.com/app/cmd/app\n\
        mod\texample.com/app\t(devel)\t\n\
        dep\tgithub.com/spf13/cobra\tv1.8.1\th1:e5/vxKd/rZsfSJMUX1agtjeTDf+qv1/JdBF8gg5k9ZM=\n\
        dep\tgolang.org/x/sys\tv0.22.0\th1:RI27ohtqKCnwULzJLqkv6AxmqOrn9Q5jt+jfOKZfp6Q=\n\
        build\t-buildmode=exe\n\
        build\tGOOS=linux\n";

    fn uvarint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    /// The Go 1.18+ blob: header, then the two strings inline.
    pub fn inline_blob(version: &str, modinfo: &[u8]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(8);
        out.push(FLAG_INLINE_STRINGS);
        out.resize(HEADER_SIZE as usize, 0);
        out.extend(uvarint(version.len() as u64));
        out.extend_from_slice(version.as_bytes());
        out.extend(uvarint(modinfo.len() as u64));
        out.extend_from_slice(modinfo);
        out
    }

    /// The pre-1.18 blob: header with two pointers to Go string headers, laid out after it.
    ///
    /// `base` is the virtual address the blob will be loaded at.
    pub fn pointer_blob(base: u64, version: &str, modinfo: &[u8], big_endian: bool) -> Vec<u8> {
        let word = |value: u64| -> [u8; 8] {
            if big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            }
        };
        // Layout: header (32) | version string header (16) | modinfo string header (16) | bytes.
        let version_hdr = base + HEADER_SIZE;
        let modinfo_hdr = version_hdr + 16;
        let version_data = modinfo_hdr + 16;
        let modinfo_data = version_data + version.len() as u64;

        let mut out = MAGIC.to_vec();
        out.push(8);
        out.push(if big_endian { FLAG_BIG_ENDIAN } else { 0 });
        out.extend_from_slice(&word(version_hdr));
        out.extend_from_slice(&word(modinfo_hdr));
        out.extend_from_slice(&word(version_data));
        out.extend_from_slice(&word(version.len() as u64));
        out.extend_from_slice(&word(modinfo_data));
        out.extend_from_slice(&word(modinfo.len() as u64));
        out.extend_from_slice(version.as_bytes());
        out.extend_from_slice(modinfo);
        out
    }

    fn write_temp(bytes: &[u8]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), bytes).unwrap();
        file
    }

    fn names(artifacts: &[Artifact]) -> Vec<(String, String)> {
        artifacts
            .iter()
            .map(|artifact| (artifact.name.clone(), artifact.version.clone()))
            .collect()
    }

    fn expected() -> Vec<(String, String)> {
        vec![
            ("github.com/spf13/cobra".into(), "v1.8.1".into()),
            ("golang.org/x/sys".into(), "v0.22.0".into()),
        ]
    }

    #[test]
    fn test_recognises_executables_by_name_shape() {
        assert!(is_metadata(Path::new("app")));
        assert!(is_metadata(Path::new("usr/local/bin/kubectl")));
        assert!(is_metadata(Path::new("app.exe")));
        assert!(is_metadata(Path::new("App.EXE")));
        assert!(!is_metadata(Path::new("usr/lib/libc.so.6")));
        assert!(!is_metadata(Path::new("srv/app/package.json")));
        assert!(!is_metadata(Path::new("home/user/.bashrc")));
        assert!(!is_metadata(Path::new(".exe")));
    }

    #[test]
    fn test_reads_inline_strings_from_an_elf() {
        let synth = Synth {
            data_addr: 0x500000,
            data: inline_blob("go1.22.5", &modinfo(SAMPLE_MODINFO)),
        };
        let file = write_temp(&elf(&synth, true));
        let artifacts = read(file.path());
        assert_eq!(names(&artifacts), expected());
        assert!(artifacts
            .iter()
            .all(|artifact| artifact.ecosystem == Ecosystem::Golang && artifact.license.is_none()));
    }

    #[test]
    fn test_finds_the_header_past_other_data_in_a_stripped_elf() {
        // No section headers, and the blob is not the first thing in the segment: the search has
        // to walk the writable segment at 16-byte alignment until it finds it.
        let mut data = b"leading data".to_vec();
        data.resize(0x230, 0);
        data.extend(inline_blob("go1.22.5", &modinfo(SAMPLE_MODINFO)));
        let synth = Synth {
            data_addr: 0x500000,
            data,
        };
        let file = write_temp(&elf(&synth, false));
        assert_eq!(names(&read(file.path())), expected());
    }

    #[test]
    fn test_an_unaligned_magic_is_not_a_header() {
        // The magic appears as data, four bytes into the segment, and the real header never comes.
        let mut data = b"\0\0\0\0".to_vec();
        data.extend_from_slice(MAGIC);
        data.resize(0x100, 0);
        let synth = Synth {
            data_addr: 0x500000,
            data,
        };
        let file = write_temp(&elf(&synth, false));
        assert!(read(file.path()).is_empty());
    }

    #[test]
    fn test_reads_pointed_strings_from_an_older_binary() {
        for big_endian in [false, true] {
            let base = 0x500000;
            let synth = Synth {
                data_addr: base,
                data: pointer_blob(base, "go1.17.13", &modinfo(SAMPLE_MODINFO), big_endian),
            };
            let file = write_temp(&elf(&synth, true));
            assert_eq!(
                names(&read(file.path())),
                expected(),
                "big_endian = {big_endian}"
            );
        }
    }

    #[test]
    fn test_reads_a_macho_and_a_pe() {
        let blob = inline_blob("go1.22.5", &modinfo(SAMPLE_MODINFO));
        let synth = Synth {
            data_addr: 0x1_0000_4000,
            data: blob.clone(),
        };
        for with_section in [true, false] {
            let file = write_temp(&macho(&synth, with_section));
            assert_eq!(names(&read(file.path())), expected());
        }

        let synth = Synth {
            data_addr: 0x1_4000_2000,
            data: blob,
        };
        let file = write_temp(&pe(&synth));
        assert_eq!(names(&read(file.path())), expected());
    }

    #[test]
    fn test_a_binary_without_build_info_yields_nothing() {
        let synth = Synth {
            data_addr: 0x500000,
            data: b"just some initialised data, nothing from a Go linker".to_vec(),
        };
        let file = write_temp(&elf(&synth, false));
        assert!(read(file.path()).is_empty());

        // A script with an executable name is not even opened as one.
        let file = write_temp(b"#!/bin/sh\nexec java -jar app.jar\n");
        assert!(read(file.path()).is_empty());
    }

    #[test]
    fn test_module_info_without_its_sentinels_is_not_module_info() {
        let synth = Synth {
            data_addr: 0x500000,
            data: inline_blob("go1.22.5", SAMPLE_MODINFO.as_bytes()),
        };
        let file = write_temp(&elf(&synth, true));
        assert!(read(file.path()).is_empty());
    }

    #[test]
    fn test_a_module_replacement_takes_over_and_a_directory_one_does_not() {
        let modinfo = "path\texample.com/app\n\
            mod\texample.com/app\t(devel)\t\n\
            dep\tgithub.com/upstream/lib\tv1.0.0\th1:aaa=\n\
            =>\tgithub.com/fork/lib\tv1.0.1\th1:bbb=\n\
            dep\tgithub.com/local/lib\tv2.3.0\th1:ccc=\n\
            =>\t../lib\t(devel)\t\n\
            dep\tgithub.com/work/lib\t(devel)\t\n\
            =>\t/home/me/lib\t(devel)\t\n";
        let modules = parse_modinfo(modinfo);
        assert_eq!(
            modules,
            vec![
                Module {
                    path: "github.com/fork/lib".into(),
                    version: "v1.0.1".into()
                },
                Module {
                    path: "github.com/local/lib".into(),
                    version: "v2.3.0".into()
                },
                Module {
                    path: "github.com/work/lib".into(),
                    version: String::new()
                },
            ]
        );
    }

    #[test]
    fn test_directory_replacements_are_told_from_modules() {
        assert!(is_directory_replacement("./internal/lib"));
        assert!(is_directory_replacement("../lib"));
        assert!(is_directory_replacement("/srv/lib"));
        assert!(is_directory_replacement("C:\\src\\lib"));
        assert!(!is_directory_replacement("github.com/fork/lib"));
        assert!(!is_directory_replacement("example.com/lib/v2"));
    }

    /// The integration tests drive the real binary against `tests/fixtures/go/app`, which is
    /// this very ELF written to disk: a real `go build` output is a couple of megabytes, which is
    /// too much to commit for a fixture. Regenerate it with `FELUDA_WRITE_FIXTURES=1 cargo test`
    /// after changing the builder or the sample module info.
    #[test]
    fn test_the_checked_in_fixture_is_what_the_builder_makes() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/go/app");
        let expected = elf(
            &Synth {
                data_addr: 0x500000,
                data: inline_blob("go1.22.5", &modinfo(SAMPLE_MODINFO)),
            },
            true,
        );
        if std::env::var_os("FELUDA_WRITE_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &expected).unwrap();
        }
        let actual = std::fs::read(&path).expect("tests/fixtures/go/app is missing");
        assert!(
            actual == expected,
            "tests/fixtures/go/app is stale; regenerate with FELUDA_WRITE_FIXTURES=1 cargo test"
        );
    }

    #[test]
    fn test_uvarint_decoding() {
        assert_eq!(decode_uvarint(&[0x05]), Some((5, 1)));
        assert_eq!(decode_uvarint(&[0x80, 0x01]), Some((128, 2)));
        assert_eq!(decode_uvarint(&[0xe5, 0x8e, 0x26]), Some((624_485, 3)));
        assert_eq!(decode_uvarint(&[0x80]), None);
        assert_eq!(decode_uvarint(&[]), None);
    }
}
