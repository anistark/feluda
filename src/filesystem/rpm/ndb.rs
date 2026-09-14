//! A read only reader for rpm's ndb package store, `Packages.db`.
//!
//! ndb is rpm's own format, written for SUSE and the default there since 2018. Unlike the sqlite
//! backend it is not a general database: `Packages.db` is a flat file of header blobs with a slot
//! table at the front saying where each one is, and the indexes rpm needs live in a separate
//! `Index.db` this never opens. Everything is little endian and everything is in units of 16 byte
//! blocks, which is what keeps the reader short.
//!
//! The layout, from `lib/backend/ndb/rpmpkg.c` in rpm:
//!
//! ```text
//! file header, 32 bytes:   "RpmP" | version (0) | generation | slotnpages | nextpkgidx | 12 pad
//! slot area:               slotnpages * 4096 bytes of 16 byte slots, the first two being the header
//!   one slot:              "Slot" | pkgidx | blkoff | blkcnt
//! blob area:               one blob per slot, at blkoff * 16, blkcnt * 16 bytes long
//!   one blob:              "BlbS" | pkgidx | generation | bloblen | header blob | zero pad |
//!                          adler32 | bloblen | "BlbE"
//! ```
//!
//! The header blob inside is the same `headerExport` image the sqlite backend keeps in its `blob`
//! column, so what comes out of here goes straight into the shared header parser.

use std::path::Path;

use crate::debug::{log, FeludaResult, LogLevel};

use super::backend_error;

/// The magic every ndb package store begins with, and the one version rpm has ever written.
const MAGIC: &[u8] = b"RpmP";
const VERSION: u32 = 0;

/// The file header occupies the first two slots of the first slot page.
const HEADER_SIZE: usize = 32;
const OFFSET_VERSION: usize = 4;
const OFFSET_SLOTNPAGES: usize = 12;

/// Slot pages are fixed size, and slots and blob blocks are both 16 bytes.
const PAGE_SIZE: usize = 4096;
const SLOT_SIZE: usize = 16;
const BLOCK_SIZE: usize = 16;

/// Slot layout.
const SLOT_MAGIC: &[u8] = b"Slot";
const SLOT_OFFSET_PKGIDX: usize = 4;
const SLOT_OFFSET_BLKOFF: usize = 8;
const SLOT_OFFSET_BLKCNT: usize = 12;

/// Blob framing. The head is `magic, pkgidx, generation, bloblen`; the tail is
/// `adler32, bloblen, magic`.
const BLOB_HEAD_MAGIC: &[u8] = b"BlbS";
const BLOB_TAIL_MAGIC: &[u8] = b"BlbE";
const BLOB_HEAD_SIZE: usize = 16;
const BLOB_TAIL_SIZE: usize = 12;
const BLOB_HEAD_OFFSET_PKGIDX: usize = 4;
const BLOB_HEAD_OFFSET_LEN: usize = 12;
const BLOB_TAIL_OFFSET_ADLER: usize = 0;
const BLOB_TAIL_OFFSET_LEN: usize = 4;
const BLOB_TAIL_OFFSET_MAGIC: usize = 8;

/// An open package store, held in memory like the sqlite one.
#[derive(Debug)]
pub struct Database {
    bytes: Vec<u8>,
    /// How many 4096 byte pages at the front of the file hold slots.
    slot_pages: usize,
}

/// One occupied slot: which package it is and where its blob lives, in blocks.
#[derive(Debug, PartialEq, Eq)]
struct Slot {
    pkgidx: u32,
    block_offset: usize,
    block_count: usize,
}

impl Database {
    /// Open a package store and validate enough of its header to know where the slots end.
    pub fn open(path: &Path) -> FeludaResult<Self> {
        let bytes = std::fs::read(path).map_err(|error| {
            backend_error(format!("Failed to read {}: {error}", path.display()))
        })?;
        Self::from_bytes(bytes, &path.display().to_string())
    }

    fn from_bytes(bytes: Vec<u8>, source: &str) -> FeludaResult<Self> {
        if bytes.len() < HEADER_SIZE || !bytes.starts_with(MAGIC) {
            return Err(backend_error(format!(
                "{source} is not an rpm ndb package store. The rpm ndb backend expects one at this path."
            )));
        }

        let version = read_u32(&bytes, OFFSET_VERSION);
        if version != VERSION {
            return Err(backend_error(format!(
                "{source} is ndb version {version}, and only version {VERSION} is understood."
            )));
        }

        let slot_pages = read_u32(&bytes, OFFSET_SLOTNPAGES) as usize;
        if slot_pages == 0 || slot_pages.saturating_mul(PAGE_SIZE) > bytes.len() {
            return Err(backend_error(format!(
                "{source} declares {slot_pages} slot pages, which do not fit in the file."
            )));
        }

        Ok(Self { bytes, slot_pages })
    }

    /// Every header blob in the store, in package index order.
    ///
    /// Package index order is install order, which is also the order `rpm -qa` lists in. A blob
    /// whose framing does not check out is skipped with a warning rather than failing the scan:
    /// rpm itself would refuse to open the store, but one damaged record is not a reason to report
    /// a whole image as unreadable.
    pub fn blobs(&self) -> FeludaResult<Vec<Vec<u8>>> {
        let mut slots = self.slots()?;
        slots.sort_by_key(|slot| slot.pkgidx);

        Ok(slots
            .iter()
            .filter_map(|slot| {
                let blob = self.blob(slot);
                if blob.is_none() {
                    log(
                        LogLevel::Warn,
                        &format!(
                            "Skipped rpm package {} in Packages.db: its record is damaged",
                            slot.pkgidx
                        ),
                    );
                }
                blob
            })
            .collect())
    }

    /// Read the slot area. An empty slot has a zero block offset; every other slot points at a
    /// blob, and the pointer is checked against the file before it is trusted.
    fn slots(&self) -> FeludaResult<Vec<Slot>> {
        let end = self.slot_pages * PAGE_SIZE;
        let file_blocks = self.bytes.len() / BLOCK_SIZE;
        // Blobs live after the slot pages, never inside them.
        let first_blob_block = end / BLOCK_SIZE;
        let mut slots = Vec::new();

        for offset in (HEADER_SIZE..end).step_by(SLOT_SIZE) {
            let slot = &self.bytes[offset..offset + SLOT_SIZE];
            if !slot.starts_with(SLOT_MAGIC) {
                return Err(backend_error(format!(
                    "The rpm ndb store has a bad slot at byte {offset} and is corrupt."
                )));
            }

            let block_offset = read_u32(slot, SLOT_OFFSET_BLKOFF) as usize;
            if block_offset == 0 {
                continue;
            }
            let pkgidx = read_u32(slot, SLOT_OFFSET_PKGIDX);
            let block_count = read_u32(slot, SLOT_OFFSET_BLKCNT) as usize;

            if pkgidx == 0
                || block_count == 0
                || block_offset < first_blob_block
                || block_offset.saturating_add(block_count) > file_blocks
            {
                return Err(backend_error(format!(
                    "The rpm ndb store points package {pkgidx} outside the file and is corrupt \
                     or truncated."
                )));
            }

            slots.push(Slot {
                pkgidx,
                block_offset,
                block_count,
            });
        }

        Ok(slots)
    }

    /// The header blob a slot points at, or `None` when its framing does not check out.
    ///
    /// Checks the same things rpm does on read: both magics, that the record names the package the
    /// slot said it would, that the declared length fits the blocks the slot allotted, and the
    /// adler32 over everything before the tail.
    fn blob(&self, slot: &Slot) -> Option<Vec<u8>> {
        let start = slot.block_offset * BLOCK_SIZE;
        let record = &self.bytes[start..start + slot.block_count * BLOCK_SIZE];
        if record.len() < BLOB_HEAD_SIZE + BLOB_TAIL_SIZE {
            return None;
        }

        let (head, rest) = record.split_at(BLOB_HEAD_SIZE);
        if !head.starts_with(BLOB_HEAD_MAGIC)
            || read_u32(head, BLOB_HEAD_OFFSET_PKGIDX) != slot.pkgidx
        {
            return None;
        }
        let length = read_u32(head, BLOB_HEAD_OFFSET_LEN) as usize;
        if blocks_for(length) != slot.block_count {
            return None;
        }

        let (body, tail) = rest.split_at(rest.len() - BLOB_TAIL_SIZE);
        if !tail[BLOB_TAIL_OFFSET_MAGIC..].starts_with(BLOB_TAIL_MAGIC)
            || read_u32(tail, BLOB_TAIL_OFFSET_LEN) as usize != length
        {
            return None;
        }

        // The checksum runs over the head, the blob and its zero padding, but not the tail.
        let mut adler = Adler32::default();
        adler.update(head);
        adler.update(body);
        if adler.finish() != read_u32(tail, BLOB_TAIL_OFFSET_ADLER) {
            return None;
        }

        Some(body[..length].to_vec())
    }
}

/// How many 16 byte blocks a record holding a blob of `length` bytes occupies, with framing.
fn blocks_for(length: usize) -> usize {
    (BLOB_HEAD_SIZE + length + BLOB_TAIL_SIZE).div_ceil(BLOCK_SIZE)
}

/// A little endian u32 at `offset`. Callers have already bounds checked the slice.
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("four bytes were sliced"),
    )
}

/// The adler32 from RFC 1950, which is what rpm frames each blob with.
struct Adler32 {
    s1: u32,
    s2: u32,
}

impl Default for Adler32 {
    fn default() -> Self {
        Self { s1: 1, s2: 0 }
    }
}

impl Adler32 {
    /// The largest run of bytes the sums can take before a modulo is needed to stay in a u32.
    const NMAX: usize = 5552;
    const MODULUS: u32 = 65521;

    fn update(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(Self::NMAX) {
            for byte in chunk {
                self.s1 += *byte as u32;
                self.s2 += self.s1;
            }
            self.s1 %= Self::MODULUS;
            self.s2 %= Self::MODULUS;
        }
    }

    fn finish(&self) -> u32 {
        (self.s2 << 16) | self.s1
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Build a package store the way rpm would: one slot page, and each blob framed, padded and
    /// checksummed. Used to make the fixtures the reader is tested against, since a hand written
    /// store is the only way to exercise a corrupt one deterministically.
    pub fn build_store(blobs: &[(u32, &[u8])]) -> Vec<u8> {
        let mut file = vec![0u8; PAGE_SIZE];
        file[..4].copy_from_slice(MAGIC);
        file[OFFSET_VERSION..OFFSET_VERSION + 4].copy_from_slice(&VERSION.to_le_bytes());
        file[OFFSET_SLOTNPAGES..OFFSET_SLOTNPAGES + 4].copy_from_slice(&1u32.to_le_bytes());
        for offset in (HEADER_SIZE..PAGE_SIZE).step_by(SLOT_SIZE) {
            file[offset..offset + 4].copy_from_slice(SLOT_MAGIC);
        }

        for (index, (pkgidx, blob)) in blobs.iter().enumerate() {
            let block_offset = (file.len() / BLOCK_SIZE) as u32;
            let block_count = blocks_for(blob.len()) as u32;

            let slot = HEADER_SIZE + index * SLOT_SIZE;
            file[slot + SLOT_OFFSET_PKGIDX..slot + SLOT_OFFSET_PKGIDX + 4]
                .copy_from_slice(&pkgidx.to_le_bytes());
            file[slot + SLOT_OFFSET_BLKOFF..slot + SLOT_OFFSET_BLKOFF + 4]
                .copy_from_slice(&block_offset.to_le_bytes());
            file[slot + SLOT_OFFSET_BLKCNT..slot + SLOT_OFFSET_BLKCNT + 4]
                .copy_from_slice(&block_count.to_le_bytes());

            let mut record = Vec::new();
            record.extend_from_slice(BLOB_HEAD_MAGIC);
            record.extend_from_slice(&pkgidx.to_le_bytes());
            record.extend_from_slice(&1u32.to_le_bytes()); // generation
            record.extend_from_slice(&(blob.len() as u32).to_le_bytes());
            record.extend_from_slice(blob);
            let padded = block_count as usize * BLOCK_SIZE - BLOB_TAIL_SIZE;
            record.resize(padded, 0);

            let mut adler = Adler32::default();
            adler.update(&record);
            record.extend_from_slice(&adler.finish().to_le_bytes());
            record.extend_from_slice(&(blob.len() as u32).to_le_bytes());
            record.extend_from_slice(BLOB_TAIL_MAGIC);

            file.extend_from_slice(&record);
        }

        file
    }

    /// The checked in fixture: a store rpm wrote inside `opensuse/leap:15.6`.
    fn fixture() -> Database {
        Database::open(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/rpm/Packages.db"
        )))
        .expect("fixture should open")
    }

    #[test]
    fn test_reads_every_record_of_the_fixture() {
        // Eight packages and two imported signing keys.
        let blobs = fixture().blobs().unwrap();
        assert_eq!(blobs.len(), 10);

        // Every blob is self describing, so the header's own declared sizes have to account for
        // exactly what the reader returned: neither the padding nor the tail leaked in.
        for blob in &blobs {
            let nindex = u32::from_be_bytes(blob[0..4].try_into().unwrap()) as usize;
            let hsize = u32::from_be_bytes(blob[4..8].try_into().unwrap()) as usize;
            assert_eq!(blob.len(), 8 + nindex * 16 + hsize, "blob does not add up");
        }
    }

    #[test]
    fn test_the_fixture_was_written_by_rpm_not_by_this_module() {
        // rpm leaves the first slot page mostly empty and writes generation numbers, which
        // `build_store` never does. A fixture that this reader could have produced would prove
        // less.
        let database = fixture();
        let generation = read_u32(&database.bytes, 8);
        assert!(generation > 0, "rpm always bumps the generation on write");
    }

    #[test]
    fn test_adler32_matches_the_reference_values() {
        // From RFC 1950 and zlib's own test vectors.
        let mut empty = Adler32::default();
        empty.update(b"");
        assert_eq!(empty.finish(), 1);

        let mut wikipedia = Adler32::default();
        wikipedia.update(b"Wikipedia");
        assert_eq!(wikipedia.finish(), 0x11E60398);
    }

    #[test]
    fn test_adler32_is_the_same_split_or_whole() {
        let bytes: Vec<u8> = (0..20_000u32).map(|value| (value % 251) as u8).collect();
        let mut whole = Adler32::default();
        whole.update(&bytes);
        let mut split = Adler32::default();
        split.update(&bytes[..7_000]);
        split.update(&bytes[7_000..]);
        assert_eq!(whole.finish(), split.finish());
    }

    #[test]
    fn test_blocks_include_framing_and_round_up() {
        // 16 head + 0 + 12 tail = 28 bytes, two blocks.
        assert_eq!(blocks_for(0), 2);
        // 16 + 4 + 12 = 32, exactly two blocks.
        assert_eq!(blocks_for(4), 2);
        assert_eq!(blocks_for(5), 3);
    }

    #[test]
    fn test_rejects_a_file_that_is_not_ndb() {
        let error = Database::from_bytes(vec![0u8; 200], "test").unwrap_err();
        assert!(
            error.to_string().contains("not an rpm ndb package store"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_a_truncated_header() {
        let error = Database::from_bytes(MAGIC.to_vec(), "test").unwrap_err();
        assert!(
            error.to_string().contains("not an rpm ndb package store"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_an_unknown_version() {
        let mut store = build_store(&[]);
        store[OFFSET_VERSION] = 7;
        let error = Database::from_bytes(store, "test").unwrap_err();
        assert!(
            error.to_string().contains("ndb version 7"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_slot_pages_past_the_end_of_the_file() {
        let mut store = build_store(&[]);
        store[OFFSET_SLOTNPAGES] = 9;
        let error = Database::from_bytes(store, "test").unwrap_err();
        assert!(
            error.to_string().contains("slot pages"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_an_empty_store_has_no_blobs() {
        let database = Database::from_bytes(build_store(&[]), "test").unwrap();
        assert!(database.blobs().unwrap().is_empty());
    }

    #[test]
    fn test_reads_blobs_back_in_package_index_order() {
        let store = build_store(&[(3, b"third"), (1, b"first"), (2, b"second and longer")]);
        let database = Database::from_bytes(store, "test").unwrap();
        let blobs = database.blobs().unwrap();
        assert_eq!(
            blobs,
            vec![
                b"first".to_vec(),
                b"second and longer".to_vec(),
                b"third".to_vec()
            ]
        );
    }

    #[test]
    fn test_a_blob_longer_than_a_page_is_read_whole() {
        let big: Vec<u8> = (0..10_000u32).map(|value| value as u8).collect();
        let store = build_store(&[(1, &big)]);
        let database = Database::from_bytes(store, "test").unwrap();
        assert_eq!(database.blobs().unwrap(), vec![big]);
    }

    #[test]
    fn test_a_bad_slot_magic_is_corruption() {
        let mut store = build_store(&[(1, b"x")]);
        store[HEADER_SIZE] = b'X';
        let error = Database::from_bytes(store, "test")
            .unwrap()
            .blobs()
            .unwrap_err();
        assert!(
            error.to_string().contains("bad slot"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_a_slot_pointing_past_the_file_is_truncation() {
        let mut store = build_store(&[(1, b"x")]);
        // Drop the blob's last block.
        store.truncate(store.len() - BLOCK_SIZE);
        let error = Database::from_bytes(store, "test")
            .unwrap()
            .blobs()
            .unwrap_err();
        assert!(
            error.to_string().contains("truncated"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_a_flipped_byte_fails_the_checksum_and_skips_that_blob() {
        let store = build_store(&[(1, b"intact"), (2, b"damaged")]);
        let database = Database::from_bytes(store, "test").unwrap();
        let mut bytes = database.bytes.clone();
        // The second record's body: past the slot page, the first record, and the head.
        let second = PAGE_SIZE + blocks_for(6) * BLOCK_SIZE + BLOB_HEAD_SIZE;
        bytes[second] ^= 0xff;

        let damaged = Database::from_bytes(bytes, "test").unwrap();
        assert_eq!(damaged.blobs().unwrap(), vec![b"intact".to_vec()]);
    }

    #[test]
    fn test_a_blob_whose_head_names_another_package_is_skipped() {
        let store = build_store(&[(1, b"one"), (2, b"two")]);
        let database = Database::from_bytes(store, "test").unwrap();
        let mut bytes = database.bytes.clone();
        let second_head = PAGE_SIZE + blocks_for(3) * BLOCK_SIZE + BLOB_HEAD_OFFSET_PKGIDX;
        bytes[second_head] = 9;

        let damaged = Database::from_bytes(bytes, "test").unwrap();
        assert_eq!(damaged.blobs().unwrap(), vec![b"one".to_vec()]);
    }
}
