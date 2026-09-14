//! Just enough of ELF, Mach-O and PE to find a blob in an executable's data.
//!
//! Go's own `debug/buildinfo` asks two things of a container format: where the data segment starts,
//! and what bytes sit at a given virtual address. That is all the build info reader needs too, so
//! that is all this module parses. Nothing here reads symbols, relocations or debug info, and no
//! executable is ever read in full: headers are a few kilobytes, and the data region is read in
//! chunks by whoever searches it.
//!
//! Every format comes down to the same shape once opened: a list of loadable segments, each a
//! virtual address range backed by a file offset, plus the one region worth searching. The three
//! parsers only differ in where they find those.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// An opened executable: its loadable segments and the region a blob would be in.
pub struct Executable {
    file: File,
    /// Every loadable segment, for mapping a virtual address to file bytes.
    segments: Vec<Segment>,
    /// Where a data blob would be, as a virtual address and size.
    data: Option<(u64, u64)>,
}

/// What a format parser yields: the loadable segments and the data region, if it named one.
type Layout = (Vec<Segment>, Option<(u64, u64)>);

/// A range of virtual addresses backed by bytes in the file.
///
/// `size` is the number of bytes present in the file, not the size in memory: the tail of a
/// writable segment is zero-filled `.bss` that exists nowhere on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Segment {
    addr: u64,
    offset: u64,
    size: u64,
}

/// Whether the first bytes of a file are an executable format this module can open.
///
/// Four bytes decide it, which is what makes asking of every extensionless file in a tree cheap.
pub fn is_executable(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok()
        && format_of(&magic).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Elf,
    MachO,
    Pe,
}

fn format_of(magic: &[u8; 4]) -> Option<Format> {
    match magic {
        [0x7f, b'E', b'L', b'F'] => Some(Format::Elf),
        // 32- and 64-bit, in either byte order. Fat binaries (`0xcafebabe`) are deliberately not
        // here: Go's reader does not open them either, and a Linux root filesystem never holds one.
        [0xfe, 0xed, 0xfa, 0xce | 0xcf] | [0xce | 0xcf, 0xfa, 0xed, 0xfe] => Some(Format::MachO),
        [b'M', b'Z', _, _] => Some(Format::Pe),
        _ => None,
    }
}

/// Cap on the tables read from a header, so a corrupt count cannot ask for gigabytes.
const MAX_TABLE_BYTES: usize = 16 * 1024 * 1024;

impl Executable {
    /// Open an executable, or `None` when the file is not one this module reads.
    pub fn open(path: &Path) -> Option<Executable> {
        let mut file = File::open(path).ok()?;
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic).ok()?;

        let (segments, data) = match format_of(&magic)? {
            Format::Elf => parse_elf(&mut file)?,
            Format::MachO => parse_macho(&mut file, &magic)?,
            Format::Pe => parse_pe(&mut file)?,
        };

        Some(Executable {
            file,
            segments,
            data,
        })
    }

    /// The region a data blob would be in, as a virtual address and size.
    pub fn data_start(&self) -> Option<(u64, u64)> {
        self.data
    }

    /// Up to `size` bytes at a virtual address.
    ///
    /// The read stops at the end of the segment holding `addr`, as Go's does: a segment's bytes on
    /// disk are all there is, and the next segment is not a continuation of them.
    pub fn read_data(&mut self, addr: u64, size: u64) -> Option<Vec<u8>> {
        let segment = self
            .segments
            .iter()
            .find(|segment| segment.contains(addr))?;
        let available = segment.addr + segment.size - addr;
        let size = size.min(available);
        read_at(&mut self.file, segment.offset + (addr - segment.addr), size)
    }
}

impl Segment {
    fn contains(&self, addr: u64) -> bool {
        self.size > 0 && self.addr <= addr && addr - self.addr < self.size
    }
}

fn read_at(file: &mut File, offset: u64, size: u64) -> Option<Vec<u8>> {
    let size = usize::try_from(size).ok()?;
    let mut buffer = vec![0u8; size];
    file.seek(SeekFrom::Start(offset)).ok()?;
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

fn read_table(file: &mut File, offset: u64, entries: u64, entry_size: u64) -> Option<Vec<u8>> {
    let size = entries.checked_mul(entry_size)?;
    if size > MAX_TABLE_BYTES as u64 {
        return None;
    }
    read_at(file, offset, size)
}

/// Fixed-width reads out of a header, in the header's own byte order.
struct Fields<'a> {
    bytes: &'a [u8],
    little_endian: bool,
}

impl Fields<'_> {
    fn u16(&self, offset: usize) -> Option<u16> {
        let bytes: [u8; 2] = self.bytes.get(offset..offset + 2)?.try_into().ok()?;
        Some(if self.little_endian {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    fn u32(&self, offset: usize) -> Option<u32> {
        let bytes: [u8; 4] = self.bytes.get(offset..offset + 4)?.try_into().ok()?;
        Some(if self.little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    fn u64(&self, offset: usize) -> Option<u64> {
        let bytes: [u8; 8] = self.bytes.get(offset..offset + 8)?.try_into().ok()?;
        Some(if self.little_endian {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        })
    }

    /// A word of the format's pointer width.
    fn word(&self, offset: usize, wide: bool) -> Option<u64> {
        if wide {
            self.u64(offset)
        } else {
            self.u32(offset).map(u64::from)
        }
    }

    /// A NUL-padded fixed-width name.
    fn name(&self, offset: usize, width: usize) -> Option<&[u8]> {
        let field = self.bytes.get(offset..offset + width)?;
        let end = field.iter().position(|&byte| byte == 0).unwrap_or(width);
        Some(&field[..end])
    }
}

// ---------------------------------------------------------------------------------------------
// ELF
// ---------------------------------------------------------------------------------------------

const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;

/// The section the Go linker writes build info into.
const ELF_BUILDINFO_SECTION: &[u8] = b".go.buildinfo";

/// Segments from the program headers; the data region from `.go.buildinfo`, else the first
/// writable non-executable `PT_LOAD`, which is where the linker puts the blob when nothing names it.
fn parse_elf(file: &mut File) -> Option<Layout> {
    // 64 bytes for a 64-bit header, 52 for a 32-bit one; a shorter file is not an ELF.
    let mut ident = Vec::with_capacity(64);
    file.seek(SeekFrom::Start(0)).ok()?;
    file.by_ref().take(64).read_to_end(&mut ident).ok()?;
    if ident.len() < 52 {
        return None;
    }
    let wide = match ident[4] {
        1 => false,
        2 => true,
        _ => return None,
    };
    let little_endian = match ident[5] {
        1 => true,
        2 => false,
        _ => return None,
    };
    let header = Fields {
        bytes: &ident,
        little_endian,
    };

    let (phoff, shoff, phentsize, phnum, shentsize, shnum, shstrndx) = if wide {
        (
            header.u64(0x20)?,
            header.u64(0x28)?,
            header.u16(0x36)?,
            header.u16(0x38)?,
            header.u16(0x3a)?,
            header.u16(0x3c)?,
            header.u16(0x3e)?,
        )
    } else {
        (
            u64::from(header.u32(0x1c)?),
            u64::from(header.u32(0x20)?),
            header.u16(0x2a)?,
            header.u16(0x2c)?,
            header.u16(0x2e)?,
            header.u16(0x30)?,
            header.u16(0x32)?,
        )
    };

    // Program headers: every PT_LOAD is a segment, and the first writable one is the fallback
    // data region.
    let min_phentsize = if wide { 56 } else { 32 };
    if usize::from(phentsize) < min_phentsize {
        return None;
    }
    let table = read_table(file, phoff, u64::from(phnum), u64::from(phentsize))?;
    let table = Fields {
        bytes: &table,
        little_endian,
    };
    let mut segments = Vec::new();
    let mut fallback = None;
    for index in 0..usize::from(phnum) {
        let base = index * usize::from(phentsize);
        let kind = table.u32(base)?;
        let (flags, offset, addr, filesz, memsz) = if wide {
            (
                table.u32(base + 4)?,
                table.u64(base + 8)?,
                table.u64(base + 16)?,
                table.u64(base + 32)?,
                table.u64(base + 40)?,
            )
        } else {
            (
                table.u32(base + 24)?,
                u64::from(table.u32(base + 4)?),
                u64::from(table.u32(base + 8)?),
                u64::from(table.u32(base + 16)?),
                u64::from(table.u32(base + 20)?),
            )
        };
        if kind != PT_LOAD {
            continue;
        }
        segments.push(Segment {
            addr,
            offset,
            size: filesz,
        });
        if fallback.is_none() && flags & (PF_X | PF_W) == PF_W {
            fallback = Some((addr, memsz));
        }
    }

    // Section headers, when the binary still has them: `.go.buildinfo` names the blob exactly.
    let mut data = None;
    let min_shentsize = if wide { 64 } else { 40 };
    if shoff != 0 && shnum > 0 && usize::from(shentsize) >= min_shentsize {
        if let Some(table) = read_table(file, shoff, u64::from(shnum), u64::from(shentsize)) {
            let table = Fields {
                bytes: &table,
                little_endian,
            };
            let section = |index: usize| -> Option<(u32, u64, u64, u64)> {
                let base = index * usize::from(shentsize);
                let name = table.u32(base)?;
                let (addr, offset, size) = if wide {
                    (
                        table.u64(base + 16)?,
                        table.u64(base + 24)?,
                        table.u64(base + 32)?,
                    )
                } else {
                    (
                        u64::from(table.u32(base + 12)?),
                        u64::from(table.u32(base + 16)?),
                        u64::from(table.u32(base + 20)?),
                    )
                };
                Some((name, addr, offset, size))
            };

            let names = section(usize::from(shstrndx))
                .filter(|(_, _, _, size)| *size <= MAX_TABLE_BYTES as u64)
                .and_then(|(_, _, offset, size)| read_at(file, offset, size));
            if let Some(names) = names {
                for index in 0..usize::from(shnum) {
                    let Some((name, addr, _, size)) = section(index) else {
                        break;
                    };
                    let name = names.get(name as usize..).unwrap_or_default();
                    let end = name
                        .iter()
                        .position(|&byte| byte == 0)
                        .unwrap_or(name.len());
                    if &name[..end] == ELF_BUILDINFO_SECTION {
                        data = Some((addr, size));
                        break;
                    }
                }
            }
        }
    }

    Some((segments, data.or(fallback)))
}

// ---------------------------------------------------------------------------------------------
// Mach-O
// ---------------------------------------------------------------------------------------------

const LC_SEGMENT: u32 = 0x1;
const LC_SEGMENT_64: u32 = 0x19;

const MACHO_DATA_SEGMENT: &[u8] = b"__DATA";
const MACHO_BUILDINFO_SECTION: &[u8] = b"__go_buildinfo";

/// Segments from the load commands; the data region from `__go_buildinfo` in `__DATA`, else the
/// whole `__DATA` segment.
fn parse_macho(file: &mut File, magic: &[u8; 4]) -> Option<Layout> {
    // The magic is written as a native word, so its byte order is the file's.
    let little_endian = magic[0] != 0xfe;
    let wide = if little_endian {
        magic[0] == 0xcf
    } else {
        magic[3] == 0xcf
    };
    let header_size = if wide { 32 } else { 28 };

    let header = read_at(file, 0, header_size)?;
    let header = Fields {
        bytes: &header,
        little_endian,
    };
    let ncmds = header.u32(16)?;
    let sizeofcmds = header.u32(20)?;
    if sizeofcmds as usize > MAX_TABLE_BYTES {
        return None;
    }

    let commands = read_at(file, header_size, u64::from(sizeofcmds))?;
    let commands = Fields {
        bytes: &commands,
        little_endian,
    };

    let mut segments = Vec::new();
    let mut data = None;
    let mut fallback = None;
    let mut cursor = 0usize;
    for _ in 0..ncmds {
        let cmd = commands.u32(cursor)?;
        let cmdsize = commands.u32(cursor + 4)? as usize;
        if cmdsize < 8 {
            return None;
        }

        let segment_wide = match cmd {
            LC_SEGMENT => Some(false),
            LC_SEGMENT_64 => Some(true),
            _ => None,
        };
        if let Some(segment_wide) = segment_wide {
            let word = if segment_wide { 8 } else { 4 };
            let name = commands.name(cursor + 8, 16)?;
            let vmaddr = commands.word(cursor + 24, segment_wide)?;
            let vmsize = commands.word(cursor + 24 + word, segment_wide)?;
            let fileoff = commands.word(cursor + 24 + 2 * word, segment_wide)?;
            let filesize = commands.word(cursor + 24 + 3 * word, segment_wide)?;
            let nsects = commands.u32(cursor + 24 + 4 * word + 8)?;
            segments.push(Segment {
                addr: vmaddr,
                offset: fileoff,
                size: filesize,
            });

            if name == MACHO_DATA_SEGMENT {
                if fallback.is_none() {
                    fallback = Some((vmaddr, vmsize));
                }
                let sections = cursor + 24 + 4 * word + 16;
                let section_size = if segment_wide { 80 } else { 68 };
                for index in 0..nsects as usize {
                    let base = sections + index * section_size;
                    if base + section_size > cursor + cmdsize {
                        break;
                    }
                    if commands.name(base, 16)? == MACHO_BUILDINFO_SECTION {
                        let addr = commands.word(base + 32, segment_wide)?;
                        let size = commands.word(base + 32 + word, segment_wide)?;
                        data = Some((addr, size));
                    }
                }
            }
        }

        cursor += cmdsize;
    }

    Some((segments, data.or(fallback)))
}

// ---------------------------------------------------------------------------------------------
// PE
// ---------------------------------------------------------------------------------------------

const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;
const IMAGE_SCN_ALIGN_32BYTES: u32 = 0x0060_0000;

/// Sections as segments, offset by the image base; the data region is the first section that is
/// exactly initialised, readable and writable data, which is the test Go's reader applies.
fn parse_pe(file: &mut File) -> Option<Layout> {
    let dos = read_at(file, 0, 0x40)?;
    let dos = Fields {
        bytes: &dos,
        little_endian: true,
    };
    let pe_offset = u64::from(dos.u32(0x3c)?);

    // Signature, COFF header, and the start of the optional header.
    let coff = read_at(file, pe_offset, 4 + 20 + 2)?;
    let coff = Fields {
        bytes: &coff,
        little_endian: true,
    };
    if &coff.bytes[..4] != b"PE\0\0" {
        return None;
    }
    let sections = coff.u16(4 + 2)?;
    let optional_size = coff.u16(4 + 16)?;
    let optional_offset = pe_offset + 4 + 20;
    let optional = read_at(file, optional_offset, u64::from(optional_size))?;
    let optional = Fields {
        bytes: &optional,
        little_endian: true,
    };
    let image_base = match optional.u16(0)? {
        0x10b => u64::from(optional.u32(28)?),
        0x20b => optional.u64(24)?,
        _ => return None,
    };

    let table = read_table(
        file,
        optional_offset + u64::from(optional_size),
        u64::from(sections),
        40,
    )?;
    let table = Fields {
        bytes: &table,
        little_endian: true,
    };
    let mut segments = Vec::new();
    let mut data = None;
    for index in 0..usize::from(sections) {
        let base = index * 40;
        let virtual_size = u64::from(table.u32(base + 8)?);
        let virtual_address = u64::from(table.u32(base + 12)?);
        let raw_size = u64::from(table.u32(base + 16)?);
        let raw_offset = u64::from(table.u32(base + 20)?);
        let characteristics = table.u32(base + 36)?;

        segments.push(Segment {
            addr: image_base + virtual_address,
            offset: raw_offset,
            size: raw_size,
        });
        if data.is_none()
            && virtual_address != 0
            && raw_size != 0
            && characteristics & !IMAGE_SCN_ALIGN_32BYTES
                == IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE
        {
            data = Some((image_base + virtual_address, virtual_size));
        }
    }

    Some((segments, data))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// What a synthesised executable is asked to carry: one data segment at a virtual address,
    /// holding these bytes.
    pub struct Synth {
        pub data_addr: u64,
        pub data: Vec<u8>,
    }

    /// A 64-bit little-endian ELF with a text segment and a writable data segment, and section
    /// headers naming the data `.go.buildinfo` when asked.
    pub fn elf(synth: &Synth, with_sections: bool) -> Vec<u8> {
        let text_addr = 0x400000u64;
        let text = b"\xcc".repeat(64);
        let headers = 64 + 2 * 56; // ELF header + two program headers
        let text_offset = 0x100u64;
        let data_offset = 0x200u64;
        let shoff = 0x1000u64;

        let mut out = vec![0u8; shoff as usize];
        out[..4].copy_from_slice(b"\x7fELF");
        out[4] = 2; // 64-bit
        out[5] = 1; // little-endian
        out[6] = 1;
        out[0x10..0x12].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        out[0x12..0x14].copy_from_slice(&0x3eu16.to_le_bytes()); // x86-64
        out[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        out[0x20..0x28].copy_from_slice(&64u64.to_le_bytes()); // phoff
        out[0x34..0x36].copy_from_slice(&64u16.to_le_bytes()); // ehsize
        out[0x36..0x38].copy_from_slice(&56u16.to_le_bytes()); // phentsize
        out[0x38..0x3a].copy_from_slice(&2u16.to_le_bytes()); // phnum
        assert!(headers <= text_offset as usize);

        let mut phdr = |index: usize, flags: u32, offset: u64, addr: u64, size: u64| {
            let base = 64 + index * 56;
            out[base..base + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
            out[base + 4..base + 8].copy_from_slice(&flags.to_le_bytes());
            out[base + 8..base + 16].copy_from_slice(&offset.to_le_bytes());
            out[base + 16..base + 24].copy_from_slice(&addr.to_le_bytes());
            out[base + 24..base + 32].copy_from_slice(&addr.to_le_bytes());
            out[base + 32..base + 40].copy_from_slice(&size.to_le_bytes());
            out[base + 40..base + 48].copy_from_slice(&(size + 0x100).to_le_bytes()); // bss
            out[base + 48..base + 56].copy_from_slice(&16u64.to_le_bytes());
        };
        phdr(0, PF_X | 4, text_offset, text_addr, text.len() as u64);
        phdr(
            1,
            PF_W | 4,
            data_offset,
            synth.data_addr,
            synth.data.len() as u64,
        );

        out[text_offset as usize..text_offset as usize + text.len()].copy_from_slice(&text);
        let data_end = data_offset as usize + synth.data.len();
        assert!(data_end <= shoff as usize, "data too large for the fixture");
        out[data_offset as usize..data_end].copy_from_slice(&synth.data);

        if with_sections {
            // Three sections: null, .go.buildinfo, .shstrtab.
            let names = b"\0.go.buildinfo\0.shstrtab\0";
            let names_offset = shoff + 3 * 64;
            out[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
            out[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
            out[0x3c..0x3e].copy_from_slice(&3u16.to_le_bytes());
            out[0x3e..0x40].copy_from_slice(&2u16.to_le_bytes());
            let mut shdr = vec![0u8; 3 * 64];
            let mut section =
                |index: usize, name: u32, kind: u32, addr: u64, offset: u64, size: u64| {
                    let base = index * 64;
                    shdr[base..base + 4].copy_from_slice(&name.to_le_bytes());
                    shdr[base + 4..base + 8].copy_from_slice(&kind.to_le_bytes());
                    shdr[base + 16..base + 24].copy_from_slice(&addr.to_le_bytes());
                    shdr[base + 24..base + 32].copy_from_slice(&offset.to_le_bytes());
                    shdr[base + 32..base + 40].copy_from_slice(&size.to_le_bytes());
                };
            // SHT_PROGBITS for the data, SHT_STRTAB for the names: Go's `debug/elf` refuses a
            // string table of any other type, and the fixture should be one it reads too.
            section(
                1,
                1,
                1,
                synth.data_addr,
                data_offset,
                synth.data.len() as u64,
            );
            section(2, 15, 3, 0, names_offset, names.len() as u64);
            out.extend_from_slice(&shdr);
            out.extend_from_slice(names);
        }
        out
    }

    /// A 64-bit little-endian Mach-O with `__TEXT` and `__DATA` segments, the latter holding a
    /// `__go_buildinfo` section when asked.
    pub fn macho(synth: &Synth, with_section: bool) -> Vec<u8> {
        let text_addr = 0x1_0000_0000u64;
        let data_offset = 0x1000u64;
        let nsects: u32 = if with_section { 1 } else { 0 };
        let segment_size = 72 + 80 * nsects as usize;

        let mut out = Vec::new();
        out.extend_from_slice(&0xfeed_facfu32.to_le_bytes());
        out.extend_from_slice(&0x0100_0007u32.to_le_bytes()); // x86-64
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes()); // MH_EXECUTE
        out.extend_from_slice(&2u32.to_le_bytes()); // ncmds
        out.extend_from_slice(&((72 + segment_size) as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        let mut segment = |name: &[u8], addr: u64, offset: u64, size: u64, nsects: u32| {
            out.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
            out.extend_from_slice(&(72 + 80 * nsects).to_le_bytes());
            let mut padded = [0u8; 16];
            padded[..name.len()].copy_from_slice(name);
            out.extend_from_slice(&padded);
            out.extend_from_slice(&addr.to_le_bytes());
            out.extend_from_slice(&(size + 0x100).to_le_bytes()); // vmsize, with bss
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&7u32.to_le_bytes());
            out.extend_from_slice(&3u32.to_le_bytes());
            out.extend_from_slice(&nsects.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
        };
        segment(b"__TEXT", text_addr, 0, 0x1000, 0);
        segment(
            b"__DATA",
            synth.data_addr,
            data_offset,
            synth.data.len() as u64,
            nsects,
        );
        if with_section {
            let mut sectname = [0u8; 16];
            sectname[..MACHO_BUILDINFO_SECTION.len()].copy_from_slice(MACHO_BUILDINFO_SECTION);
            let mut segname = [0u8; 16];
            segname[..6].copy_from_slice(b"__DATA");
            out.extend_from_slice(&sectname);
            out.extend_from_slice(&segname);
            out.extend_from_slice(&synth.data_addr.to_le_bytes());
            out.extend_from_slice(&(synth.data.len() as u64).to_le_bytes());
            out.extend_from_slice(&(data_offset as u32).to_le_bytes());
            out.extend_from_slice(&4u32.to_le_bytes());
            out.extend_from_slice(&[0u8; 24]);
        }

        out.resize(data_offset as usize, 0);
        out.extend_from_slice(&synth.data);
        out
    }

    /// A PE32+ with a `.text` and a `.data` section.
    pub fn pe(synth: &Synth) -> Vec<u8> {
        let image_base = 0x1_4000_0000u64;
        let data_rva = (synth.data_addr - image_base) as u32;
        let data_offset = 0x600u32;

        let mut out = vec![0u8; 0x40];
        out[..2].copy_from_slice(b"MZ");
        out[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        out.resize(0x80, 0);

        out.extend_from_slice(b"PE\0\0");
        out.extend_from_slice(&0x8664u16.to_le_bytes()); // machine
        out.extend_from_slice(&2u16.to_le_bytes()); // sections
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(&240u16.to_le_bytes()); // optional header size
        out.extend_from_slice(&0x22u16.to_le_bytes());

        let mut optional = vec![0u8; 240];
        optional[..2].copy_from_slice(&0x20bu16.to_le_bytes());
        optional[24..32].copy_from_slice(&image_base.to_le_bytes());
        out.extend_from_slice(&optional);

        let mut section = |name: &[u8], rva: u32, size: u32, offset: u32, flags: u32| {
            let mut padded = [0u8; 8];
            padded[..name.len()].copy_from_slice(name);
            out.extend_from_slice(&padded);
            out.extend_from_slice(&(size + 0x100).to_le_bytes()); // virtual size, with bss
            out.extend_from_slice(&rva.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&[0u8; 12]);
            out.extend_from_slice(&flags.to_le_bytes());
        };
        section(b".text", 0x1000, 0x200, 0x400, 0x6000_0020);
        section(
            b".data",
            data_rva,
            synth.data.len() as u32,
            data_offset,
            IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE,
        );

        out.resize(data_offset as usize, 0);
        out.extend_from_slice(&synth.data);
        out
    }

    fn write_temp(bytes: &[u8]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), bytes).unwrap();
        file
    }

    fn synth() -> Synth {
        Synth {
            data_addr: 0x500000,
            data: b"hello, data segment".to_vec(),
        }
    }

    #[test]
    fn test_sniffs_the_three_formats_and_nothing_else() {
        let synth = synth();
        let pe_synth = Synth {
            data_addr: 0x1_4000_2000,
            data: synth.data.clone(),
        };
        for bytes in [elf(&synth, true), macho(&synth, true), pe(&pe_synth)] {
            assert!(is_executable(write_temp(&bytes).path()));
        }
        assert!(!is_executable(write_temp(b"#!/bin/sh\necho hi\n").path()));
        assert!(!is_executable(write_temp(b"").path()));
        assert!(!is_executable(Path::new("/nonexistent/feluda-fixture")));
    }

    #[test]
    fn test_elf_names_its_buildinfo_section() {
        let synth = synth();
        let file = write_temp(&elf(&synth, true));
        let mut exe = Executable::open(file.path()).unwrap();
        assert_eq!(
            exe.data_start(),
            Some((synth.data_addr, synth.data.len() as u64))
        );
        assert_eq!(
            exe.read_data(synth.data_addr + 7, 4).unwrap(),
            b"data".to_vec()
        );
    }

    #[test]
    fn test_elf_without_section_headers_falls_back_to_the_writable_segment() {
        let synth = synth();
        let file = write_temp(&elf(&synth, false));
        let exe = Executable::open(file.path()).unwrap();
        // The segment's memory size includes the bss the fixture adds.
        assert_eq!(
            exe.data_start(),
            Some((synth.data_addr, synth.data.len() as u64 + 0x100))
        );
    }

    #[test]
    fn test_reads_stop_at_the_end_of_a_segment() {
        let synth = synth();
        let file = write_temp(&elf(&synth, true));
        let mut exe = Executable::open(file.path()).unwrap();
        // Asking past the file-backed bytes yields what is there, not an error, and an address
        // in no segment yields nothing.
        assert_eq!(exe.read_data(synth.data_addr, 1 << 20).unwrap(), synth.data);
        assert!(exe.read_data(0x900000, 4).is_none());
    }

    #[test]
    fn test_macho_names_its_buildinfo_section() {
        let synth = synth();
        let file = write_temp(&macho(&synth, true));
        let mut exe = Executable::open(file.path()).unwrap();
        assert_eq!(
            exe.data_start(),
            Some((synth.data_addr, synth.data.len() as u64))
        );
        assert_eq!(
            exe.read_data(synth.data_addr, 5).unwrap(),
            b"hello".to_vec()
        );
    }

    #[test]
    fn test_macho_without_the_section_falls_back_to_the_data_segment() {
        let synth = synth();
        let file = write_temp(&macho(&synth, false));
        let exe = Executable::open(file.path()).unwrap();
        assert_eq!(
            exe.data_start(),
            Some((synth.data_addr, synth.data.len() as u64 + 0x100))
        );
    }

    #[test]
    fn test_pe_finds_the_first_writable_data_section() {
        let synth = Synth {
            data_addr: 0x1_4000_2000,
            data: b"hello, data segment".to_vec(),
        };
        let file = write_temp(&pe(&synth));
        let mut exe = Executable::open(file.path()).unwrap();
        assert_eq!(
            exe.data_start(),
            Some((synth.data_addr, synth.data.len() as u64 + 0x100))
        );
        assert_eq!(
            exe.read_data(synth.data_addr + 7, 4).unwrap(),
            b"data".to_vec()
        );
    }

    #[test]
    fn test_a_truncated_header_is_not_an_executable() {
        let synth = synth();
        let bytes = elf(&synth, true);
        let file = write_temp(&bytes[..0x30]);
        assert!(Executable::open(file.path()).is_none());
        let file = write_temp(b"MZ\0\0");
        assert!(Executable::open(file.path()).is_none());
    }
}
