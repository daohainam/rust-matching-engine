//! Memory-mapped binary Write-Ahead Log (WAL) for deterministic replay of
//! matching engine requests.
//!
//! Every mutating request (new order / cancel) is appended as a fixed-size
//! record directly into a memory-mapped file, so appending needs no
//! serialization allocation and records can be located by arithmetic.
//!
//! # File format (version 2)
//!
//! All integers are little-endian and every field is encoded explicitly, so
//! a log is portable across targets and never contains padding bytes.
//!
//! The file starts with a 32-byte header:
//!
//! | Offset | Size | Field                       |
//! |-------:|-----:|-----------------------------|
//! | 0      | 8    | magic `b"MEWALOG\0"`        |
//! | 8      | 4    | format version (`2`)        |
//! | 12     | 4    | record size in bytes (`40`) |
//! | 16     | 16   | reserved, zero              |
//!
//! followed by 40-byte records:
//!
//! | Offset | Size | Field                                          |
//! |-------:|-----:|------------------------------------------------|
//! | 0      | 1    | kind: `1` = new order, `2` = cancel, `3` = reduce |
//! | 1      | 1    | side: `0` = buy, `1` = sell (`0` for cancels)  |
//! | 2      | 1    | order type, see below (`0` for cancels)        |
//! | 3      | 1    | reserved, zero                                 |
//! | 4      | 4    | price tick (`0` for cancels)                   |
//! | 8      | 8    | order id                                       |
//! | 16     | 8    | quantity; the reduction for reduce records (`0` for cancels) |
//! | 24     | 8    | caller-provided sequence number                |
//! | 32     | 4    | checksum of bytes `0..32`                      |
//! | 36     | 4    | reserved, zero                                 |
//!
//! Order types: `0` = limit, `1` = immediate-or-cancel, `2` = market,
//! `3` = post-only.
//!
//! Version 1 had no order type: byte 2 was reserved and always zero, which
//! reads as "limit". Readers therefore accept both versions, and
//! [`WalWriter::open_append`] upgrades a version 1 header to version 2
//! before appending, so that an older reader cannot mistake a new record
//! for a limit order.
//!
//! # Crash safety
//!
//! The writer preallocates zero-filled space, and a crash can leave a torn
//! final record, so readers treat the first record that fails validation
//! (unknown kind or side, or a checksum mismatch) as the end of the log. The
//! checksum detects torn and zeroed records; it is not a cryptographic MAC.
//! [`WalWriter::open_append`] resumes an existing log after that point.
//!
//! # Durability and its limits
//!
//! Records appended before a successful [`WalWriter::flush`] (or
//! [`WalWriter::finalize`]) have been handed to the storage device, so they
//! survive a process crash and, normally, a power loss. To stay small, this
//! module deliberately leaves these cases to the caller:
//!
//! - **A failed flush is final.** After `flush` returns an error, records may
//!   be lost even if a later flush succeeds: the OS may already have dropped
//!   the pages it could not write. Stop accepting requests and recover by
//!   replaying the log.
//! - **New files on Unix.** A new file's directory entry is durable only once
//!   the parent directory is synced, which [`WalWriter::create`] does not do.
//!   A power loss soon after creating a log can lose the whole file.
//! - **A full disk on Linux.** `set_len` grows the file sparsely. If the disk
//!   is full when a page is first written through the mapping, the kernel
//!   stops the process with `SIGBUS` instead of `append` returning an error.
//! - **`create` is not atomic.** A crash before the header reaches the disk
//!   leaves a file whose header is all zeros. `open_append` and [`read_all`]
//!   then report `InvalidData`; nothing was logged in it, so it can be
//!   deleted.
//! - **Damage in the middle of a log.** `open_append` treats everything after
//!   the first invalid record as an unfinished tail and discards it. That is
//!   right after a crash, but would also drop valid records that follow a
//!   corrupted one.
//! - **One writer per file.** `create` refuses an existing file, but nothing
//!   stops a second `open_append` on a log that a live writer is using; on
//!   Linux that truncates the file under the first writer's mapping.

use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

use crate::engine::{NewOrder, OrderType, SelfTradePrevention};
use crate::pool::Side;

/// Size in bytes of the file header that precedes the records.
pub const WAL_HEADER_SIZE: usize = 32;
/// Size in bytes of one encoded record.
pub const WAL_RECORD_SIZE: usize = 40;
/// On-disk format version written by this module. Version 1 logs are
/// still read; see the [module docs](self#file-format-version-2).
pub const WAL_FORMAT_VERSION: u32 = 2;

const WAL_MAGIC: [u8; 8] = *b"MEWALOG\0";
/// Bytes of an encoded record covered by its checksum.
const CHECKSUMMED_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordKind {
    /// A request to submit a new order.
    NewOrder = 1,
    /// A request to cancel an existing order.
    Cancel = 2,
    /// A request to lower a resting order's quantity by `qty`.
    Reduce = 3,
}

/// One logged engine request. See the [module docs](self) for its encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalRecord {
    /// Which engine operation this record replays.
    pub kind: WalRecordKind,
    /// Side of a new order; [`Side::Buy`] for cancellation records.
    pub side: Side,
    /// Type of a new order; [`OrderType::Limit`] for cancellation records.
    pub order_type: OrderType,
    /// Identifier supplied by the caller.
    pub order_id: u64,
    /// Dense integer price tick; zero for cancellation records.
    pub price_tick: u32,
    /// Requested order quantity, or the reduction for reduce records; zero
    /// for cancellation records.
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
    /// The order type byte is not a known [`OrderType`].
    InvalidOrderType(u8),
    /// The stored checksum does not match the record contents, e.g. after a
    /// torn write.
    ChecksumMismatch,
}

impl std::fmt::Display for WalDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKind(kind) => write!(f, "invalid WAL record kind {kind}"),
            Self::InvalidSide(side) => write!(f, "invalid WAL record side {side}"),
            Self::InvalidOrderType(order_type) => {
                write!(f, "invalid WAL record order type {order_type}")
            }
            Self::ChecksumMismatch => f.write_str("WAL record checksum mismatch"),
        }
    }
}

impl std::error::Error for WalDecodeError {}

impl WalRecord {
    /// Describes a new limit-order request.
    pub fn new_order(order_id: u64, side: Side, price_tick: u32, qty: u64, sequence: u64) -> Self {
        Self::submit(NewOrder::limit(order_id, side, price_tick, qty), sequence)
    }

    /// Describes a request to submit `order`, of any [`OrderType`].
    ///
    /// The format has no account or self-trade prevention field yet: the
    /// order is logged, and replayed, for account 0 with self-trades
    /// allowed.
    pub fn submit(order: NewOrder, sequence: u64) -> Self {
        Self {
            kind: WalRecordKind::NewOrder,
            side: order.side,
            order_type: order.order_type,
            order_id: order.order_id,
            price_tick: order.price_tick,
            qty: order.qty,
            sequence,
        }
    }

    /// The order a [`NewOrder`](WalRecordKind::NewOrder) record submits,
    /// ready for [`Engine::submit_order`](crate::Engine::submit_order).
    /// Meaningless for other kinds of record.
    pub fn order(&self) -> NewOrder {
        NewOrder {
            order_id: self.order_id,
            account: 0,
            side: self.side,
            order_type: self.order_type,
            price_tick: self.price_tick,
            qty: self.qty,
            self_trade_prevention: SelfTradePrevention::Allow,
        }
    }

    /// Describes a request to lower a resting order's quantity by
    /// `reduce_by`; side, order type and price are not used.
    pub fn reduce(order_id: u64, reduce_by: u64, sequence: u64) -> Self {
        Self {
            kind: WalRecordKind::Reduce,
            qty: reduce_by,
            ..Self::cancel(order_id, sequence)
        }
    }

    /// Describes a cancellation request; fields not used by cancellation are zeroed.
    pub fn cancel(order_id: u64, sequence: u64) -> Self {
        Self {
            kind: WalRecordKind::Cancel,
            side: Side::Buy,
            order_type: OrderType::Limit,
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
        out[2] = match self.order_type {
            OrderType::Limit => 0,
            OrderType::ImmediateOrCancel => 1,
            OrderType::Market => 2,
            OrderType::PostOnly => 3,
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
            3 => WalRecordKind::Reduce,
            other => return Err(WalDecodeError::InvalidKind(other)),
        };
        let side = match bytes[1] {
            0 => Side::Buy,
            1 => Side::Sell,
            other => return Err(WalDecodeError::InvalidSide(other)),
        };
        let order_type = match bytes[2] {
            0 => OrderType::Limit,
            1 => OrderType::ImmediateOrCancel,
            2 => OrderType::Market,
            3 => OrderType::PostOnly,
            other => return Err(WalDecodeError::InvalidOrderType(other)),
        };
        if read_u32(bytes, 32) != checksum(&bytes[..CHECKSUMMED_LEN]) {
            return Err(WalDecodeError::ChecksumMismatch);
        }
        Ok(Self {
            kind,
            side,
            order_type,
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

/// Checks the header and returns the log's format version.
fn validate_header(bytes: &[u8]) -> io::Result<u32> {
    if bytes.len() < WAL_HEADER_SIZE {
        return Err(invalid_data("file is shorter than a WAL header".into()));
    }
    if bytes[..8] != WAL_MAGIC {
        return Err(invalid_data("not a WAL file (bad magic)".into()));
    }
    let version = read_u32(bytes, 8);
    if !(1..=WAL_FORMAT_VERSION).contains(&version) {
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
    Ok(version)
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

/// Append-only memory-mapped WAL writer.
///
/// The backing file is preallocated and grown (then remapped) only when
/// that space is exhausted. Appending within the mapped capacity writes
/// directly to the mapping; call [`flush`](Self::flush) when the appended
/// records must be durable, and [`finalize`](Self::finalize) when done.
pub struct WalWriter {
    /// Always `Some` while the writer is usable; taken by `finalize`/`Drop`.
    mmap: Option<MmapMut>,
    file: File,
    next_offset: usize,
    capacity_bytes: usize,
}

impl WalWriter {
    /// Creates a new WAL file, writes its header, and preallocates room for
    /// at least `capacity_records` records.
    ///
    /// Fails with [`io::ErrorKind::AlreadyExists`] if `path` already exists,
    /// so a restart can never erase the log it should replay. Continue an
    /// existing log with [`open_append`](Self::open_append), or delete the
    /// file deliberately before calling `create`.
    ///
    /// Creation is not atomic: a crash before the header is flushed leaves a
    /// file with a zeroed header, and on Unix the new directory entry is not
    /// synced. See the [module docs](self#durability-and-its-limits).
    pub fn create<P: AsRef<Path>>(path: P, capacity_records: usize) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
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
    /// are discarded, so they can never be mistaken for records later. That
    /// is right after a crash, but it would also discard valid records that
    /// follow a corrupted one. The whole file is read once, with plain I/O,
    /// to find that record. A version 1 log is upgraded to the current
    /// version first (see the [module docs](self#file-format-version-2)).
    /// Use one writer per file: opening a log that a live writer is still
    /// using corrupts it.
    pub fn open_append<P: AsRef<Path>>(path: P, spare_records: usize) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let version = validate_header(&bytes)?;
        let valid_len =
            WAL_HEADER_SIZE + valid_records(&bytes[WAL_HEADER_SIZE..]).count() * WAL_RECORD_SIZE;
        // Truncating (no view is mapped yet, which Windows requires) and then
        // re-extending in `map` zero-fills everything after the valid prefix.
        file.set_len(valid_len as u64)?;
        let mut writer = Self::map(file, valid_len, spare_records)?;
        if version != WAL_FORMAT_VERSION {
            // Every older record is also valid in the current format, so
            // only the version field changes. It must be durable before a
            // record that an older reader would misread can be.
            writer.mapping()[..WAL_HEADER_SIZE].copy_from_slice(&encode_header());
            writer.flush()?;
        }
        Ok(writer)
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
    pub fn append(&mut self, record: WalRecord) -> io::Result<()> {
        if self.next_offset + WAL_RECORD_SIZE > self.capacity_bytes {
            self.grow()?;
        }
        let start = self.next_offset;
        self.mapping()[start..start + WAL_RECORD_SIZE].copy_from_slice(&record.encode());
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
        // SAFETY: as in `map`.
        let mmap = unsafe { MmapOptions::new().len(new_capacity).map_mut(&self.file)? };
        self.mmap = Some(mmap);
        self.capacity_bytes = new_capacity;
        Ok(())
    }

    /// Writes dirty mapped pages back to the file and waits until the
    /// storage device reports them durable (`msync` / `FlushViewOfFile` +
    /// `FlushFileBuffers`).
    ///
    /// If this returns an error, treat the log as damaged rather than
    /// retrying and carrying on: the OS may already have discarded the pages
    /// it failed to write, so a later successful flush does not prove they
    /// reached the disk. See the [module docs](self#durability-and-its-limits).
    pub fn flush(&self) -> io::Result<()> {
        match &self.mmap {
            Some(mmap) => mmap.flush(),
            None => Ok(()),
        }
    }

    /// Flushes the log, releases the mapping, trims the preallocated tail,
    /// and syncs the file, leaving exactly the header plus every record.
    ///
    /// Consumes the writer so nothing can be appended afterwards; use
    /// [`open_append`](Self::open_append) to continue the log later. Dropping
    /// a writer performs the same steps but ignores any I/O error.
    pub fn finalize(mut self) -> io::Result<()> {
        self.release()
    }

    fn release(&mut self) -> io::Result<()> {
        let Some(mmap) = self.mmap.take() else {
            return Ok(());
        };
        mmap.flush()?;
        // Windows refuses to shrink a file while any view of it is mapped.
        drop(mmap);
        self.file.set_len(self.next_offset as u64)?;
        self.file.sync_all()
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
        let _ = self.release();
    }
}

/// Reads every valid record from a WAL file for replay.
///
/// Reading stops at the first partial or invalid record, which marks the end
/// of the log (for example the zero-filled preallocated tail left by a crash,
/// or a torn final write). A missing or unsupported header is reported as
/// [`io::ErrorKind::InvalidData`].
///
/// The file is read into memory with plain I/O rather than mapped. A copy
/// cannot race a writer that is appending to the same file: at worst it ends
/// in a half-written record, which fails its checksum and ends the log.
pub fn read_all<P: AsRef<Path>>(path: P) -> io::Result<Vec<WalRecord>> {
    let bytes = std::fs::read(path)?;
    validate_header(&bytes)?;
    Ok(valid_records(&bytes[WAL_HEADER_SIZE..]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A per-process temp file path that is removed when dropped.
    struct TempPath(PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("wal_{name}_{}.bin", std::process::id()));
            // A file left behind by an aborted earlier run would make
            // `WalWriter::create` fail, so start from a clean slate.
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    const ORDER_TYPES: [OrderType; 4] = [
        OrderType::Limit,
        OrderType::ImmediateOrCancel,
        OrderType::Market,
        OrderType::PostOnly,
    ];

    fn sample(i: u64) -> WalRecord {
        let order_type = ORDER_TYPES[(i / 3 % 4) as usize];
        match i % 3 {
            2 if i.is_multiple_of(2) => WalRecord::reduce(i - 1, i % 7 + 1, i),
            2 => WalRecord::cancel(i - 1, i),
            0 => WalRecord::new_order(i, Side::Buy, 100 + i as u32, 10 + i, i),
            _ => WalRecord::submit(
                NewOrder::limit(i, Side::Sell, 200 + i as u32, u64::MAX - i).with_type(order_type),
                i,
            ),
        }
    }

    /// Builds a raw log image: a valid header followed by the given slots.
    fn image(slots: &[[u8; WAL_RECORD_SIZE]]) -> Vec<u8> {
        let mut bytes = encode_header().to_vec();
        for slot in slots {
            bytes.extend_from_slice(slot);
        }
        bytes
    }

    fn file_len(path: &TempPath) -> usize {
        std::fs::metadata(&path.0).unwrap().len() as usize
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn create_refuses_to_overwrite_an_existing_log() {
        let path = TempPath::new("no_overwrite");
        let original = image(&[sample(0).encode()]);
        std::fs::write(&path.0, &original).unwrap();

        let err = WalWriter::create(&path.0, 4)
            .err()
            .expect("create must not open an existing file");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(&path.0).unwrap(),
            original,
            "log was modified"
        );
    }

    #[test]
    fn encode_decode_roundtrip() {
        for i in 0..12 {
            let record = sample(i);
            assert_eq!(WalRecord::decode(&record.encode()), Ok(record));
        }
    }

    #[test]
    fn decode_rejects_zeroed_and_corrupted_records() {
        assert_eq!(
            WalRecord::decode(&[0; WAL_RECORD_SIZE]),
            Err(WalDecodeError::InvalidKind(0))
        );

        let good = sample(0).encode();
        let mut bad_side = good;
        bad_side[1] = 7;
        assert_eq!(
            WalRecord::decode(&bad_side),
            Err(WalDecodeError::InvalidSide(7))
        );
        let mut bad_type = good;
        bad_type[2] = 4;
        assert_eq!(
            WalRecord::decode(&bad_type),
            Err(WalDecodeError::InvalidOrderType(4))
        );

        // Every single-bit flip in the covered fields or in the checksum
        // itself is detected. A flip in byte 0, 1 or 2 may be caught as an
        // invalid kind, side or order type instead of a checksum mismatch.
        for byte in 0..36 {
            for bit in 0..8 {
                let mut torn = good;
                torn[byte] ^= 1 << bit;
                assert!(
                    WalRecord::decode(&torn).is_err(),
                    "flip of bit {bit} in byte {byte} went undetected"
                );
            }
        }
        // Bytes 36..40 are reserved and not covered: changing them is not
        // detected.
        let mut reserved = good;
        reserved[36] = 0xFF;
        assert_eq!(WalRecord::decode(&reserved), Ok(sample(0)));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri cannot map files.
    fn append_grow_finalize_and_read_back() {
        let path = TempPath::new("roundtrip");
        let mut writer = WalWriter::create(&path.0, 4).unwrap();
        // Force several growths beyond the preallocated capacity.
        for i in 0..10 {
            writer.append(sample(i)).unwrap();
        }
        assert_eq!(writer.len(), 10);
        writer.finalize().unwrap();

        assert_eq!(file_len(&path), WAL_HEADER_SIZE + 10 * WAL_RECORD_SIZE);
        let records = read_all(&path.0).unwrap();
        assert_eq!(records, (0..10).map(sample).collect::<Vec<_>>());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn dropping_a_writer_finalizes_it() {
        let path = TempPath::new("drop");
        {
            let mut writer = WalWriter::create(&path.0, 8).unwrap();
            writer.append(sample(0)).unwrap();
        }
        assert_eq!(file_len(&path), WAL_HEADER_SIZE + WAL_RECORD_SIZE);
        assert_eq!(read_all(&path.0).unwrap(), vec![sample(0)]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn finalizing_an_empty_log_keeps_only_the_header() {
        // The preallocated space is trimmed, but the header stays: an empty
        // log is still a valid log that `open_append` can continue.
        let path = TempPath::new("empty");
        let writer = WalWriter::create(&path.0, 16).unwrap();
        assert!(writer.is_empty());
        writer.finalize().unwrap();

        assert_eq!(file_len(&path), WAL_HEADER_SIZE);
        assert!(read_all(&path.0).unwrap().is_empty());
        let writer = WalWriter::open_append(&path.0, 1).unwrap();
        assert!(writer.is_empty());
    }

    // Crash images are written directly: a writer leaked with
    // `mem::forget` would keep its view mapped, which blocks the truncation
    // `open_append` performs on Windows.

    #[test]
    #[cfg_attr(miri, ignore)]
    fn read_stops_at_zeroed_preallocated_tail() {
        let path = TempPath::new("zero_tail");
        let mut slots: Vec<_> = (0..3).map(|i| sample(i).encode()).collect();
        slots.extend([[0u8; WAL_RECORD_SIZE]; 5]);
        std::fs::write(&path.0, image(&slots)).unwrap();

        assert_eq!(
            read_all(&path.0).unwrap(),
            (0..3).map(sample).collect::<Vec<_>>()
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn read_stops_before_a_torn_record() {
        let path = TempPath::new("torn");
        let mut slots: Vec<_> = (0..4).map(|i| sample(i).encode()).collect();
        slots[2][20] ^= 0xFF;
        std::fs::write(&path.0, image(&slots)).unwrap();

        assert_eq!(
            read_all(&path.0).unwrap(),
            (0..2).map(sample).collect::<Vec<_>>()
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn read_ignores_a_partial_trailing_record() {
        let path = TempPath::new("partial");
        let mut bytes = image(&[sample(0).encode(), sample(1).encode()]);
        bytes.extend_from_slice(&sample(2).encode()[..17]);
        std::fs::write(&path.0, bytes).unwrap();

        assert_eq!(read_all(&path.0).unwrap(), vec![sample(0), sample(1)]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn open_append_resumes_after_the_last_valid_record() {
        let path = TempPath::new("resume");
        let mut slots: Vec<_> = (0..3).map(|i| sample(i).encode()).collect();
        slots[2][9] ^= 0x10; // torn third record
        slots.push(sample(3).encode()); // stale data after the tear
        std::fs::write(&path.0, image(&slots)).unwrap();

        let mut writer = WalWriter::open_append(&path.0, 2).unwrap();
        assert_eq!(writer.len(), 2);
        for i in 10..15 {
            writer.append(sample(i)).unwrap();
        }
        writer.finalize().unwrap();

        let expected: Vec<_> = (0..2).chain(10..15).map(sample).collect();
        assert_eq!(read_all(&path.0).unwrap(), expected);
        assert_eq!(file_len(&path), WAL_HEADER_SIZE + 7 * WAL_RECORD_SIZE);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn open_append_upgrades_a_version_1_log() {
        // A version 1 log: limit orders and cancels, byte 2 always zero.
        let path = TempPath::new("v1");
        let old: Vec<_> = vec![
            WalRecord::new_order(1, Side::Buy, 100, 5, 0),
            WalRecord::cancel(1, 1),
        ];
        let mut bytes = image(&old.iter().map(WalRecord::encode).collect::<Vec<_>>());
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        std::fs::write(&path.0, &bytes).unwrap();
        assert_eq!(read_all(&path.0).unwrap(), old, "version 1 is still read");

        let new = WalRecord::submit(NewOrder::market(2, Side::Sell, 3), 2);
        let mut writer = WalWriter::open_append(&path.0, 4).unwrap();
        writer.append(new).unwrap();
        writer.finalize().unwrap();

        let bytes = std::fs::read(&path.0).unwrap();
        assert_eq!(read_u32(&bytes, 8), WAL_FORMAT_VERSION);
        assert_eq!(read_all(&path.0).unwrap(), [old[0], old[1], new]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn rejects_files_without_a_valid_header() {
        let path = TempPath::new("bad_header");
        let is_invalid_data = |result: io::Result<Vec<WalRecord>>| matches!(result, Err(e) if e.kind() == io::ErrorKind::InvalidData);

        std::fs::write(&path.0, [0u8; 8]).unwrap();
        assert!(is_invalid_data(read_all(&path.0)), "short file");

        let mut bytes = image(&[sample(0).encode()]);
        bytes[0] ^= 0xFF;
        std::fs::write(&path.0, &bytes).unwrap();
        assert!(is_invalid_data(read_all(&path.0)), "bad magic");
        assert!(
            matches!(WalWriter::open_append(&path.0, 1), Err(e) if e.kind() == io::ErrorKind::InvalidData),
            "open_append must reject a bad magic too"
        );

        let mut bytes = image(&[sample(0).encode()]);
        bytes[8] = 3;
        std::fs::write(&path.0, &bytes).unwrap();
        assert!(is_invalid_data(read_all(&path.0)), "future version");
    }
}
