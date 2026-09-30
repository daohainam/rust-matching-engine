//! Execution report events emitted by the matching engine, plus a
//! fixed-capacity, heap-allocation-free container used to collect them
//! during a single `process_order` / `cancel_order` call.

/// Maximum number of events retained from one engine call.
///
/// A call that produces more events still completes its matching work, but
/// events after this limit are dropped by [`EventBatch::push`]. Consumers
/// that require a complete execution report should size this limit for their
/// workload and monitor the maximum number of fills per call.
pub const MAX_EVENTS_PER_CALL: usize = 64;

/// One observable outcome of placing or cancelling an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A new order passed validation and processing began; it may trade
    /// immediately, rest, or later have an un-rested remainder rejected.
    OrderAccepted { order_id: u64 },
    /// A trade occurred: `maker_order_id` rests in the book, `taker_order_id`
    /// is the incoming order. `price_tick` / `qty` describe the fill.
    Trade {
        maker_order_id: u64,
        taker_order_id: u64,
        price_tick: u32,
        qty: u64,
    },
    /// An order (maker or taker) received a fill but still has unfilled
    /// quantity. A taker's remainder rests only if the book has capacity.
    OrderPartiallyFilled { order_id: u64, remaining_qty: u64 },
    /// An order has no remaining quantity; a resting maker is removed.
    OrderFilled { order_id: u64 },
    /// An order was cancelled by explicit request.
    OrderCancelled { order_id: u64 },
    /// An order was rejected (for example, invalid input, duplicate ID,
    /// exhausted arena capacity, or price-level quantity overflow).
    OrderRejected { order_id: u64 },
}

/// A fixed-capacity, stack-allocated buffer of [`Event`]s. Pushing beyond
/// `MAX_EVENTS_PER_CALL` silently drops the event rather than allocating or
/// panicking, keeping the hot path allocation-free and panic-free; callers
/// that need more headroom can raise `MAX_EVENTS_PER_CALL`.
#[derive(Debug, Clone, Copy)]
pub struct EventBatch {
    events: [Event; MAX_EVENTS_PER_CALL],
    len: usize,
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
            events: [Event::OrderAccepted { order_id: 0 }; MAX_EVENTS_PER_CALL],
            len: 0,
        }
    }

    /// Appends an event unless the fixed-capacity buffer is already full.
    ///
    /// A full buffer silently drops the new event; it never allocates or
    /// interrupts the engine operation.
    #[inline(always)]
    pub fn push(&mut self, event: Event) {
        if self.len < MAX_EVENTS_PER_CALL {
            self.events[self.len] = event;
            self.len += 1;
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

    /// Makes the batch logically empty without clearing its fixed storage.
    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl<'a> IntoIterator for &'a EventBatch {
    type Item = &'a Event;
    type IntoIter = std::slice::Iter<'a, Event>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}
