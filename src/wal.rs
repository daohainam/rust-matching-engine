//! Memory-mapped binary Write-Ahead Log (WAL) for deterministic replay of
//! matching engine requests.
//!
//! Every mutating request (new order / cancel) is appended as a fixed-size
//! record directly into a memory-mapped file, so appending needs no
//! serialization allocation and records can be located by arithmetic.
//!
//! # File format (version 1)
//!
//! All integers are little-endian and every field is encoded explicitly, so
//! a log is portable across targets and never contains padding bytes.
//!
//! The file starts with a 32-byte header:
//!
//! | Offset | Size | Field                       |
//! |-------:|-----:|-----------------------------|
//! | 0      | 8    | magic `b"MEWALOG\0"`        |
//! | 8      | 4    | format version (`1`)        |
//! | 12     | 4    | record size in bytes (`40`) |
//! | 16     | 16   | reserved, zero              |
//!
//! followed by 40-byte records:
//!
//! | Offset | Size | Field                                          |
//! |-------:|-----:|------------------------------------------------|
//! | 0      | 1    | kind: `1` = new order, `2` = cancel            |
//! | 1      | 1    | side: `0` = buy, `1` = sell (`0` for cancels)  |
//! | 2      | 2    | reserved, zero                                 |
//! | 4      | 4    | price tick (`0` for cancels)                   |
//! | 8      | 8    | order id                                       |
//! | 16     | 8    | quantity (`0` for cancels)                     |
//! | 24     | 8    | caller-provided sequence number                |
//! | 32     | 4    | checksum of bytes `0..32`                      |
//! | 36     | 4    | reserved, zero                                 |
//!
//! # Crash safety
//!
//! The writer preallocates zero-filled space, and a crash can leave a torn
//! final record, so readers treat the first record that fails validation
//! (unknown kind or side, or a checksum mismatch) as the end of the log. The
//! checksum detects torn and zeroed records; it is not a cryptographic MAC.
//! [`WalWriter::open_append`] resumes an existing log after that point.
//! Appended records are durable once [`WalWriter::flush`] (or
//! [`WalWriter::finalize`]) returns.

use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use crate::pool::Side;

/// Size in bytes of the file header that precedes the records.
pub const WAL_HEADER_SIZE: usize = 32;
/// Size in bytes of one encoded record.
pub const WAL_RECORD_SIZE: usize = 40;
/// On-disk format version written by this module.
pub const WAL_FORMAT_VERSION: u32 = 1;

const WAL_MAGIC: [u8; 8] = *b"MEWALOG\0";
/// Bytes of an encoded record covered by its checksum.
const CHECKSUMMED_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordKind {
    /// A request to submit a new limit order.
    NewOrder = 1,
    /// A request to cancel an existing order.
    Cancel = 2,
}

/// One logged engine request. See the [module docs](self) for its encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalRecord {
    /// Which engine operation this record replays.
    pub kind: WalRecordKind,
    /// Side of a new order; [`Side::Buy`] for cancellation records.
    pub side: Side,
    /// Identifier supplied by the caller.
    pub order_id: u64,
    /// Dense integer price tick; zero for cancellation records.
    pub price_tick: u32,
    /// Requested order quantity; zero for cancellation records.
    pub qty: u64,
    /// Caller-provided sequence number for deterministic request ordering.
    pub sequence: u64,
}

/// Why an encoded record failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalDecodeError {
    /// The kind byte is not a known [`WalRecordKind`]; `0` marks
    /// preallocated space that was never written.
    InvalidKind(u8),
    /// The side byte is neither `0` (buy) nor `1` (sell).
    InvalidSide(u8),
    /// The stored checksum does not match the record contents, e.g. after a
    /// torn write.
    ChecksumMismatch,
}

impl std::fmt::Display for WalDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKind(kind) => write!(f, "invalid WAL record kind {kind}"),
            Self::InvalidSide(side) => write!(f, "invalid WAL record side {side}"),
            Self::ChecksumMismatch => f.write_str("WAL record checksum mismatch"),
        }
    }
}

impl std::error::Error for WalDecodeError {}

impl WalRecord {
    /// Describes a new-order request.
    pub fn new_order(order_id: u64, side: Side, price_tick: u32, qty: u64, sequence: u64) -> Self {
        Self {
            kind: WalRecordKind::NewOrder,
            side,
            order_id,
            price_tick,
            qty,
            sequence,
        }
    }

    /// Describes a cancellation request; fields not used by cancellation are zeroed.
    pub fn cancel(order_id: u64, sequence: u64) -> Self {
        Self {
            kind: WalRecordKind::Cancel,
            side: Side::Buy,
            order_id,
            price_tick: 0,
            qty: 0,
            sequence,
        }
    }

    /// Encodes the record into its fixed little-endian on-disk form.
    pub fn encode(&self) -> [u8; WAL_RECORD_SIZE] {
        let mut out = [0u8; WAL_RECORD_SIZE];
        out[0] = self.kind as u8;
        out[1] = match self.side {
            Side::Buy => 0,
            Side::Sell => 1,
        };
        out[4..8].copy_from_slice(&self.price_tick.to_le_bytes());
        out[8..16].copy_from_slice(&self.order_id.to_le_bytes());
        out[16..24].copy_from_slice(&self.qty.to_le_bytes());
        out[24..32].copy_from_slice(&self.sequence.to_le_bytes());
        let checksum = checksum(&out[..CHECKSUMMED_LEN]);
        out[32..36].copy_from_slice(&checksum.to_le_bytes());
        out
    }

    /// Decodes and validates one on-disk record.
    pub fn decode(bytes: &[u8; WAL_RECORD_SIZE]) -> Result<Self, WalDecodeError> {
        let kind = match bytes[0] {
            1 => WalRecordKind::NewOrder,
            2 => WalRecordKind::Cancel,
            other => return Err(WalDecodeError::InvalidKind(other)),
        };
        let side = match bytes[1] {
            0 => Side::Buy,
            1 => Side::Sell,
            other => return Err(WalDecodeError::InvalidSide(other)),
        };
        if read_u32(bytes, 32) != checksum(&bytes[..CHECKSUMMED_LEN]) {
            return Err(WalDecodeError::ChecksumMismatch);
        }
        Ok(Self {
            kind,
            side,
            price_tick: read_u32(bytes, 4),
            order_id: read_u64(bytes, 8),
            qty: read_u64(bytes, 16),
            sequence: read_u64(bytes, 24),
        })
    }
}

#[inline]
fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4-byte field"))
}

#[inline]
fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8-byte field"))
}

/// Folds the checksummed bytes, as little-endian words, through the
/// SplitMix64 finalizer. Cheap enough for the append path and sensitive to
/// every bit; not a cryptographic MAC.
fn checksum(body: &[u8]) -> u32 {
    let (words, _) = body.as_chunks::<8>();
    let folded = words
        .iter()
        .map(|word| u64::from_le_bytes(*word))
        .fold(0u64, |acc, word| mix64(acc ^ word));
    (folded ^ (folded >> 32)) as u32
}

#[inline]
fn mix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn encode_header() -> [u8; WAL_HEADER_SIZE] {
    let mut out = [0u8; WAL_HEADER_SIZE];
    out[..8].copy_from_slice(&WAL_MAGIC);
    out[8..12].copy_from_slice(&WAL_FORMAT_VERSION.to_le_bytes());
    out[12..16].copy_from_slice(&(WAL_RECORD_SIZE as u32).to_le_bytes());
    out
}

fn validate_header(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() < WAL_HEADER_SIZE {
        return Err(invalid_data("file is shorter than a WAL header".into()));
    }
    if bytes[..8] != WAL_MAGIC {
        return Err(invalid_data("not a WAL file (bad magic)".into()));
    }
    let version = read_u32(bytes, 8);
    if version != WAL_FORMAT_VERSION {
        return Err(invalid_data(format!(
            "unsupported WAL format version {version}"
        )));
    }
    let record_size = read_u32(bytes, 12);
    if record_size as usize != WAL_RECORD_SIZE {
        return Err(invalid_data(format!(
            "unexpected WAL record size {record_size}"
        )));
    }
    Ok(())
}

fn invalid_data(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Decodes the records that follow the header, stopping at the first
/// partial or invalid one, which marks the end of the log.
fn valid_records(body: &[u8]) -> impl Iterator<Item = WalRecord> + '_ {
    let (records, _partial_tail) = body.as_chunks::<WAL_RECORD_SIZE>();
    records
        .iter()
        .map_while(|record| WalRecord::decode(record).ok())
}

/// Maps `file` read-only, validates its header, and hands the bytes to `f`.
fn with_validated_map<R>(file: &File, f: impl FnOnce(&[u8]) -> R) -> io::Result<R> {
    let len = usize::try_from(file.metadata()?.len())
        .map_err(|_| invalid_data("WAL file too large to map".into()))?;
    if len < WAL_HEADER_SIZE {
        return Err(invalid_data("file is shorter than a WAL header".into()));
    }
    // SAFETY: the view is read-only and dropped before this function returns.
    // As with any file mapping, the file must not be truncated or rewritten
    // by another process while it is mapped.
    let map = unsafe { MmapOptions::new().len(len).map(file)? };
    validate_header(&map)?;
    Ok(f(&map))
}

/// Append-only memory-mapped WAL writer.
///
/// The backing file is pre-sized to `capacity_records` and grown (then
/// remapped) only when that space is exhausted. Appending within the mapped
/// capacity writes directly to the mapping; call [`flush`](Self::flush) or
/// [`finalize`](Self::finalize) when the caller needs to request persistence.
///
/// [`finalize`](Self::finalize) unmaps the file before truncating it (Windows
/// refuses to shrink a file with a mapped view open) and ends the writer's
/// life: afterwards [`append`](Self::append) and [`flush`](Self::flush) return
/// an error instead of re-mapping the file.
pub struct WalWriter {
    /// `None` once the writer has been finalized.
    mmap: Option<MmapMut>,
    file: std::fs::File,
    next_offset: usize,
    capacity_bytes: usize,
}

impl WalWriter {
    /// Creates or truncates a WAL file, writes its header, and preallocates
    /// room for at least `capacity_records` records.
    pub fn create<P: AsRef<Path>>(path: P, capacity_records: usize) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        let mut writer = Self::map(file, WAL_HEADER_SIZE, capacity_records)?;
        writer.mapping()[..WAL_HEADER_SIZE].copy_from_slice(&encode_header());
        // The header must be durable before any record can be.
        writer.flush()?;
        Ok(writer)
    }

    /// Opens an existing WAL to append after its last valid record, with
    /// room for at least `spare_records` more before the file must grow.
    ///
    /// Any bytes after the last valid record (a torn write or stale data)
    /// are discarded, so they can never be mistaken for records later.
    pub fn open_append<P: AsRef<Path>>(path: P, spare_records: usize) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let valid_len = with_validated_map(&file, |bytes| {
            WAL_HEADER_SIZE + valid_records(&bytes[WAL_HEADER_SIZE..]).count() * WAL_RECORD_SIZE
        })?;
        // Truncating (with no view mapped, which Windows requires) and then
        // re-extending in `map` zero-fills everything after the valid prefix.
        file.set_len(valid_len as u64)?;
        Self::map(file, valid_len, spare_records)
    }

    /// Sizes `file` to hold at least `spare_records` records after
    /// `next_offset` and maps it read-write.
    fn map(file: File, next_offset: usize, spare_records: usize) -> io::Result<Self> {
        let capacity_bytes = spare_records
            .max(1)
            .checked_mul(WAL_RECORD_SIZE)
            .and_then(|bytes| bytes.checked_add(next_offset))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "WAL capacity overflows usize")
            })?;
        file.set_len(capacity_bytes as u64)?;
        // SAFETY: the file is open read-write and the mapping never outlives
        // this writer. As with any shared file mapping, no other process may
        // truncate or rewrite the file while it is mapped.
        let mmap = unsafe { MmapOptions::new().len(capacity_bytes).map_mut(&file)? };
        Ok(Self {
            mmap: Some(mmap),
            mmap: Some(mmap),
            file,
            next_offset,
            capacity_bytes,
        })
    }

    #[inline]
    fn mapping(&mut self) -> &mut MmapMut {
        self.mmap
            .as_mut()
            .expect("WAL mapping is present until the writer is finalized")
    }

    /// Appends `record` at the next byte offset in the log.
    ///
    /// Usually this copies into already-mapped memory. If the record would
    /// exceed the current mapping, the file is grown and remapped first.
    ///
    /// Returns an error if the writer has already been
    /// [`finalize`](Self::finalize)d; the file is never re-mapped.
    pub fn append(&mut self, record: WalRecord) -> io::Result<()> {
        if self.mmap.is_none() {
            return Err(finalized_error());
        }
        if self.next_offset + WAL_RECORD_SIZE > self.capacity_bytes {
            self.grow()?;
        }
        let bytes = record.to_bytes();
        let mmap = self.mmap.as_mut().ok_or_else(finalized_error)?;
        mmap[self.next_offset..self.next_offset + WAL_RECORD_SIZE].copy_from_slice(&bytes);
        self.next_offset += WAL_RECORD_SIZE;
        Ok(())
    }

    fn grow(&mut self) -> io::Result<()> {
        // Doubling amortizes remapping: many appends can follow each growth.
        // The file always holds the header, so doubling leaves room for at
        // least one more record.
        let new_capacity = self.capacity_bytes.checked_mul(2).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "WAL capacity overflows usize")
        })?;
        // Extending a file while it is mapped is allowed on every supported
        // platform; only shrinking requires unmapping first.
        self.file.set_len(new_capacity as u64)?;
        let mmap = unsafe { MmapOptions::new().len(new_capacity).map_mut(&self.file)? };
        self.mmap = Some(mmap);
        self.capacity_bytes = new_capacity;
        Ok(())
    }

    /// Requests that dirty mapped pages be flushed to the backing file.
    ///
    /// Returns an error if the writer has already been
    /// [`finalize`](Self::finalize)d.
    pub fn flush(&self) -> io::Result<()> {
        self.mmap.as_ref().ok_or_else(finalized_error)?.flush()
    }

    /// Flushes the mapping, unmaps it, and truncates the file to the bytes
    /// actually written.
    ///
    /// This removes unused preallocated tail space, leaving only complete
    /// appended records. The mapping is dropped *before* the file is
    /// truncated because Windows refuses to shrink a file that still has a
    /// mapped view open. Safe to call multiple times: later calls only
    /// re-apply the truncation. If the flush fails the writer stays usable
    /// and `finalize` can be retried.
    ///
    /// After `finalize` the writer is closed: [`append`](Self::append) and
    /// [`flush`](Self::flush) return an error.
    pub fn finalize(&mut self) -> io::Result<()> {
        if let Some(mmap) = &self.mmap {
            mmap.flush()?;
        }
        self.mmap = None;
        self.file.set_len(self.next_offset as u64)
    }

    /// Returns the number of records in the log, including any that were
    /// already present when it was opened with [`open_append`](Self::open_append).
    #[inline(always)]
    pub fn len(&self) -> usize {
        (self.next_offset - WAL_HEADER_SIZE) / WAL_RECORD_SIZE
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.next_offset == WAL_HEADER_SIZE
    }
}

impl Drop for WalWriter {
    fn drop(&mut self) {
        // Best-effort finalization: callers can call `finalize` themselves
        // when they need to observe and handle any I/O error.
        let _ = self.finalize();
    }
}

fn finalized_error() -> io::Error {
    io::Error::other("WAL writer is already finalized")
}

/// Reads every valid record from a WAL file for replay.
///
/// Reading stops at the first partial or invalid record, which marks the end
/// of the log (for example the zero-filled preallocated tail left by a crash,
/// or a torn final write). A missing or unsupported header is reported as
/// [`io::ErrorKind::InvalidData`]. The file must not be written concurrently.
pub fn read_all<P: AsRef<Path>>(path: P) -> io::Result<Vec<WalRecord>> {
    let file = File::open(path)?;
    with_validated_map(&file, |bytes| {
        valid_records(&bytes[WAL_HEADER_SIZE..]).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_replay_roundtrip() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("wal_test_{}.bin", std::process::id()));
        {
            let mut writer = WalWriter::create(&path, 4).unwrap();
            writer
                .append(WalRecord::new_order(1, Side::Buy, 100, 10, 0))
                .unwrap();
            writer
                .append(WalRecord::new_order(2, Side::Sell, 101, 5, 1))
                .unwrap();
            writer.append(WalRecord::cancel(1, 2)).unwrap();
            // Force growth beyond the pre-sized capacity.
            for i in 3..10u64 {
                writer
                    .append(WalRecord::new_order(i, Side::Buy, 100, 1, i))
                    .unwrap();
            }
            writer.flush().unwrap();
        }

        let records = read_all(&path).unwrap();
        assert_eq!(records.len(), 10);
        assert_eq!(records[0].kind, WalRecordKind::NewOrder as u8);
        assert_eq!(records[0].order_id, 1);
        assert_eq!(records[0].price_tick, 100);
        assert_eq!(records[2].kind, WalRecordKind::Cancel as u8);
        assert_eq!(records[2].order_id, 1);
        assert_eq!(records[9].order_id, 9);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn finalize_truncates_file_and_closes_writer() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("wal_finalize_test_{}.bin", std::process::id()));

        let mut writer = WalWriter::create(&path, 16).unwrap();
        for i in 0..3u64 {
            writer
                .append(WalRecord::new_order(i, Side::Buy, 100, 1, i))
                .unwrap();
        }

        writer.finalize().unwrap();
        let expected = (writer.len() * WAL_RECORD_SIZE) as u64;
        assert_eq!(std::fs::metadata(&path).unwrap().len(), expected);

        // Idempotent, and the writer is closed afterwards.
        writer.finalize().unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), expected);
        assert!(writer.flush().is_err());
        assert!(writer.append(WalRecord::cancel(1, 3)).is_err());
        assert_eq!(writer.len(), 3);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), expected);

        drop(writer);
        let records = read_all(&path).unwrap();
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|r| r.kind == WalRecordKind::NewOrder as u8));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn finalize_on_empty_writer_leaves_empty_file() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("wal_empty_test_{}.bin", std::process::id()));

        let mut writer = WalWriter::create(&path, 4).unwrap();
        writer.finalize().unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        assert!(read_all(&path).unwrap().is_empty());

        drop(writer);
        let _ = std::fs::remove_file(&path);
    }
}
