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
//! # Checkpoint file format (version 1)
//!
//! Little-endian, like the WAL. A 56-byte header:
//!
//! | Offset | Size | Field                                 |
//! |-------:|-----:|---------------------------------------|
//! | 0      | 8    | magic `b"MESNAP\0\0"`                 |
//! | 8      | 4    | format version (`1`)                  |
//! | 12     | 4    | order entry size in bytes (`32`)      |
//! | 16     | 8    | engine capacity                       |
//! | 24     | 8    | accepted orders                       |
//! | 32     | 8    | trades                                |
//! | 40     | 8    | next WAL sequence (see [`Checkpoint`]) |
//! | 48     | 8    | number of orders                      |
//!
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
//! the whole file to `<path>.tmp`, syncs it, renames it over `path` (a
//! rename replaces the target in one step), and on Unix syncs the directory
//! so the rename itself is durable. A crash at any point leaves either the
//! old checkpoint or the new one; at worst a stale `.tmp` file, which the
//! next write replaces.
//!
//! [`Engine::snapshot`]: crate::Engine::snapshot
//! [`Engine::restore`]: crate::Engine::restore

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::engine::RestingOrder;
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
const FORMAT_VERSION: u32 = 1;
const HEADER_SIZE: usize = 56;
const ENTRY_SIZE: usize = 32;
const CHECKSUM_SIZE: usize = 8;

/// Writes `checkpoint` to `path` atomically, replacing any checkpoint
/// already there. See [Writing atomically](self#writing-atomically).
///
/// The format has no tick count field yet: a checkpoint is read back with
/// [`DEFAULT_TICKS`](crate::engine::DEFAULT_TICKS).
pub fn write_checkpoint<P: AsRef<Path>>(path: P, checkpoint: &Checkpoint) -> io::Result<()> {
    let path = path.as_ref();
    let temporary = temporary_path(path);
    {
        let mut file = File::create(&temporary)?;
        file.write_all(&encode(checkpoint))?;
        // The contents must be durable before the rename makes them the
        // checkpoint.
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path)?;
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

/// `<path>.tmp`: where a checkpoint is written before it replaces `path`.
fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
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
        tick_count: _, // not in the format yet (see `write_checkpoint`)
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

    if bytes.len() < HEADER_SIZE + CHECKSUM_SIZE || bytes[..8] != MAGIC {
        return Err(invalid("not a checkpoint file"));
    }
    if u32_at(8) != FORMAT_VERSION || u32_at(12) as usize != ENTRY_SIZE {
        return Err(invalid("unsupported checkpoint format"));
    }
    let count = usize::try_from(u64_at(48)).map_err(|_| invalid("too many orders"))?;
    let expected_len = count
        .checked_mul(ENTRY_SIZE)
        .and_then(|entries| entries.checked_add(HEADER_SIZE + CHECKSUM_SIZE));
    if expected_len != Some(bytes.len()) {
        return Err(invalid("checkpoint file has the wrong length"));
    }
    let body = &bytes[..bytes.len() - CHECKSUM_SIZE];
    if checksum(body) != u64_at(body.len()) {
        return Err(invalid("checkpoint checksum mismatch"));
    }

    let (entries, _) = bytes[HEADER_SIZE..body.len()].as_chunks::<ENTRY_SIZE>();
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
            tick_count: crate::engine::DEFAULT_TICKS,
            accepted_orders: u64_at(24),
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
                tick_count: crate::engine::DEFAULT_TICKS,
                accepted_orders: 1_000,
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
        // A `.tmp` file left by a crash in an earlier write is harmless.
        std::fs::write(temporary_path(&path), b"half a checkpoint").unwrap();

        let first = sample();
        write_checkpoint(&path, &first).unwrap();
        assert_eq!(read_checkpoint(&path).unwrap(), first);
        assert!(!temporary_path(&path).exists());

        let second = Checkpoint {
            next_sequence: 2_000,
            ..sample()
        };
        write_checkpoint(&path, &second).unwrap();
        assert_eq!(read_checkpoint(&path).unwrap(), second);
        std::fs::remove_file(&path).unwrap();
    }
}
