//! A read only SQLite reader, big enough for the one table rpm keeps its headers in.
//!
//! rpm's sqlite backend is a stock SQLite database whose `Packages` table is
//! `(hnum INTEGER PRIMARY KEY, blob BLOB NOT NULL)`. Every other table in the file is an index rpm
//! rebuilds from those blobs, so reading the database means reading one column of one table: no
//! queries, no writing, no journalling, no locking.
//!
//! That is small enough to do here rather than link a C SQLite, which would put a build toolchain
//! in front of every release target for a single scan source. The file format is documented and
//! frozen for backward compatibility, so what this reads will keep parsing.
//!
//! What it implements: the file header, table b-tree interior and leaf pages, record decoding far
//! enough to pull one column, and overflow chains. Overflow is the common path rather than an edge
//! case, since an rpm header is routinely larger than a page.

use std::path::Path;

use crate::debug::FeludaResult;

use super::backend_error;

/// The magic every SQLite file begins with.
const MAGIC: &[u8] = b"SQLite format 3\0";

/// Interior and leaf page types of a *table* b-tree. Index b-trees (0x02, 0x0a) are the file's
/// other trees and are never walked: rpm's indexes are derivable from the headers.
const INTERIOR_TABLE: u8 = 0x05;
const LEAF_TABLE: u8 = 0x0d;

/// The schema table always lives on page 1, and its rows describe every other table.
const SCHEMA_PAGE: u32 = 1;

/// Columns of `sqlite_schema`.
const SCHEMA_TYPE: usize = 0;
const SCHEMA_NAME: usize = 1;
const SCHEMA_ROOT_PAGE: usize = 3;

/// An open database, held in memory.
///
/// An rpm database runs to a few megabytes, the same order as the dpkg status file the deb
/// cataloger already reads whole, so paging from disk would buy nothing.
pub struct Database {
    bytes: Vec<u8>,
    page_size: usize,
    /// Bytes reserved at the end of every page. Zero in practice, but it changes the usable page
    /// size that every overflow calculation is in terms of.
    reserved: usize,
}

/// One decoded column value. Only the two shapes the schema and the `Packages` table use are
/// represented; everything else decodes to [`Value::Other`] with its bytes skipped.
enum Value {
    Integer(i64),
    Bytes(Vec<u8>),
    Other,
}

impl Value {
    fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(value) => Some(*value),
            _ => None,
        }
    }

    fn as_text(&self) -> Option<String> {
        match self {
            Value::Bytes(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        }
    }
}

impl Database {
    /// Open a database file and validate enough of its header to know the geometry.
    pub fn open(path: &Path) -> FeludaResult<Self> {
        let bytes = std::fs::read(path).map_err(|error| {
            backend_error(format!("Failed to read {}: {error}", path.display()))
        })?;
        Self::from_bytes(bytes, &path.display().to_string())
    }

    fn from_bytes(bytes: Vec<u8>, source: &str) -> FeludaResult<Self> {
        if bytes.len() < 100 || !bytes.starts_with(MAGIC) {
            return Err(backend_error(format!(
                "{source} is not a SQLite database. The rpm sqlite backend expects one at this path."
            )));
        }

        // A page size of 1 means 65536, which does not fit the u16 the header stores it in.
        let page_size = match u16::from_be_bytes([bytes[16], bytes[17]]) {
            1 => 65536,
            size => size as usize,
        };
        if !page_size.is_power_of_two() || page_size < 512 {
            return Err(backend_error(format!(
                "{source} declares an invalid page size of {page_size}."
            )));
        }

        let reserved = bytes[20] as usize;
        if reserved >= page_size {
            return Err(backend_error(format!(
                "{source} reserves {reserved} bytes of a {page_size} byte page."
            )));
        }

        Ok(Self {
            bytes,
            page_size,
            reserved,
        })
    }

    /// Every value of one column of one table, in b-tree order.
    ///
    /// Rows whose column decodes to something other than a blob or text are skipped rather than
    /// failing the scan: one unreadable row should not cost the whole database.
    pub fn column_values(&self, table: &str, column: usize) -> FeludaResult<Vec<Vec<u8>>> {
        let root = self.root_page(table)?;
        let mut payloads = Vec::new();
        self.walk_table(root, &mut payloads)?;

        Ok(payloads
            .into_iter()
            .filter_map(|payload| match self.column(&payload, column) {
                Some(Value::Bytes(bytes)) => Some(bytes),
                _ => None,
            })
            .collect())
    }

    /// Find a table's root page in `sqlite_schema`.
    fn root_page(&self, table: &str) -> FeludaResult<u32> {
        let mut rows = Vec::new();
        self.walk_table(SCHEMA_PAGE, &mut rows)?;

        for row in rows {
            let is_table = self
                .column(&row, SCHEMA_TYPE)
                .and_then(|value| value.as_text())
                .is_some_and(|kind| kind == "table");
            let matches = self
                .column(&row, SCHEMA_NAME)
                .and_then(|value| value.as_text())
                .is_some_and(|name| name == table);
            if !is_table || !matches {
                continue;
            }
            if let Some(page) = self
                .column(&row, SCHEMA_ROOT_PAGE)
                .and_then(|value| value.as_integer())
                .filter(|page| *page > 0)
            {
                return Ok(page as u32);
            }
        }

        Err(backend_error(format!(
            "No '{table}' table in the rpm database. The file is a SQLite database but not an rpm one."
        )))
    }

    /// Collect the payload of every leaf cell under `page`.
    fn walk_table(&self, page: u32, payloads: &mut Vec<Vec<u8>>) -> FeludaResult<()> {
        // A corrupt file can point a page at an ancestor. Bounding the walk by the page count keeps
        // that a reported error rather than a hang.
        let mut stack = vec![page];
        let mut visited = std::collections::HashSet::new();

        while let Some(page) = stack.pop() {
            if !visited.insert(page) {
                return Err(backend_error(
                    "The rpm database has a cyclic page reference and is corrupt.".to_string(),
                ));
            }

            let body = self.page(page)?;
            // Page 1 carries the 100 byte file header before its b-tree node.
            let header_start = if page == SCHEMA_PAGE { 100 } else { 0 };
            let header = &body[header_start..];
            if header.is_empty() {
                continue;
            }

            let kind = header[0];
            let cell_count = read_u16(header, 3)? as usize;
            // Interior nodes carry a rightmost child pointer after the eight byte header.
            let pointer_start = header_start + if kind == INTERIOR_TABLE { 12 } else { 8 };

            match kind {
                LEAF_TABLE => {
                    for index in 0..cell_count {
                        let offset = read_u16(body, pointer_start + index * 2)? as usize;
                        payloads.push(self.leaf_payload(body, offset)?);
                    }
                }
                INTERIOR_TABLE => {
                    for index in 0..cell_count {
                        let offset = read_u16(body, pointer_start + index * 2)? as usize;
                        stack.push(read_u32(body, offset)?);
                    }
                    stack.push(read_u32(header, 8)?);
                }
                // An empty database has a page 1 that is not yet a b-tree node.
                _ => continue,
            }
        }

        Ok(())
    }

    /// Read one table leaf cell, following its overflow chain when the payload does not fit.
    fn leaf_payload(&self, page: &[u8], offset: usize) -> FeludaResult<Vec<u8>> {
        let (payload_size, read) = varint(page, offset)?;
        // The rowid follows and is only identity, which `INTEGER PRIMARY KEY` columns read back as
        // NULL in the record itself. Nothing here needs it.
        let (_, rowid_read) = varint(page, offset + read)?;
        let start = offset + read + rowid_read;
        let payload_size = payload_size as usize;

        let usable = self.usable_size();
        // The spill thresholds the format defines for a table leaf.
        let max_local = usable - 35;
        if payload_size <= max_local {
            return slice(page, start, payload_size).map(<[u8]>::to_vec);
        }

        let min_local = ((usable - 12) * 32 / 255) - 23;
        let spill = min_local + (payload_size - min_local) % (usable - 4);
        let local = if spill <= max_local { spill } else { min_local };

        let mut payload = slice(page, start, local)?.to_vec();
        let mut next = read_u32(page, start + local)?;

        // Each overflow page is a four byte pointer to the next, then data.
        let per_page = usable - 4;
        let mut seen = std::collections::HashSet::new();
        while next != 0 && payload.len() < payload_size {
            if !seen.insert(next) {
                return Err(backend_error(
                    "The rpm database has a cyclic overflow chain and is corrupt.".to_string(),
                ));
            }
            let page = self.page(next)?;
            let take = per_page.min(payload_size - payload.len());
            payload.extend_from_slice(slice(page, 4, take)?);
            next = read_u32(page, 0)?;
        }

        if payload.len() != payload_size {
            return Err(backend_error(
                "An rpm database row ends before its declared length and is corrupt.".to_string(),
            ));
        }
        Ok(payload)
    }

    /// Decode one column out of a record payload.
    ///
    /// A record is a header of serial types followed by the values they describe, so reaching
    /// column `n` means walking the first `n` serial types to know how far in its value starts.
    fn column(&self, payload: &[u8], column: usize) -> Option<Value> {
        let (header_size, read) = varint(payload, 0).ok()?;
        let header_size = header_size as usize;

        let mut cursor = read;
        let mut body = header_size;
        for index in 0..=column {
            if cursor >= header_size {
                return None;
            }
            let (serial, read) = varint(payload, cursor).ok()?;
            cursor += read;

            let size = serial_size(serial);
            if index == column {
                return Some(decode(payload, body, serial, size));
            }
            body += size;
        }
        None
    }

    /// The bytes of one page. Pages are numbered from 1.
    fn page(&self, page: u32) -> FeludaResult<&[u8]> {
        let start = (page as usize)
            .checked_sub(1)
            .and_then(|index| index.checked_mul(self.page_size))
            .ok_or_else(|| backend_error("The rpm database references page 0.".to_string()))?;
        slice(&self.bytes, start, self.page_size)
    }

    /// Page size minus the reserved tail, which every payload calculation is in terms of.
    fn usable_size(&self) -> usize {
        self.page_size - self.reserved
    }
}

/// The byte length a serial type occupies in the record body.
fn serial_size(serial: u64) -> usize {
    match serial {
        0 | 8 | 9 => 0,
        1..=4 => serial as usize,
        5 => 6,
        6 | 7 => 8,
        // Blobs are even from 12, text odd from 13, both encoding their own length.
        _ => (serial as usize - 12) / 2,
    }
}

/// Turn one serial type and its bytes into a value.
fn decode(payload: &[u8], offset: usize, serial: u64, size: usize) -> Value {
    match serial {
        // The constant integers, which occupy no bytes at all.
        8 => Value::Integer(0),
        9 => Value::Integer(1),
        1..=6 => match slice(payload, offset, size) {
            Ok(bytes) => {
                // Big endian and two's complement, so sign extend from the top bit.
                let mut value = if bytes.first().is_some_and(|byte| byte & 0x80 != 0) {
                    -1i64
                } else {
                    0
                };
                for byte in bytes {
                    value = (value << 8) | *byte as i64;
                }
                Value::Integer(value)
            }
            Err(_) => Value::Other,
        },
        // Blob and text alike: the caller knows which it asked for.
        serial if serial >= 12 => match slice(payload, offset, size) {
            Ok(bytes) => Value::Bytes(bytes.to_vec()),
            Err(_) => Value::Other,
        },
        _ => Value::Other,
    }
}

/// Read a SQLite variable length integer, returning it and how many bytes it took.
///
/// Seven bits per byte, high bit set to continue, up to nine bytes. The ninth is special: all eight
/// of its bits count, which is what lets the encoding reach a full 64 bits.
fn varint(bytes: &[u8], offset: usize) -> FeludaResult<(u64, usize)> {
    let mut value: u64 = 0;
    for index in 0..9 {
        let byte = *bytes.get(offset + index).ok_or_else(|| {
            backend_error("The rpm database ends inside a value and is corrupt.".to_string())
        })?;

        if index == 8 {
            return Ok(((value << 8) | byte as u64, 9));
        }
        value = (value << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Ok((value, index + 1));
        }
    }
    unreachable!("the nine byte case returns inside the loop")
}

fn read_u16(bytes: &[u8], offset: usize) -> FeludaResult<u16> {
    let bytes = slice(bytes, offset, 2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> FeludaResult<u32> {
    let bytes = slice(bytes, offset, 4)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Bounds checked slicing, so a truncated database reports as corrupt rather than panicking.
fn slice(bytes: &[u8], offset: usize, length: usize) -> FeludaResult<&[u8]> {
    bytes
        .get(offset..offset + length)
        .ok_or_else(|| backend_error("The rpm database is truncated.".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checked in fixture: seven packages taken from a `fedora:41` image.
    fn fixture() -> Database {
        Database::open(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/rpm/rpmdb.sqlite"
        )))
        .expect("fixture should open")
    }

    #[test]
    fn test_varint_single_byte() {
        assert_eq!(varint(&[0x00], 0).unwrap(), (0, 1));
        assert_eq!(varint(&[0x7f], 0).unwrap(), (127, 1));
    }

    #[test]
    fn test_varint_continues_while_the_high_bit_is_set() {
        // 0x81 0x00 is 1 << 7.
        assert_eq!(varint(&[0x81, 0x00], 0).unwrap(), (128, 2));
        assert_eq!(varint(&[0x82, 0x23], 0).unwrap(), (291, 2));
    }

    #[test]
    fn test_varint_ninth_byte_contributes_all_eight_bits() {
        let bytes = [0xff; 9];
        assert_eq!(varint(&bytes, 0).unwrap(), (u64::MAX, 9));
    }

    #[test]
    fn test_varint_past_the_end_is_an_error() {
        assert!(varint(&[0x81], 0).is_err());
    }

    #[test]
    fn test_serial_sizes() {
        assert_eq!(serial_size(0), 0);
        assert_eq!(serial_size(1), 1);
        assert_eq!(serial_size(4), 4);
        assert_eq!(serial_size(5), 6);
        assert_eq!(serial_size(6), 8);
        // Constant 0 and 1 occupy nothing.
        assert_eq!(serial_size(8), 0);
        assert_eq!(serial_size(9), 0);
        // Blob of 4, then text of 4.
        assert_eq!(serial_size(20), 4);
        assert_eq!(serial_size(21), 4);
    }

    #[test]
    fn test_negative_integers_sign_extend() {
        // A one byte -1.
        match decode(&[0xff], 0, 1, 1) {
            Value::Integer(value) => assert_eq!(value, -1),
            _ => panic!("expected an integer"),
        }
        match decode(&[0x00, 0x80], 0, 2, 2) {
            Value::Integer(value) => assert_eq!(value, 128),
            _ => panic!("expected an integer"),
        }
    }

    #[test]
    fn test_rejects_a_file_that_is_not_sqlite() {
        let error = Database::from_bytes(vec![0u8; 200], "test")
            .err()
            .expect("a file of zeroes is not a database");
        assert!(
            error.to_string().contains("not a SQLite database"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_rejects_a_truncated_file() {
        let error = Database::from_bytes(MAGIC.to_vec(), "test")
            .err()
            .expect("a header with no body is not a database");
        assert!(
            error.to_string().contains("not a SQLite database"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_reads_every_row_of_the_fixture() {
        let blobs = fixture().column_values("Packages", 1).unwrap();
        assert_eq!(blobs.len(), 7);
    }

    #[test]
    fn test_overflow_chains_reassemble_whole_payloads() {
        // Every blob is self describing, so a chain that stopped early or ran on would not add up:
        // the header's own declared sizes have to account for exactly what the reader returned.
        let blobs = fixture().column_values("Packages", 1).unwrap();
        for blob in &blobs {
            let nindex = u32::from_be_bytes(blob[0..4].try_into().unwrap()) as usize;
            let hsize = u32::from_be_bytes(blob[4..8].try_into().unwrap()) as usize;
            assert_eq!(blob.len(), 8 + nindex * 16 + hsize, "blob does not add up");
        }

        // And most of them spill past the 4096 byte page, so the chain walk is what produced them
        // rather than a lucky single-page read.
        let spilled = blobs.iter().filter(|blob| blob.len() > 4096).count();
        assert!(spilled >= 5, "only {spilled} blobs exercised overflow");
    }

    #[test]
    fn test_a_missing_table_is_an_error() {
        let error = fixture().column_values("Nonexistent", 0).unwrap_err();
        assert!(
            error.to_string().contains("No 'Nonexistent' table"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_finds_a_table_by_name() {
        // A real rpm database carries a dozen index tables beside this one, so the schema walk has
        // to match on the name rather than take the first table it meets.
        let database = fixture();
        assert!(database.root_page("Packages").is_ok());
        assert!(database.root_page("sqlite_sequence").is_ok());
    }
}
