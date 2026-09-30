//! Execution report events emitted by the matching engine, the [`EventSink`]
//! trait the engine streams them through, and [`EventBatch`], a bounded,
//! heap-allocation-free sink.

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

/// Receives the events produced by one engine call, in the order they occur.
///
/// The engine streams every event through this trait rather than collecting
/// them into a fixed-size buffer, so no execution report can be lost. The
/// engine itself never allocates; a call is allocation-free as long as its
/// sink is, as with a reused [`EventBatch`], a `Vec<Event>` with enough
/// reserved capacity, or a closure that forwards into a preallocated queue.
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
            events: [Event::OrderAccepted { order_id: 0 }; MAX_EVENTS_PER_CALL],
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
            batch.emit(Event::OrderAccepted { order_id });
        }
        assert_eq!(batch.len(), MAX_EVENTS_PER_CALL);
        assert_eq!(batch.dropped(), 6);
        assert!(batch.is_truncated());
        assert_eq!(
            batch.as_slice().last(),
            Some(&Event::OrderAccepted {
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
        events.emit(Event::OrderCancelled { order_id: 3 });
        assert_eq!(events, vec![Event::OrderCancelled { order_id: 3 }]);
    }
}
