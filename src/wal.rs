//! Memory-mapped binary Write-Ahead Log (WAL) for deterministic replay of
//! matching engine state transitions.
//!
//! Every mutating request (new order / cancel) is appended as a
//! fixed-size, `repr(C)` binary record directly into a memory-mapped file.
//! Because the record layout is fixed-width, no serialization allocation is
//! required and replay can seek by simple arithmetic (`offset = index *
//! size_of::<WalRecord>()`).

use memmap2::{MmapMut, MmapOptions};
use std::fs::OpenOptions;
use std::io;
use std::mem::size_of;
use std::path::Path;

use crate::pool::Side;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordKind {
    NewOrder = 1,
    Cancel = 2,
}

/// Fixed-width binary WAL record. `repr(C)` keeps the layout stable across
/// compilations so that log files can be replayed deterministically.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct WalRecord {
    pub kind: u8,
    pub side: u8,
    pub _padding: [u8; 6],
    pub order_id: u64,
    pub price_tick: u32,
    pub qty: u64,
    pub sequence: u64,
}

pub const WAL_RECORD_SIZE: usize = size_of::<WalRecord>();

impl WalRecord {
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

/// Append-only memory-mapped WAL writer. The backing file is pre-sized to
/// `capacity_records` and grown (remapped) only if that capacity is
/// exceeded, keeping the steady-state append path allocation-free.
pub struct WalWriter {
    mmap: MmapMut,
    file: std::fs::File,
    next_offset: usize,
    capacity_bytes: usize,
}

impl WalWriter {
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
            mmap,
            file,
            next_offset: 0,
            capacity_bytes,
        })
    }

    /// Appends `record` to the log. O(1); only touches already-mapped
    /// memory unless the pre-sized capacity has been exhausted, in which
    /// case the backing file is grown and remapped.
    pub fn append(&mut self, record: WalRecord) -> io::Result<()> {
        if self.next_offset + WAL_RECORD_SIZE > self.capacity_bytes {
            self.grow()?;
        }
        let bytes = record.to_bytes();
        self.mmap[self.next_offset..self.next_offset + WAL_RECORD_SIZE].copy_from_slice(&bytes);
        self.next_offset += WAL_RECORD_SIZE;
        Ok(())
    }

    fn grow(&mut self) -> io::Result<()> {
        let new_capacity = (self.capacity_bytes * 2).max(WAL_RECORD_SIZE);
        self.file.set_len(new_capacity as u64)?;
        self.mmap = unsafe { MmapOptions::new().len(new_capacity).map_mut(&self.file)? };
        self.capacity_bytes = new_capacity;
        Ok(())
    }

    /// Flushes pending writes to disk.
    pub fn flush(&self) -> io::Result<()> {
        self.mmap.flush()
    }

    /// Flushes and truncates the backing file down to the number of bytes
    /// actually written, discarding any pre-allocated-but-unused tail
    /// capacity. Safe to call multiple times.
    pub fn finalize(&mut self) -> io::Result<()> {
        self.flush()?;
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
        // Best-effort: reclaim any pre-allocated-but-unwritten tail so that
        // `read_all` sees exactly the records that were appended.
        let _ = self.finalize();
    }
}

/// Reads back all records from a WAL file for deterministic replay.
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
}
