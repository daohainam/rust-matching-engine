//! Memory-mapped binary Write-Ahead Log (WAL) for deterministic replay of
//! matching engine state transitions.
//!
//! Every mutating request (new order / cancel) is appended as a
//! fixed-size, `repr(C)` binary record directly into a memory-mapped file.
//! Because the record layout is fixed-width, no serialization allocation is
//! required and replay can seek by simple arithmetic (`offset = index *
//! size_of::<WalRecord>()`).
//!
//! This is a compact, implementation-specific log format: raw record bytes
//! use the target's `repr(C)` layout and native integer byte order. Replaying
//! on a different architecture or with a changed record definition requires
//! an explicit format migration or field-by-field encoding.

use memmap2::{MmapMut, MmapOptions};
use std::fs::OpenOptions;
use std::io;
use std::mem::size_of;
use std::path::Path;

use crate::pool::Side;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordKind {
    /// A request to submit a new limit order.
    NewOrder = 1,
    /// A request to cancel an existing order.
    Cancel = 2,
}

/// Fixed-layout record stored in the WAL.
///
/// `repr(C)` fixes field ordering and alignment according to the target's C
/// ABI. The record is written as raw bytes, so this layout is not by itself a
/// portable, versioned on-disk schema.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct WalRecord {
    /// Numeric discriminant corresponding to [`WalRecordKind`].
    pub kind: u8,
    /// Side encoding for a new order: `0` for buy, `1` for sell.
    pub side: u8,
    /// Reserved bytes that keep the leading fields aligned.
    pub _padding: [u8; 6],
    /// Identifier supplied by the caller.
    pub order_id: u64,
    /// Dense integer price tick; unused by cancellation records.
    pub price_tick: u32,
    /// Requested order quantity; unused by cancellation records.
    pub qty: u64,
    /// Caller-provided sequence number for deterministic request ordering.
    pub sequence: u64,
}

/// Size in bytes of one record on the current compilation target.
pub const WAL_RECORD_SIZE: usize = size_of::<WalRecord>();

impl WalRecord {
    /// Encodes the fields of a new-order request into one record.
    pub fn new_order(order_id: u64, side: Side, price_tick: u32, qty: u64, sequence: u64) -> Self {
        Self {
            kind: WalRecordKind::NewOrder as u8,
            side: match side {
                Side::Buy => 0,
                Side::Sell => 1,
            },
            _padding: [0; 6],
            order_id,
            price_tick,
            qty,
            sequence,
        }
    }

    /// Encodes a cancellation request; fields not used by cancellation are zeroed.
    pub fn cancel(order_id: u64, sequence: u64) -> Self {
        Self {
            kind: WalRecordKind::Cancel as u8,
            side: 0,
            _padding: [0; 6],
            order_id,
            price_tick: 0,
            qty: 0,
            sequence,
        }
    }

    #[inline]
    fn to_bytes(self) -> [u8; WAL_RECORD_SIZE] {
        // The WAL stores the record's in-memory representation directly; it
        // does not perform per-field endian conversion or schema versioning.
        // SAFETY: WalRecord is `repr(C)`, `Copy`, and contains only plain
        // integer fields, so reinterpreting it as bytes is sound.
        unsafe { std::mem::transmute_copy(&self) }
    }

    #[inline]
    fn from_bytes(bytes: &[u8]) -> Self {
        debug_assert_eq!(bytes.len(), WAL_RECORD_SIZE);
        let mut buf = [0u8; WAL_RECORD_SIZE];
        buf.copy_from_slice(bytes);
        // SAFETY: buf has exactly the size and (via padding) alignment
        // requirements of WalRecord, whose fields are all plain integers.
        unsafe { std::mem::transmute_copy(&buf) }
    }
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
    /// Creates or truncates a WAL file and reserves room for at least one record.
    pub fn create<P: AsRef<Path>>(path: P, capacity_records: usize) -> io::Result<Self> {
        let capacity_bytes = capacity_records.max(1) * WAL_RECORD_SIZE;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(capacity_bytes as u64)?;
        let mmap = unsafe { MmapOptions::new().len(capacity_bytes).map_mut(&file)? };
        Ok(Self {
            mmap: Some(mmap),
            file,
            next_offset: 0,
            capacity_bytes,
        })
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
        let new_capacity = (self.capacity_bytes * 2).max(WAL_RECORD_SIZE);
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

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.next_offset / WAL_RECORD_SIZE
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.next_offset == 0
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

/// Reads every complete record from a WAL file for replay.
///
/// Any trailing bytes that do not make up a full record are ignored. This
/// function decodes the target-specific raw layout; it does not validate the
/// record kind or translate between format versions.
pub fn read_all<P: AsRef<Path>>(path: P) -> io::Result<Vec<WalRecord>> {
    let file = OpenOptions::new().read(true).open(path)?;
    let len = file.metadata()?.len() as usize;
    if len == 0 {
        return Ok(Vec::new());
    }
    let mmap = unsafe { MmapOptions::new().len(len).map(&file)? };
    let count = len / WAL_RECORD_SIZE;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let start = i * WAL_RECORD_SIZE;
        out.push(WalRecord::from_bytes(&mmap[start..start + WAL_RECORD_SIZE]));
    }
    Ok(out)
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
