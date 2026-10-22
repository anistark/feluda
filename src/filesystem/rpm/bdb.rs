//! A read only reader for rpm's Berkeley DB package store, `Packages`.
//!
//! This is the backend RHEL 8, Rocky Linux 8, AlmaLinux 8 and the `ubi8` images ship, and the last
//! one rpm used before it moved to sqlite. It is also what CentOS 7 and Amazon Linux 2 ship: rpm
//! 4.11 on libdb 5.3 writes the same hash version 9 store, which is read here too. `Packages` is a
//! plain Berkeley DB hash database: the key is a package's header number and the value is its
//! header blob, the same blob the sqlite and ndb readers hand to `header.rs`. Nothing here needs
//! the hash function or the bucket table, because a full dump never looks a key up: walking the
//! pages and their overflow chains is enough.
//!
//! The layout, from `db_page.h` in libdb, which is what rpm 4.17's own read only backend
//! (`lib/backend/bdb_ro.c`) follows:
//!
//! ```text
//! meta page, page 0:
//!   12  magic       0x00061561, or 0x61150600 when the other endian wrote the file
//!   16  version     9 for every hash database libdb 4.6 or later writes
//!   20  page size   4096 or 8192 in practice
//!   24  encrypt alg 0 in every rpm database; anything else puts a per page header in the way
//!   26  meta flags  a checksum flag has the same effect, and rpm never sets it either
//!   32  last page   the highest page number the file holds
//!
//! hash page, every page of type 13 (2 on a store written before libdb 4.6):
//!   20  entries     the item count, always even: key, value, key, value...
//!   26  slots       entries * 2 bytes, holding where each item starts in the page
//!   item            [type: u8][bytes]: H_KEYDATA is the key or a value that fits on the page,
//!                   H_OFFPAGE is a 12 byte record of a page number and a length
//!
//! overflow page, every page of type 7:
//!   16  next        the next page of the chain, 0 at its end
//!   22  length      how many bytes of the value sit on this page
//!   26  data        the bytes themselves
//! ```
//!
//! Header blobs run from a few hundred bytes to over a megabyte, so nearly every value lives on an
//! overflow chain rather than on the hash page, and the chain walk is the common path here as it is
//! in the sqlite reader.
//!
//! Key 0 holds no header. rpm keeps the last header number it allocated there, in
//! `pkgInstance` in `lib/backend/db3.c`, and starts packages at 1, so a scan skips it.

use std::path::Path;

use crate::debug::{log, FeludaResult, LogLevel};

use super::backend_error;

/// The magic a hash database begins with, and the one a database written by the other endian
/// reads back as.
const MAGIC: u32 = 0x0006_1561;
const MAGIC_SWAPPED: u32 = 0x6115_0600;

/// The hash versions a store can carry. 8 is the unsorted page layout libdb wrote before 4.6, and 9
/// is what 4.6 and later write, the 5.3 in EL7 and EL8 included. 10 is accepted on the assumption
/// that a later libdb kept the layout; no rpm release has ever written one.
const MIN_VERSION: u32 = 8;
const MAX_VERSION: u32 = 10;

/// The smallest page libdb writes, and the size of the meta page it keeps its header in.
const SMALLEST_PAGE_SIZE: usize = 512;

/// Where the meta page keeps what the rest of the file depends on.
const OFFSET_MAGIC: usize = 12;
const OFFSET_VERSION: usize = 16;
const OFFSET_PAGE_SIZE: usize = 20;
const OFFSET_ENCRYPT_ALG: usize = 24;
const OFFSET_METAFLAGS: usize = 26;
const OFFSET_LAST_PAGE: usize = 32;

/// The page header every page type shares, and where the items of a hash page and the data of an
/// overflow page therefore begin.
const OFFSET_NEXT_PAGE: usize = 16;
const OFFSET_ENTRIES: usize = 20;
const OFFSET_HIGH_FREE: usize = 22;
const OFFSET_TYPE: usize = 25;
const PAGE_HEADER: usize = 26;

/// Page types, from `db_page.h`. `P_HASH_UNSORTED` is what a pre-4.6 libdb wrote, and its items are
/// laid out the same way.
const P_HASH_UNSORTED: u8 = 2;
const P_OVERFLOW: u8 = 7;
const P_HASH: u8 = 13;

/// Item types on a hash page.
const H_KEYDATA: u8 = 1;
const H_OFFPAGE: u8 = 3;

/// An off page item: `type, 3 padding bytes, page number, total length`.
const HOFFPAGE_SIZE: usize = 12;
const HOFFPAGE_PAGE: usize = 4;
const HOFFPAGE_LENGTH: usize = 8;

/// The key rpm stores the last allocated header number under.
const INSTANCE_KEY: u32 = 0;

/// The byte order a database was written in, which libdb records only in the magic number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    Little,
    Big,
}

impl Order {
    /// A u16 in this database's byte order. Callers have already bounds checked the slice.
    fn u16(self, bytes: &[u8], offset: usize) -> u16 {
        let word = bytes[offset..offset + 2]
            .try_into()
            .expect("two bytes were sliced");
        match self {
            Order::Little => u16::from_le_bytes(word),
            Order::Big => u16::from_be_bytes(word),
        }
    }

    /// A u32 in this database's byte order. Callers have already bounds checked the slice.
    fn u32(self, bytes: &[u8], offset: usize) -> u32 {
        let word = bytes[offset..offset + 4]
            .try_into()
            .expect("four bytes were sliced");
        match self {
            Order::Little => u32::from_le_bytes(word),
            Order::Big => u32::from_be_bytes(word),
        }
    }
}

/// An open package store, held in memory like the sqlite and ndb ones.
#[derive(Debug)]
pub struct Database {
    bytes: Vec<u8>,
    order: Order,
    page_size: usize,
    last_page: u32,
    /// Where the store was read from, so a record skipped out of it can name the file.
    source: String,
}

impl Database {
    /// Open a package store and validate the meta page it is described by.
    pub fn open(path: &Path) -> FeludaResult<Self> {
        let bytes = std::fs::read(path).map_err(|error| {
            backend_error(format!("Failed to read {}: {error}", path.display()))
        })?;
        Self::from_bytes(bytes, &path.display().to_string())
    }

    fn from_bytes(bytes: Vec<u8>, source: &str) -> FeludaResult<Self> {
        if bytes.len() < SMALLEST_PAGE_SIZE {
            return Err(not_a_hash_database(source));
        }

        let order = match Order::Little.u32(&bytes, OFFSET_MAGIC) {
            MAGIC => Order::Little,
            MAGIC_SWAPPED => Order::Big,
            _ => return Err(not_a_hash_database(source)),
        };

        let version = order.u32(&bytes, OFFSET_VERSION);
        if !(MIN_VERSION..=MAX_VERSION).contains(&version) {
            return Err(backend_error(format!(
                "{source} is Berkeley DB hash version {version}, and only versions {MIN_VERSION} to \
                 {MAX_VERSION} are understood."
            )));
        }

        // A checksum or an initialization vector sits between the page header and the items, and
        // every offset below would be wrong by its size. rpm opens its databases with neither.
        if bytes[OFFSET_ENCRYPT_ALG] != 0 || bytes[OFFSET_METAFLAGS] != 0 {
            return Err(backend_error(format!(
                "{source} is an encrypted or checksummed Berkeley DB, which rpm does not write."
            )));
        }

        let page_size = order.u32(&bytes, OFFSET_PAGE_SIZE) as usize;
        if page_size < SMALLEST_PAGE_SIZE || !page_size.is_power_of_two() {
            return Err(backend_error(format!(
                "{source} declares a {page_size} byte page, which is not a Berkeley DB page size."
            )));
        }

        // A page size larger than the file is not checked here: the last page count below reads
        // back as at least one page, so the same file gets the more accurate message.
        let last_page = order.u32(&bytes, OFFSET_LAST_PAGE);
        if (last_page as u64 + 1) * page_size as u64 > bytes.len() as u64 {
            return Err(backend_error(format!(
                "{source} declares {} pages, which do not fit in the file.",
                last_page as u64 + 1
            )));
        }

        Ok(Self {
            bytes,
            order,
            page_size,
            last_page,
            source: source.to_string(),
        })
    }

    /// Every header blob in the store, in header number order, which is the order rpm allocated
    /// them in. `rpm -qa` lists a Berkeley DB store in hash order instead, so the two agree on the
    /// set and not on the sequence.
    pub fn blobs(&self) -> FeludaResult<Vec<Vec<u8>>> {
        Ok(self
            .records()?
            .into_iter()
            .filter(|(number, _)| *number != INSTANCE_KEY)
            .map(|(_, blob)| blob)
            .collect())
    }

    /// Every record the store holds, in header number order, the instance counter included.
    ///
    /// A record whose framing does not check out is skipped rather than failing the scan: one
    /// damaged package is not a reason to report a whole image as unreadable. Both a skipped record
    /// and a repeated header number are reported on stderr once, with the file they came from:
    /// `log(LogLevel::Warn, …)` only prints under `--debug`, and a scan that quietly returns one
    /// package fewer looks exactly like an image that has one package fewer.
    fn records(&self) -> FeludaResult<Vec<(u32, Vec<u8>)>> {
        let mut records = Vec::new();
        let mut skipped = 0;

        // shortcut: every hash page in the file is read rather than the bucket table being walked,
        // which a full dump has no use for. libdb retypes a page when it frees it, so a page left
        // over from a deleted package is not mistaken for one; upgrade to a bucket walk if a
        // database ever turns up with records `rpm -qa` does not list.
        for number in 1..=self.last_page {
            let page = self.page(number);
            if !matches!(page[OFFSET_TYPE], P_HASH | P_HASH_UNSORTED) {
                continue;
            }

            let entries = self.order.u16(page, OFFSET_ENTRIES) as usize;
            if !entries.is_multiple_of(2) || PAGE_HEADER + entries * 2 > self.page_size {
                return Err(backend_error(format!(
                    "{} has a malformed hash page at page {number} and is corrupt.",
                    self.source
                )));
            }

            for index in 0..entries / 2 {
                match self.record(page, index) {
                    Some(record) => records.push(record),
                    None => {
                        skipped += 1;
                        log(
                            LogLevel::Warn,
                            &format!("Skipped an unreadable rpm header record on page {number}"),
                        );
                    }
                }
            }
        }

        records.sort_by_key(|(number, _)| *number);

        // A page left over from an interrupted `--rebuilddb` or an abandoned bucket split can keep
        // its type and hand back a header number the live page also carries. One record per number
        // is kept: the sort above is stable, so that is the one on the lowest page number. Nothing
        // ties page order to which copy is live, so when the two blobs differ the one kept is
        // arbitrary, and the stderr line below is what says the sweep can no longer be trusted.
        // This guards only a repeated number: a stale page whose numbers are unique is still read
        // as installed.
        let found = records.len();
        records.dedup_by_key(|(number, _)| *number);
        let repeated = found - records.len();

        if skipped > 0 {
            eprintln!(
                "⚠️ Skipped {skipped} unreadable rpm headers in {}",
                self.source
            );
        }
        if repeated > 0 {
            eprintln!(
                "⚠️ Dropped {repeated} repeated rpm header records in {}",
                self.source
            );
        }

        Ok(records)
    }

    /// The pair at `index` on `page`, or `None` when it is not shaped like a record.
    fn record(&self, page: &[u8], index: usize) -> Option<(u32, Vec<u8>)> {
        // Each pair's items are packed against the end of the page in slot order, so the key ends
        // where the previous item starts and the value ends where the key starts.
        let slot = PAGE_HEADER + index * 4;
        let key_offset = self.order.u16(page, slot) as usize;
        let value_offset = self.order.u16(page, slot + 2) as usize;
        let key_end = if index == 0 {
            self.page_size
        } else {
            self.order.u16(page, slot - 2) as usize
        };
        let key_length = key_end.checked_sub(key_offset)?;
        let value_length = key_offset.checked_sub(value_offset)?;

        let key = item(page, key_offset, key_length)?;
        if key.len() != 4 {
            return None;
        }
        let number = self.order.u32(key, 0);

        let blob = match *page.get(value_offset)? {
            H_KEYDATA => item(page, value_offset, value_length)?.to_vec(),
            H_OFFPAGE if value_length == HOFFPAGE_SIZE => {
                let page_number = self.order.u32(page, value_offset + HOFFPAGE_PAGE);
                let length = self.order.u32(page, value_offset + HOFFPAGE_LENGTH) as usize;
                self.overflow(page_number, length)?
            }
            // rpm stores one value per key, so a duplicate set is not something to read here.
            _ => return None,
        };

        Some((number, blob))
    }

    /// Read `length` bytes from the overflow chain that begins at `number`.
    ///
    /// Each overflow page says how much of the value it holds and where the chain goes next, and
    /// every page is checked before it is trusted: a chain that leaves the file, walks into a page
    /// that is not an overflow page, or claims more bytes than the value has, is a damaged record.
    fn overflow(&self, mut number: u32, length: usize) -> Option<Vec<u8>> {
        // The value length comes from the file, so the file is what bounds it.
        if length == 0 || length > self.bytes.len() {
            return None;
        }

        let mut blob = Vec::with_capacity(length);
        while blob.len() < length {
            if number == 0 || number > self.last_page {
                return None;
            }

            let page = self.page(number);
            if page[OFFSET_TYPE] != P_OVERFLOW {
                return None;
            }

            let length_on_page = self.order.u16(page, OFFSET_HIGH_FREE) as usize;
            if length_on_page == 0
                || length_on_page > length - blob.len()
                || PAGE_HEADER + length_on_page > self.page_size
            {
                return None;
            }

            blob.extend_from_slice(&page[PAGE_HEADER..PAGE_HEADER + length_on_page]);
            number = self.order.u32(page, OFFSET_NEXT_PAGE);
        }

        Some(blob)
    }

    /// One page of the file. Callers have checked `number` against the meta page's last page.
    fn page(&self, number: u32) -> &[u8] {
        let start = number as usize * self.page_size;
        &self.bytes[start..start + self.page_size]
    }
}

/// The payload of an on page item, or `None` when its type byte says it does not hold one.
fn item(page: &[u8], offset: usize, length: usize) -> Option<&[u8]> {
    if length < 1 || *page.get(offset)? != H_KEYDATA {
        return None;
    }
    page.get(offset + 1..offset + length)
}

/// Report a file that is not a Berkeley DB hash database at all.
fn not_a_hash_database(source: &str) -> crate::debug::FeludaError {
    backend_error(format!(
        "{source} is not an rpm Berkeley DB package store. The rpm Berkeley DB backend expects a \
         hash database at this path."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page type libdb gives a page it has freed, from `db_page.h`.
    const P_INVALID: u8 = 0;

    /// The head of the freed page list, in the meta page.
    const OFFSET_FREE: usize = 28;

    /// The checked in fixture: five `rockylinux:8` packages and the image's signing key, in the
    /// store rpm 4.14.3 wrote for them inside `rockylinux:8`:
    ///
    /// ```sh
    /// rpm --initdb --dbpath /work/db
    /// rpm --dbpath /work/db --justdb --nodeps --noscripts -i gzip file file-libs which \
    ///     python3-pip-wheel platform-python-setuptools   # the downloaded .rpm files
    /// rpm --dbpath /work/db --import /etc/pki/rpm-gpg/RPM-GPG-KEY-rockyofficial
    /// rpm --dbpath /work/db --justdb --nodeps --noscripts -i diffutils
    /// rpm --dbpath /work/db --justdb -e file diffutils
    /// ```
    ///
    /// Eight headers were written and two erased. `--nodeps` is there only because the fresh
    /// database knows none of `glibc`. diffutils goes in last so that erasing it frees the highest
    /// header number. What the file carries is what only real transactions produce: packed items,
    /// sorted `P_HASH` pages, and the freed pages libdb chained onto its free list when it released
    /// them. No script touches it.
    fn fixture() -> Database {
        Database::open(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/rpm/Packages"
        )))
        .expect("fixture should open")
    }

    #[test]
    fn test_a_database_swapped_to_big_endian_reads_the_same_records() {
        // RHEL 8 runs on s390x, which is big endian, and libdb writes every metadata field of the
        // database in the machine's own byte order. What this proves is bounded: the swapper below
        // agrees with the reader, so a field one of them gets wrong and the other mirrors stays
        // wrong in both. It catches a field the reader forgets to look at, not a misreading of
        // libdb's layout. A two package store written by rpm under `--platform linux/s390x` would
        // prove the second half; no such image is available to build it from.
        let little = fixture();
        let big = Database::from_bytes(big_endian(&little.bytes), "test").unwrap();
        assert_eq!(big.order, Order::Big);
        assert_eq!(big.blobs().unwrap(), little.blobs().unwrap());
    }

    /// Swap a little endian store into the shape a big endian machine would have written: the meta
    /// page's words, the page header fields, the slot array, the off page pointers and the header
    /// number keys. Header blobs are big endian already and are left alone.
    fn big_endian(bytes: &[u8]) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        let page_size = little_u32(&bytes, OFFSET_PAGE_SIZE) as usize;
        let last_page = little_u32(&bytes, OFFSET_LAST_PAGE);

        for at in [
            OFFSET_MAGIC,
            OFFSET_VERSION,
            OFFSET_PAGE_SIZE,
            OFFSET_LAST_PAGE,
        ] {
            swap_word(&mut bytes, at);
        }

        for number in 1..=last_page as usize {
            let page = number * page_size;
            let entries =
                u16::from_le_bytes(bytes[page + 20..page + 22].try_into().expect("two bytes"))
                    as usize;
            swap_word(&mut bytes, page + OFFSET_NEXT_PAGE);
            swap_half(&mut bytes, page + OFFSET_ENTRIES);
            swap_half(&mut bytes, page + OFFSET_HIGH_FREE);
            if !matches!(bytes[page + OFFSET_TYPE], P_HASH | P_HASH_UNSORTED) {
                continue;
            }

            for index in 0..entries {
                swap_half(&mut bytes, page + PAGE_HEADER + index * 2);
            }

            // The slot array is big endian now, as the reader will find it.
            let slots: Vec<usize> = (0..entries)
                .map(|index| {
                    let at = page + PAGE_HEADER + index * 2;
                    u16::from_be_bytes(bytes[at..at + 2].try_into().expect("two bytes")) as usize
                })
                .collect();
            for (index, start) in slots.iter().enumerate() {
                match bytes[page + start] {
                    // A key is a header number, which rpm reads in the database's byte order.
                    H_KEYDATA if index.is_multiple_of(2) => swap_word(&mut bytes, page + start + 1),
                    H_OFFPAGE => {
                        swap_word(&mut bytes, page + start + HOFFPAGE_PAGE);
                        swap_word(&mut bytes, page + start + HOFFPAGE_LENGTH);
                    }
                    _ => {}
                }
            }
        }

        bytes
    }

    fn little_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn swap_word(bytes: &mut [u8], at: usize) {
        bytes[at..at + 4].reverse();
    }

    fn swap_half(bytes: &mut [u8], at: usize) {
        bytes[at..at + 2].reverse();
    }

    #[test]
    fn test_reads_every_record_of_the_fixture() {
        // Five packages and the signing key, with the instance counter skipped.
        let blobs = fixture().blobs().unwrap();
        assert_eq!(blobs.len(), 6);

        // Every blob is self describing, so the header's own declared sizes have to account for
        // exactly what the reader returned: no page framing leaked in, and no overflow page was
        // dropped or doubled.
        for blob in &blobs {
            let nindex = u32::from_be_bytes(blob[0..4].try_into().unwrap()) as usize;
            let hsize = u32::from_be_bytes(blob[4..8].try_into().unwrap()) as usize;
            assert_eq!(blob.len(), 8 + nindex * 16 + hsize, "blob does not add up");
        }
    }

    #[test]
    fn test_key_zero_is_the_instance_counter_not_a_header() {
        // rpm keeps the last header number it allocated under key 0. Eight headers were written to
        // this store and two were erased, the last one among them, so the counter still says 8
        // while the highest number left in the file is 7: key 0 is a counter, not a header, and a
        // reader that handed it on would report a four byte "header" for a package that is not
        // installed.
        let records = fixture().records().unwrap();
        assert_eq!(records[0].0, INSTANCE_KEY);
        let instance: [u8; 4] = records[0].1.as_slice().try_into().expect("four bytes");
        assert_eq!(u32::from_le_bytes(instance), 8);
        assert_eq!(records.last().unwrap().0, 7);
        assert!(!fixture()
            .blobs()
            .unwrap()
            .iter()
            .any(|blob| blob.len() == 4));
    }

    #[test]
    fn test_the_fixture_was_written_by_rpm_not_by_this_module() {
        // A store this module could have produced would prove less, so read back what only rpm
        // writes: two hash pages, a file ending exactly at the meta page's declared last page, and
        // pages the two `rpm -e` runs freed. The freed pages are the point of the fixture: the page
        // sweep trusts the type byte, so it has to be reading a store where libdb really retyped a
        // page it released.
        let database = fixture();
        assert_eq!(database.order, Order::Little);
        assert_eq!(database.page_size, 4096);
        assert_eq!(
            (database.last_page as usize + 1) * database.page_size,
            database.bytes.len()
        );

        let types: Vec<u8> = (1..=database.last_page)
            .map(|number| database.page(number)[OFFSET_TYPE])
            .collect();
        let hash_pages = types
            .iter()
            .filter(|kind| matches!(**kind, P_HASH | P_HASH_UNSORTED))
            .count();
        assert_eq!(hash_pages, 2);
        // The freed pages have to be on libdb's free list, not merely typed `P_INVALID`: the meta
        // page chains them through each page's next pointer, which is bookkeeping a script would
        // have to invent. Every page on the chain is freed, and every freed page is on it.
        let freed: Vec<u32> = (1..=database.last_page)
            .filter(|number| database.page(*number)[OFFSET_TYPE] == P_INVALID)
            .collect();
        assert!(freed.len() >= 2, "the two erases should have freed pages");
        let mut chained = Vec::new();
        let mut next = database.order.u32(&database.bytes, OFFSET_FREE);
        while next != 0 {
            assert!(
                database.page(next)[OFFSET_TYPE] == P_INVALID,
                "page {next} is on the free list and not freed"
            );
            assert!(
                !chained.contains(&next),
                "the free list loops at page {next}"
            );
            chained.push(next);
            next = database.order.u32(database.page(next), OFFSET_NEXT_PAGE);
        }
        let mut chained_sorted = chained.clone();
        chained_sorted.sort_unstable();
        let mut freed_sorted = freed.clone();
        freed_sorted.sort_unstable();
        assert_eq!(chained_sorted, freed_sorted);
        assert!(
            types.iter().filter(|kind| **kind == P_OVERFLOW).count() > 0,
            "the headers should live on overflow chains"
        );
    }

    #[test]
    fn test_a_repeated_header_number_is_kept_once() {
        // A page released by a delete keeps its type if a `--rebuilddb` was interrupted or a bucket
        // split was abandoned, and the same header number then comes back from two pages. Copying a
        // live hash page into a freed one is exactly what the sweep would see; the reader must not
        // report the package twice.
        let mut database = fixture();
        let pristine = fixture().records().unwrap();
        let hash_page = (1..=database.last_page)
            .find(|number| database.page(*number)[OFFSET_TYPE] == P_HASH)
            .expect("the fixture has hash pages");
        let freed_page = (1..=database.last_page)
            .find(|number| database.page(*number)[OFFSET_TYPE] == P_INVALID)
            .expect("the fixture has freed pages");
        let page_size = database.page_size;
        let (from, to) = (
            hash_page as usize * page_size,
            freed_page as usize * page_size,
        );
        let copy = database.bytes[from..from + page_size].to_vec();
        database.bytes[to..to + page_size].copy_from_slice(&copy);

        // Seven records and six blobs whether or not a page was read twice, and the copy carries
        // the same blob: the reader keeps the record, not the page number.
        assert_eq!(database.records().unwrap().len(), pristine.len());
        assert_eq!(database.records().unwrap().len(), 7);
        assert_eq!(database.blobs().unwrap().len(), 6);
        assert!(database.records().unwrap().iter().all(|(number, blob)| {
            pristine.iter().any(|(pristine_number, pristine_blob)| {
                pristine_number == number && pristine_blob == blob
            })
        }));
    }

    #[test]
    fn test_rejects_a_file_that_is_not_a_hash_database() {
        let error = Database::from_bytes(vec![0u8; 4096], "test").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("not an rpm Berkeley DB package store"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_a_truncated_meta_page() {
        let error = Database::from_bytes(MAGIC.to_le_bytes().to_vec(), "test").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("not an rpm Berkeley DB package store"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_an_unknown_hash_version() {
        let mut bytes = fixture().bytes.clone();
        bytes[OFFSET_VERSION] = 6;
        let error = Database::from_bytes(bytes, "test").unwrap_err();
        assert!(
            error.to_string().contains("hash version 6"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_a_page_size_that_is_not_a_power_of_two() {
        let mut bytes = fixture().bytes.clone();
        bytes[OFFSET_PAGE_SIZE] = 0x10;
        bytes[OFFSET_PAGE_SIZE + 1] = 0x00;
        let error = Database::from_bytes(bytes, "test").unwrap_err();
        assert!(
            error.to_string().contains("not a Berkeley DB page size"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_a_last_page_past_the_end_of_the_file() {
        let mut bytes = fixture().bytes.clone();
        bytes[OFFSET_LAST_PAGE] = 0xff;
        bytes[OFFSET_LAST_PAGE + 1] = 0xff;
        let error = Database::from_bytes(bytes, "test").unwrap_err();
        assert!(
            error.to_string().contains("do not fit in the file"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_a_checksummed_database() {
        // Every offset below the page header depends on there being no checksum, so a database
        // carrying one has to be refused rather than misread into an empty tree.
        let mut bytes = fixture().bytes.clone();
        bytes[OFFSET_METAFLAGS] = 0x01;
        let error = Database::from_bytes(bytes, "test").unwrap_err();
        assert!(
            error.to_string().contains("encrypted or checksummed"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_a_broken_overflow_chain_costs_one_record() {
        let mut database = fixture();
        let intact = database.blobs().unwrap().len();

        // A record whose value starts on page 3 keeps the rest of the record there; retype the
        // page and that record is damaged, the other five are not.
        let first_overflow = (1..=database.last_page)
            .find(|number| database.page(*number)[OFFSET_TYPE] == P_OVERFLOW)
            .expect("the fixture keeps its headers on overflow pages");
        let start = first_overflow as usize * database.page_size;
        database.bytes[start + OFFSET_TYPE] = 0;

        assert_eq!(database.blobs().unwrap().len(), intact - 1);
    }

    #[test]
    fn test_a_malformed_hash_page_is_corruption() {
        let mut database = fixture();
        let hash_page = (1..=database.last_page)
            .find(|number| database.page(*number)[OFFSET_TYPE] == P_HASH)
            .expect("the fixture has hash pages");
        let start = hash_page as usize * database.page_size;
        // An odd item count is not a set of key and value pairs.
        database.bytes[start + OFFSET_ENTRIES] = 3;

        let error = database.blobs().unwrap_err();
        assert!(
            error.to_string().contains("malformed hash page"),
            "unexpected error: {error}"
        );
    }
}
