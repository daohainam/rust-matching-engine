//! Snapshots of an engine's complete state.
//!
//! The write-ahead log rebuilds a book by replaying every request since the
//! log began, so recovery gets slower the longer the engine runs. A
//! [`Snapshot`] captures the state itself: [`Engine::snapshot`] takes one,
//! and [`Engine::restore`] builds an engine that behaves exactly like the
//! original from that point on. Replaying only the requests logged after
//! the snapshot then finishes the recovery.
//!
//! [`Engine::snapshot`]: crate::Engine::snapshot
//! [`Engine::restore`]: crate::Engine::restore

use crate::engine::RestingOrder;
use crate::events::RejectReason;

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
