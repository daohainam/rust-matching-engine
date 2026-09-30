//! Execution report events emitted by the matching engine, plus a
//! fixed-capacity, heap-allocation-free container used to collect them
//! during a single `process_order` / `cancel_order` call.

pub const MAX_EVENTS_PER_CALL: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A new order was accepted into the book (either fully resting or
    /// about to be matched).
    OrderAccepted { order_id: u64 },
    /// A trade occurred: `maker_order_id` rests in the book, `taker_order_id`
    /// is the incoming order. `price_tick` / `qty` describe the fill.
    Trade {
        maker_order_id: u64,
        taker_order_id: u64,
        price_tick: u32,
        qty: u64,
    },
    /// An order (maker or taker) received a fill but still has remaining
    /// quantity resting in the book.
    OrderPartiallyFilled { order_id: u64, remaining_qty: u64 },
    /// An order was completely filled and removed from the book.
    OrderFilled { order_id: u64 },
    /// An order was cancelled by explicit request.
    OrderCancelled { order_id: u64 },
    /// An order could not be accepted (e.g. arena exhausted or unknown id).
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
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            events: [Event::OrderAccepted { order_id: 0 }; MAX_EVENTS_PER_CALL],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, event: Event) {
        if self.len < MAX_EVENTS_PER_CALL {
            self.events[self.len] = event;
            self.len += 1;
        }
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[Event] {
        &self.events[..self.len]
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

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
