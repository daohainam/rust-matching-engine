//! Snapshots of an engine's complete state, and checkpoint files that keep
//! them on disk.
//!
//! The write-ahead log rebuilds a book by replaying every request since the
//! log began, so recovery gets slower the longer the engine runs. A
//! [`Snapshot`] captures the state itself: [`Engine::snapshot`] takes one,
//! and [`Engine::restore`] builds an engine that behaves exactly like the
//! original from that point on. A [`Checkpoint`] stores a snapshot together
//! with the WAL sequence number it reaches, so recovery is: restore the
//! checkpoint, then replay only the requests logged after it.
//!
//! # Checkpoint file format (version 2)
//!
//! Little-endian, like the WAL. An 80-byte header:
//!
//! | Offset | Size | Field                                 |
//! |-------:|-----:|---------------------------------------|
//! | 0      | 8    | magic `b"MESNAP\0\0"`                 |
//! | 8      | 4    | format version (`2`)                  |
//! | 12     | 4    | order entry size in bytes (`32`)      |
//! | 16     | 8    | engine capacity                       |
//! | 24     | 8    | accepted orders                       |
//! | 32     | 8    | trades                                |
//! | 40     | 8    | next WAL sequence (see [`Checkpoint`]) |
//! | 48     | 8    | number of orders                      |
//! | 56     | 8    | engine tick count                     |
//! | 64     | 8    | maximum order quantity                |
//! | 72     | 8    | price band (`u64::MAX` = none)        |
//!
//! Version 1 had a 56-byte header, without the last three fields; it is
//! still read, with the default 4096 ticks and no risk limits, the only
//! values it could describe.
//! then one 32-byte entry per resting order, in the snapshot's order:
//!
//! | Offset | Size | Field                         |
//! |-------:|-----:|-------------------------------|
//! | 0      | 8    | order id                      |
//! | 8      | 8    | remaining quantity            |
//! | 16     | 4    | account                       |
//! | 20     | 4    | price tick                    |
//! | 24     | 1    | side: `0` = buy, `1` = sell   |
//! | 25     | 7    | reserved, zero                |
//!
//! and finally an 8-byte checksum of everything before it, every 8-byte
//! word folded through the SplitMix64 finalizer.
//!
//! # Writing atomically
//!
//! A checkpoint must never be half-written: recovery would then have
//! neither the old checkpoint nor the new one. [`write_checkpoint`] writes
//! the whole file to a unique `<path>.tmp.<pid>.<id>` file, syncs it, and
//! renames it over `path` (a rename replaces the target in one step).
//! On Unix it then syncs the directory
//! so the rename itself is durable. A crash at any point leaves either the
//! old checkpoint or the new one; at worst a stale temporary file, which
//! future writes skip. Overlapping writes use separate files, so none can
//! modify another write's published checkpoint.
//!
//! [`Engine::snapshot`]: crate::Engine::snapshot
//! [`Engine::restore`]: crate::Engine::restore

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::engine::{RestingOrder, RiskLimits};
use crate::events::RejectReason;
use crate::pool::Side;
use crate::wal::mix64;

/// The complete state of an [`Engine`](crate::Engine): everything needed to
/// build an engine that behaves exactly like it from then on.
///
/// Taking and restoring a snapshot allocates, so it belongs next to the hot
/// path, not on it (for example in a checkpoint every few minutes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The engine's `order_capacity`. It decides which orders can rest, so
    /// a restored engine must have the same one.
    pub capacity: usize,
    /// The engine's number of price ticks: it decides which prices are
    /// valid and where a market order's limit is.
    pub tick_count: usize,
    /// How many orders the engine has accepted (its audit counter).
    pub accepted_orders: u64,
    /// How many trades it has executed: the next trade gets `trades + 1`
    /// as its `trade_id`.
    pub trades: u64,
    /// The pre-trade risk limits in force.
    pub risk_limits: RiskLimits,
    /// Every resting order, in priority order: bids best price first, then
    /// asks best price first, oldest first at each price. Restoring needs
    /// only the order *within* each price; the rest makes snapshots of the
    /// same book compare equal.
    pub orders: Vec<RestingOrder>,
}

/// Why [`Engine::restore`](crate::Engine::restore) refused a snapshot. A
/// snapshot taken by the engine never fails; these catch damaged or
/// hand-made ones before they become a corrupt book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    /// The capacity is zero or too large for an engine.
    InvalidCapacity(usize),
    /// The tick count is zero or above
    /// [`MAX_LADDER_TICKS`](crate::bitset::MAX_LADDER_TICKS).
    InvalidTickCount(usize),
    /// There are more orders than the capacity allows to rest.
    TooManyOrders { orders: usize, capacity: usize },
    /// An order could not rest; `reason` says why, as it would for a new
    /// order (`ZeroQuantity`, `ReservedOrderId`, `PriceOutOfRange`,
    /// `DuplicateOrderId` or `LevelQuantityOverflow`).
    InvalidOrder { order_id: u64, reason: RejectReason },
    /// The best bid reaches the best ask. A live book never looks like
    /// this: such orders would have traded.
    CrossedBook { best_bid: u32, best_ask: u32 },
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCapacity(capacity) => write!(f, "invalid engine capacity {capacity}"),
            Self::InvalidTickCount(ticks) => write!(f, "invalid tick count {ticks}"),
            Self::TooManyOrders { orders, capacity } => {
                write!(
                    f,
                    "{orders} resting orders do not fit in capacity {capacity}"
                )
            }
            Self::InvalidOrder { order_id, reason } => {
                write!(f, "order {order_id} cannot rest: {reason:?}")
            }
            Self::CrossedBook { best_bid, best_ask } => {
                write!(
                    f,
                    "crossed book: best bid {best_bid} >= best ask {best_ask}"
                )
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Lets recovery code that already returns [`io::Result`] use `?` on
/// [`Engine::restore`](crate::Engine::restore): a snapshot that does not
/// restore is invalid data.
impl From<SnapshotError> for io::Error {
    fn from(err: SnapshotError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, err)
    }
}

/// A [`Snapshot`] on disk, and how far into the write-ahead log it reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// The WAL sequence number of the first request the snapshot does *not*
    /// include: recovery replays the records from this sequence on. A
    /// checkpoint taken before any request has `0`.
    pub next_sequence: u64,
    /// The engine's state after every request before `next_sequence`.
    pub snapshot: Snapshot,
}

const MAGIC: [u8; 8] = *b"MESNAP\0\0";
const FORMAT_VERSION: u32 = 2;
const HEADER_SIZE: usize = 80;
/// The header of a version 1 checkpoint, which ends before the tick count.
const HEADER_SIZE_V1: usize = 56;
const ENTRY_SIZE: usize = 32;
const CHECKSUM_SIZE: usize = 8;

/// Writes `checkpoint` to `path` atomically, replacing any checkpoint
/// already there. See [Writing atomically](self#writing-atomically).
pub fn write_checkpoint<P: AsRef<Path>>(path: P, checkpoint: &Checkpoint) -> io::Result<()> {
    let path = path.as_ref();
    let (temporary, file) = TemporaryCheckpoint::create(path)?;
    {
        let mut file = file;
        file.write_all(&encode(checkpoint))?;
        // The contents must be durable before the rename makes them the
        // checkpoint.
        file.sync_all()?;
    }
    std::fs::rename(&temporary.0, path)?;
    sync_parent_directory(path)
}

/// Reads a checkpoint written by [`write_checkpoint`].
///
/// A file that is truncated, damaged (checksum mismatch) or not a
/// checkpoint is reported as [`io::ErrorKind::InvalidData`]. Whether its
/// snapshot describes a possible book is checked later, by
/// [`Engine::restore`](crate::Engine::restore).
pub fn read_checkpoint<P: AsRef<Path>>(path: P) -> io::Result<Checkpoint> {
    decode(&std::fs::read(path)?)
}

/// A separate file for each write, created exclusively so stale files and
/// other processes cannot be overwritten. Drop also cleans up failed writes.
struct TemporaryCheckpoint(PathBuf);

impl TemporaryCheckpoint {
    fn create(path: &Path) -> io::Result<(Self, File)> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        loop {
            let temporary = temporary_path(path, NEXT_ID.fetch_add(1, Ordering::Relaxed));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => return Ok((Self(temporary), file)),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
        }
    }
}

impl Drop for TemporaryCheckpoint {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temporary_path(path: &Path, id: u64) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".tmp.{}.{id}", std::process::id()));
    name.into()
}

/// Makes a rename inside `path`'s directory durable. Unix keeps directory
/// entries apart from file contents, so syncing the file is not enough;
/// on Windows a rename that returned is durable already.
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn checksum(bytes: &[u8]) -> u64 {
    let (words, rest) = bytes.as_chunks::<8>();
    debug_assert!(rest.is_empty(), "a checkpoint is a whole number of words");
    words
        .iter()
        .fold(0, |acc, word| mix64(acc ^ u64::from_le_bytes(*word)))
}

fn encode(checkpoint: &Checkpoint) -> Vec<u8> {
    let Snapshot {
        capacity,
        accepted_orders,
        trades,
        ref orders,
        tick_count,
        risk_limits,
    } = checkpoint.snapshot;
    let mut out = Vec::with_capacity(HEADER_SIZE + orders.len() * ENTRY_SIZE + CHECKSUM_SIZE);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(ENTRY_SIZE as u32).to_le_bytes());
    for field in [
        capacity as u64,
        accepted_orders,
        trades,
        checkpoint.next_sequence,
        orders.len() as u64,
        tick_count as u64,
        risk_limits.max_order_qty,
        risk_limits.price_band.map_or(u64::MAX, u64::from),
    ] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    for order in orders {
        out.extend_from_slice(&order.order_id.to_le_bytes());
        out.extend_from_slice(&order.remaining_qty.to_le_bytes());
        out.extend_from_slice(&order.account.to_le_bytes());
        out.extend_from_slice(&order.price_tick.to_le_bytes());
        out.push(match order.side {
            Side::Buy => 0,
            Side::Sell => 1,
        });
        out.extend_from_slice(&[0; 7]);
    }
    let checksum = checksum(&out);
    out.extend_from_slice(&checksum.to_le_bytes());
    out
}

fn decode(bytes: &[u8]) -> io::Result<Checkpoint> {
    let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidData, message.to_owned());
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"));

    if bytes.len() < HEADER_SIZE_V1 + CHECKSUM_SIZE || bytes[..8] != MAGIC {
        return Err(invalid("not a checkpoint file"));
    }
    let header_size = match u32_at(8) {
        1 => HEADER_SIZE_V1,
        FORMAT_VERSION => HEADER_SIZE,
        _ => return Err(invalid("unsupported checkpoint format")),
    };
    if u32_at(12) as usize != ENTRY_SIZE || bytes.len() < header_size + CHECKSUM_SIZE {
        return Err(invalid("unsupported checkpoint format"));
    }
    let count = usize::try_from(u64_at(48)).map_err(|_| invalid("too many orders"))?;
    let expected_len = count
        .checked_mul(ENTRY_SIZE)
        .and_then(|entries| entries.checked_add(header_size + CHECKSUM_SIZE));
    if expected_len != Some(bytes.len()) {
        return Err(invalid("checkpoint file has the wrong length"));
    }
    let body = &bytes[..bytes.len() - CHECKSUM_SIZE];
    if checksum(body) != u64_at(body.len()) {
        return Err(invalid("checkpoint checksum mismatch"));
    }

    let (tick_count, risk_limits) = if header_size == HEADER_SIZE_V1 {
        (crate::engine::DEFAULT_TICKS, RiskLimits::NONE)
    } else {
        let price_band = match u64_at(72) {
            u64::MAX => None,
            band => Some(u32::try_from(band).map_err(|_| invalid("price band too large"))?),
        };
        let ticks = usize::try_from(u64_at(56)).map_err(|_| invalid("tick count too large"))?;
        let limits = RiskLimits {
            max_order_qty: u64_at(64),
            price_band,
        };
        (ticks, limits)
    };
    let (entries, _) = bytes[header_size..body.len()].as_chunks::<ENTRY_SIZE>();
    let orders = entries
        .iter()
        .map(|entry| {
            let side = match entry[24] {
                0 => Side::Buy,
                1 => Side::Sell,
                _ => return Err(invalid("invalid side in checkpoint")),
            };
            Ok(RestingOrder {
                order_id: u64::from_le_bytes(entry[0..8].try_into().expect("8 bytes")),
                remaining_qty: u64::from_le_bytes(entry[8..16].try_into().expect("8 bytes")),
                account: u32::from_le_bytes(entry[16..20].try_into().expect("4 bytes")),
                price_tick: u32::from_le_bytes(entry[20..24].try_into().expect("4 bytes")),
                side,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(Checkpoint {
        next_sequence: u64_at(40),
        snapshot: Snapshot {
            capacity: usize::try_from(u64_at(16)).map_err(|_| invalid("capacity too large"))?,
            tick_count,
            accepted_orders: u64_at(24),
            risk_limits,
            trades: u64_at(32),
            orders,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Checkpoint {
        let order = |order_id, account, side, price_tick, remaining_qty| RestingOrder {
            order_id,
            account,
            side,
            price_tick,
            remaining_qty,
        };
        Checkpoint {
            next_sequence: 1_234,
            snapshot: Snapshot {
                capacity: 4_096,
                tick_count: 300_000,
                accepted_orders: 1_000,
                risk_limits: RiskLimits {
                    max_order_qty: 5_000,
                    price_band: Some(25),
                },
                trades: 77,
                orders: vec![
                    order(5, 1, Side::Buy, 99, 7),
                    order(2, 0, Side::Sell, 100, u64::MAX),
                    order(3, 9, Side::Sell, 4_095, 1),
                ],
            },
        }
    }

    #[test]
    fn encode_decode_roundtrip() {
        let checkpoint = sample();
        let bytes = encode(&checkpoint);
        assert_eq!(bytes.len(), HEADER_SIZE + 3 * ENTRY_SIZE + CHECKSUM_SIZE);
        assert_eq!(decode(&bytes).unwrap(), checkpoint);

        let empty = Checkpoint {
            next_sequence: 0,
            snapshot: Snapshot {
                orders: vec![],
                ..sample().snapshot
            },
        };
        assert_eq!(decode(&encode(&empty)).unwrap(), empty);
    }

    #[test]
    fn a_version_1_checkpoint_is_still_read() {
        // Version 1: no tick count or limits, so the defaults come back.
        let checkpoint = Checkpoint {
            next_sequence: 7,
            snapshot: Snapshot {
                tick_count: crate::engine::DEFAULT_TICKS,
                risk_limits: RiskLimits::NONE,
                ..sample().snapshot
            },
        };
        let current = encode(&checkpoint);
        let mut old = current[..HEADER_SIZE_V1].to_vec();
        old[8..12].copy_from_slice(&1u32.to_le_bytes());
        old.extend_from_slice(&current[HEADER_SIZE..current.len() - CHECKSUM_SIZE]);
        let checksum = checksum(&old);
        old.extend_from_slice(&checksum.to_le_bytes());
        assert_eq!(decode(&old).unwrap(), checkpoint);
    }

    #[test]
    fn damage_anywhere_is_detected() {
        let good = encode(&sample());
        for byte in 0..good.len() {
            for bit in 0..8 {
                let mut bad = good.clone();
                bad[byte] ^= 1 << bit;
                let err = decode(&bad).expect_err("a flipped bit must be caught");
                assert_eq!(err.kind(), io::ErrorKind::InvalidData, "byte {byte}");
            }
        }
        for len in 0..good.len() {
            assert!(decode(&good[..len]).is_err(), "truncated to {len} bytes");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri cannot rename files.
    fn writing_replaces_the_checkpoint_in_one_step() {
        let path = std::env::temp_dir().join(format!("checkpoint_{}.bin", std::process::id()));
        // A temporary file left by a crash is skipped, never overwritten.
        let stale = temporary_path(&path, 0);
        std::fs::write(&stale, b"half a checkpoint").unwrap();

        let first = sample();
        write_checkpoint(&path, &first).unwrap();
        assert_eq!(read_checkpoint(&path).unwrap(), first);
        assert_eq!(std::fs::read(&stale).unwrap(), b"half a checkpoint");

        let second = Checkpoint {
            next_sequence: 2_000,
            ..sample()
        };
        write_checkpoint(&path, &second).unwrap();
        assert_eq!(read_checkpoint(&path).unwrap(), second);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(stale).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri cannot rename files.
    fn overlapping_checkpoints_use_independent_temporary_files() {
        let path =
            std::env::temp_dir().join(format!("checkpoint_overlap_{}.bin", std::process::id()));
        let first = sample();
        // Pause the first write after it creates its file, before writing it.
        let (temporary, mut file) = TemporaryCheckpoint::create(&path).unwrap();
        let second = Checkpoint {
            next_sequence: 2_000,
            ..sample()
        };
        write_checkpoint(&path, &second).unwrap();
        assert_eq!(read_checkpoint(&path).unwrap(), second);
        assert!(
            temporary.0.exists(),
            "the second write renamed the first one's file"
        );

        // Resuming the first write must not touch the published checkpoint.
        file.write_all(&encode(&first)).unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert_eq!(read_checkpoint(&path).unwrap(), second);
        std::fs::rename(&temporary.0, &path).unwrap();
        assert_eq!(read_checkpoint(&path).unwrap(), first);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri cannot rename files.
    fn concurrent_checkpoint_writes_publish_complete_files() {
        let path =
            std::env::temp_dir().join(format!("checkpoint_threads_{}.bin", std::process::id()));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        std::thread::scope(|scope| {
            for writer in 0..4 {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for write in 0..8 {
                        let checkpoint = Checkpoint {
                            next_sequence: writer * 8 + write,
                            ..sample()
                        };
                        write_checkpoint(path, &checkpoint).unwrap();
                    }
                });
            }
        });
        let checkpoint = read_checkpoint(&path).unwrap();
        assert!(checkpoint.next_sequence < 32);
        assert_eq!(checkpoint.snapshot, sample().snapshot);
        std::fs::remove_file(path).unwrap();
    }
}
