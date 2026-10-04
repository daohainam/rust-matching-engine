//! Execution report events emitted by the matching engine, the [`EventSink`]
//! trait the engine streams them through, and [`EventBatch`], a bounded,
//! heap-allocation-free sink.
//!
//! Three kinds of sink are built in:
//!
//! ```
//! use matching_engine::{Engine, Event, EventBatch, OrderType, Side};
//!
//! let mut engine = Engine::new(16);
//!
//! // 1. A `Vec<Event>` keeps everything. Reuse it (with `clear`) and it stops
//! //    allocating once it has grown to fit the largest call.
//! let mut events = Vec::new();
//! engine.process_order(1, Side::Sell, 100, 5, &mut events);
//! assert_eq!(
//!     events,
//!     [Event::OrderAccepted {
//!         order_id: 1,
//!         side: Side::Sell,
//!         order_type: OrderType::Limit,
//!         price_tick: 100,
//!         qty: 5,
//!     }]
//! );
//!
//! // 2. A closure sees each event as it happens; this one counts trades.
//! let mut trades = 0;
//! engine.process_order(2, Side::Buy, 100, 2, &mut |event: Event| {
//!     if matches!(event, Event::Trade { .. }) {
//!         trades += 1;
//!     }
//! });
//! assert_eq!(trades, 1);
//!
//! // 3. An `EventBatch` never allocates. It keeps the first 64 events of a
//! //    call and only counts the rest, so check `is_truncated` afterwards.
//! let mut batch = EventBatch::new();
//! engine.process_order(3, Side::Buy, 100, 3, &mut batch);
//! assert!(!batch.is_truncated());
//! assert_eq!(batch.as_slice().last(), Some(&Event::OrderFilled { order_id: 3 }));
//! ```

use crate::engine::OrderType;
use crate::pool::Side;

/// Capacity of an [`EventBatch`].
///
/// A batch is a bounded convenience sink: events pushed after this limit are
/// counted by [`EventBatch::dropped`] instead of stored. The engine itself
/// never drops events; callers that need a complete execution report for
/// arbitrarily deep sweeps should use an unbounded sink (a closure, a
/// `Vec<Event>`, a queue) or check [`EventBatch::is_truncated`] after a call.
pub const MAX_EVENTS_PER_CALL: usize = 64;

/// One observable outcome of placing or cancelling an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A new order passed validation and processing began; it may trade
    /// immediately, rest, or later have an un-rested remainder rejected.
    /// It repeats the order's terms, so a consumer of the event stream
    /// alone knows the side, price and size of every order it sees.
    OrderAccepted {
        order_id: u64,
        side: Side,
        order_type: OrderType,
        price_tick: u32,
        qty: u64,
    },
    /// A trade occurred: `maker_order_id` rests in the book, `taker_order_id`
    /// is the incoming order. `price_tick` / `qty` describe the fill, and
    /// `taker_side` is the aggressor's side (the maker is on the other one),
    /// which market data reports with every trade. `trade_id` numbers the
    /// engine's trades 1, 2, 3, ... in the order they happen; it is the same
    /// on every replay of the same requests, so downstream can use it to
    /// spot a duplicate or a gap.
    Trade {
        trade_id: u64,
        maker_order_id: u64,
        taker_order_id: u64,
        taker_side: Side,
        price_tick: u32,
        qty: u64,
    },
    /// An order (maker or taker) received a fill but still has unfilled
    /// quantity. A taker's remainder rests only if the book has capacity.
    OrderPartiallyFilled { order_id: u64, remaining_qty: u64 },
    /// An order has no remaining quantity; a resting maker is removed.
    OrderFilled { order_id: u64 },
    /// The unfilled quantity of an order whose [`OrderType`] does not let it
    /// rest was dropped. It is the order's last event, after any trades,
    /// and `expired_qty` never entered the book.
    OrderExpired { order_id: u64, expired_qty: u64 },
    /// An order was cancelled by explicit request. `side` and `price_tick`
    /// say where it rested and `cancelled_qty` how much quantity left the
    /// book, so a consumer can update its depth from this event alone.
    OrderCancelled {
        order_id: u64,
        side: Side,
        price_tick: u32,
        cancelled_qty: u64,
    },
    /// An order was rejected; `reason` says why. An input error is reported
    /// alone (no `OrderAccepted`); a remainder that cannot rest comes after
    /// `OrderAccepted`. See [`RejectReason`].
    OrderRejected { order_id: u64, reason: RejectReason },
    /// A cancel request named no resting order: the ID was never accepted,
    /// or the order has already been filled, cancelled or had its remainder
    /// rejected. The book is unchanged.
    CancelRejected { order_id: u64 },
}

/// Why an order was rejected.
///
/// The first five reasons are found **before acceptance**: the first four
/// are input errors, and the fifth is a post-only order that would trade.
/// The order is rejected on its own, without an `OrderAccepted` event, and
/// the book is unchanged. The last two can only happen after
/// `OrderAccepted`, when the unfilled remainder of an order cannot rest in
/// the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// The quantity was zero.
    ZeroQuantity,
    /// The order ID was `u64::MAX`, which the ID map reserves to mark an
    /// empty slot (chapter 5).
    ReservedOrderId,
    /// The price tick was not below [`MAX_TICKS`](crate::engine::MAX_TICKS).
    PriceOutOfRange,
    /// A resting order already uses this order ID.
    DuplicateOrderId,
    /// A [`PostOnly`](crate::engine::OrderType::PostOnly) order would have
    /// traded on arrival: its price reaches the best price on the other side.
    PostOnlyWouldTrade,
    /// The remainder could not rest: every order slot was in use.
    BookFull,
    /// The remainder could not rest: the total quantity at its price level
    /// would no longer fit in a `u64`.
    LevelQuantityOverflow,
}

/// Receives the events produced by one engine call, in the order they occur.
///
/// The engine streams every event through this trait rather than collecting
/// them into a fixed-size buffer, so no execution report can be lost. The
/// engine itself never allocates; a call is allocation-free as long as its
/// sink is, as with a reused [`EventBatch`], a `Vec<Event>` with enough
/// reserved capacity, or a closure that forwards into a preallocated queue.
///
/// The engine calls [`emit`](Self::emit) only between complete state
/// changes, so the book is consistent whenever a sink runs. A sink should not
/// panic. If one does and the panic is caught, the book is still consistent,
/// but the request was only partly applied (some trades done, the remainder
/// never rested), so the engine no longer matches a replay of its requests:
/// drop it and rebuild it from the write-ahead log. With `panic = "abort"`
/// (this crate's release profile) a panic ends the process instead, and
/// recovery is a replay anyway.
pub trait EventSink {
    /// Accepts the next event.
    fn emit(&mut self, event: Event);
}

impl<F: FnMut(Event)> EventSink for F {
    #[inline(always)]
    fn emit(&mut self, event: Event) {
        self(event);
    }
}

impl EventSink for Vec<Event> {
    #[inline(always)]
    fn emit(&mut self, event: Event) {
        self.push(event);
    }
}

impl EventSink for EventBatch {
    #[inline(always)]
    fn emit(&mut self, event: Event) {
        self.push(event);
    }
}

/// A fixed-capacity, stack-allocated buffer of [`Event`]s. Pushing beyond
/// [`MAX_EVENTS_PER_CALL`] counts the event as dropped rather than allocating
/// or panicking; [`is_truncated`](Self::is_truncated) reports whether that
/// happened. Reuse one batch across calls (calling [`clear`](Self::clear) in
/// between) to avoid re-initializing its storage every time.
#[derive(Debug, Clone)]
pub struct EventBatch {
    events: [Event; MAX_EVENTS_PER_CALL],
    len: usize,
    dropped: usize,
}

impl Default for EventBatch {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBatch {
    /// Creates an empty batch with storage for [`MAX_EVENTS_PER_CALL`] events.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            events: [Event::OrderFilled { order_id: 0 }; MAX_EVENTS_PER_CALL],
            len: 0,
            dropped: 0,
        }
    }

    /// Appends an event, or counts it as dropped if the batch is full.
    ///
    /// A full batch never allocates or interrupts the engine operation.
    #[inline(always)]
    pub fn push(&mut self, event: Event) {
        if self.len < MAX_EVENTS_PER_CALL {
            self.events[self.len] = event;
            self.len += 1;
        } else {
            self.dropped += 1;
        }
    }

    /// Borrows the events that fit in this batch, in the order they occurred.
    #[inline(always)]
    pub fn as_slice(&self) -> &[Event] {
        &self.events[..self.len]
    }

    /// Returns the number of retained events (at most [`MAX_EVENTS_PER_CALL`]).
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns whether no events are retained.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns how many events did not fit since the last [`clear`](Self::clear).
    #[inline(always)]
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Returns whether any event was dropped since the last [`clear`](Self::clear).
    #[inline(always)]
    pub fn is_truncated(&self) -> bool {
        self.dropped > 0
    }

    /// Makes the batch logically empty and resets the dropped count, without
    /// clearing its fixed storage.
    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
        self.dropped = 0;
    }
}

impl<'a> IntoIterator for &'a EventBatch {
    type Item = &'a Event;
    type IntoIter = std::slice::Iter<'a, Event>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_batch_counts_dropped_events_until_cleared() {
        let mut batch = EventBatch::new();
        for order_id in 0..(MAX_EVENTS_PER_CALL as u64 + 6) {
            batch.emit(Event::OrderFilled { order_id });
        }
        assert_eq!(batch.len(), MAX_EVENTS_PER_CALL);
        assert_eq!(batch.dropped(), 6);
        assert!(batch.is_truncated());
        assert_eq!(
            batch.as_slice().last(),
            Some(&Event::OrderFilled {
                order_id: MAX_EVENTS_PER_CALL as u64 - 1
            })
        );

        batch.clear();
        assert!(batch.is_empty());
        assert!(!batch.is_truncated());
    }

    #[test]
    fn closures_and_vectors_are_sinks() {
        let mut seen = 0;
        let mut count = |_: Event| seen += 1;
        count.emit(Event::OrderFilled { order_id: 1 });
        count.emit(Event::OrderFilled { order_id: 2 });
        assert_eq!(seen, 2);

        let mut events = Vec::new();
        events.emit(Event::OrderFilled { order_id: 3 });
        assert_eq!(events, vec![Event::OrderFilled { order_id: 3 }]);
    }
}
