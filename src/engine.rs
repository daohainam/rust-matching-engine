//! The matching engine core: price-time priority limit order book with
//! O(1) place/cancel/match operations and O(1) best-bid/best-ask lookup.
//!
//! This module intentionally performs **no heap allocation** inside
//! [`Engine::process_order`] or [`Engine::cancel_order`]: all storage
//! (order arena, hash map, price-level arrays, bitsets) is allocated once
//! up front in [`Engine::new`], and execution reports are streamed to a
//! caller-supplied [`EventSink`] instead of being collected internally.

use crate::bitset::{TwoLevelBitset, TICKS};
use crate::events::{Event, EventSink, RejectReason};
use crate::hashmap::FlatHashMap;
use crate::pool::{OrderPool, PriceLevel, Side};

/// The maximum representable price tick (exclusive), mirroring the dense
/// tick space tracked by [`TwoLevelBitset`].
pub const MAX_TICKS: usize = TICKS;

/// Which side is the aggressive taker, determining which bitset extremum
/// (lowest vs. highest) and marketability comparison to use when crossing
/// the spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Crossing {
    /// Incoming buy order; crosses resting asks from lowest price upward.
    TakerBuy,
    /// Incoming sell order; crosses resting bids from highest price downward.
    TakerSell,
}

/// A single-threaded, price-time-priority limit order book.
///
/// Orders are stored in a preallocated arena. Each price tick has a FIFO
/// queue for resting orders, while a bitset tracks which ticks are active so
/// the engine can find the best bid and ask without scanning every level. A
/// hash map connects each live order ID to its arena slot for cancellation.
///
/// New orders act as takers: they trade against the best eligible resting
/// orders first, then any unfilled quantity rests on the book if capacity is
/// available. The mutable API keeps matching and cancellation serialized;
/// callers that need concurrency should place synchronization or a queue
/// around the engine rather than access its internal state concurrently.
pub struct Engine {
    pool: OrderPool,
    id_map: FlatHashMap,
    bid_bitset: TwoLevelBitset,
    ask_bitset: TwoLevelBitset,
    bid_levels: Box<[PriceLevel]>,
    ask_levels: Box<[PriceLevel]>,
    /// Counts accepted orders and is stamped into each resting order as
    /// audit metadata. Matching never reads it: time priority comes from the
    /// position in a level's FIFO queue, not from comparing this number.
    sequence: u64,
}

impl Engine {
    /// Creates an empty book that can hold up to `order_capacity` resting orders.
    ///
    /// The order arena, ID map, and fixed-size price-level arrays are prepared
    /// here. This moves their allocation cost to setup instead of order
    /// processing. A resting order consumes one arena slot regardless of its
    /// quantity; fully matched incoming orders do not need a slot.
    ///
    /// The ID map uses a randomly seeded hash, so crafted order IDs cannot
    /// force collisions; matching results never depend on the seed.
    ///
    /// # Panics
    /// Panics if `order_capacity` is zero or does not fit in a `u32` arena
    /// index (`order_capacity >= u32::MAX`).
    pub fn new(order_capacity: usize) -> Self {
        Self {
            pool: OrderPool::new(order_capacity),
            id_map: FlatHashMap::with_capacity(order_capacity),
            bid_bitset: TwoLevelBitset::new(),
            ask_bitset: TwoLevelBitset::new(),
            bid_levels: vec![PriceLevel::empty(); MAX_TICKS].into_boxed_slice(),
            ask_levels: vec![PriceLevel::empty(); MAX_TICKS].into_boxed_slice(),
            sequence: 0,
        }
    }

    /// Returns the highest price tick containing at least one resting bid.
    ///
    /// The bitset summary skips empty 64-tick groups, then a hardware bit scan
    /// selects the highest active tick. Returns `None` when there are no bids.
    #[inline(always)]
    pub fn best_bid(&self) -> Option<u32> {
        self.bid_bitset.highest().map(|t| t as u32)
    }

    /// Returns the lowest price tick containing at least one resting ask.
    ///
    /// The bitset summary skips empty 64-tick groups, then a hardware bit scan
    /// selects the lowest active tick. Returns `None` when there are no asks.
    #[inline(always)]
    pub fn best_ask(&self) -> Option<u32> {
        self.ask_bitset.lowest().map(|t| t as u32)
    }

    /// Returns the total resting quantity at `tick` on the requested `side`.
    ///
    /// This is an aggregate across every order queued at that price. An empty
    /// price level, or a `tick` at or above [`MAX_TICKS`], returns zero.
    #[inline]
    pub fn depth_at(&self, side: Side, tick: u32) -> u64 {
        let levels = match side {
            Side::Buy => &self.bid_levels,
            Side::Sell => &self.ask_levels,
        };
        levels.get(tick as usize).map_or(0, |level| level.total_qty)
    }

    /// Returns up to `levels` non-empty price ticks on `side`, best first.
    ///
    /// Each result contains a price tick and the total quantity resting at
    /// that tick. Bids are ordered from highest to lowest; asks from lowest
    /// to highest. The snapshot owns a newly allocated `Vec`, so this method
    /// is intended for inspection/reporting rather than the allocation-free
    /// order-processing path. Finding each level after the best one searches
    /// the intervening ticks; unlike [`best_bid`](Self::best_bid) and
    /// [`best_ask`](Self::best_ask), the work is not strictly proportional
    /// only to the number of returned levels. At most [`MAX_TICKS`] levels
    /// exist, so a larger `levels` (e.g. `usize::MAX`) returns every level.
    pub fn depth_snapshot(&self, side: Side, levels: usize) -> Vec<(u32, u64)> {
        let mut out = Vec::with_capacity(levels.min(MAX_TICKS));
        match side {
            Side::Buy => {
                let mut tick = self.bid_bitset.highest();
                while let Some(t) = tick {
                    if out.len() >= levels {
                        break;
                    }
                    out.push((t as u32, self.bid_levels[t].total_qty));
                    tick = self.next_lower_active(&self.bid_bitset, t);
                }
            }
            Side::Sell => {
                let mut tick = self.ask_bitset.lowest();
                while let Some(t) = tick {
                    if out.len() >= levels {
                        break;
                    }
                    out.push((t as u32, self.ask_levels[t].total_qty));
                    tick = self.next_higher_active(&self.ask_bitset, t);
                }
            }
        }
        out
    }

    fn next_lower_active(&self, bitset: &TwoLevelBitset, tick: usize) -> Option<usize> {
        // Used for later bid levels after the best bid; inspect only lower
        // ticks and stop as soon as another active one is found.
        (0..tick).rev().find(|&t| bitset.test(t))
    }

    fn next_higher_active(&self, bitset: &TwoLevelBitset, tick: usize) -> Option<usize> {
        // Used for later ask levels after the best ask; inspect only higher
        // ticks and stop as soon as another active one is found.
        (tick + 1..MAX_TICKS).find(|&t| bitset.test(t))
    }

    /// Checks an incoming order before it can touch the book, returning the
    /// first problem found. The checks run in a fixed order, so an order with
    /// several problems always gets the same reason.
    fn validate(&self, order_id: u64, price_tick: u32, qty: u64) -> Result<(), RejectReason> {
        if qty == 0 {
            Err(RejectReason::ZeroQuantity)
        } else if order_id == crate::hashmap::RESERVED_KEY {
            Err(RejectReason::ReservedOrderId)
        } else if (price_tick as usize) >= MAX_TICKS {
            Err(RejectReason::PriceOutOfRange)
        } else if self.id_map.get(order_id).is_some() {
            Err(RejectReason::DuplicateOrderId)
        } else {
            Ok(())
        }
    }

    /// Submits a new limit order.
    ///
    /// The order is first checked for a nonzero quantity, a non-reserved ID,
    /// a valid price tick and an unused ID, and then accepted. A failed check
    /// emits only `OrderRejected`, with the [`RejectReason`]. It trades at resting
    /// maker prices, consuming the best eligible price first and preserving
    /// FIFO order among orders at that price. A buy may trade at asks no
    /// higher than `price_tick`; a sell may trade at bids no lower than it.
    /// Any unfilled quantity is added to the book if an order slot is
    /// available and the price-level quantity can be represented.
    ///
    /// Every event is emitted to `sink` in order: acceptance, then each trade
    /// followed by the maker's fill update, then the taker's final fill or
    /// partial-fill state. If the remainder cannot rest, an `OrderRejected`
    /// event (reason [`BookFull`](RejectReason::BookFull) or
    /// [`LevelQuantityOverflow`](RejectReason::LevelQuantityOverflow)) is
    /// also emitted; trades already completed by this call are not
    /// undone. The engine does not allocate, so the call is allocation-free
    /// whenever `sink` is.
    ///
    /// O(1) amortized to place; each matched maker node is processed in
    /// O(1), so total cost is O(1) + O(fills).
    pub fn process_order<S: EventSink + ?Sized>(
        &mut self,
        order_id: u64,
        side: Side,
        price_tick: u32,
        qty: u64,
        sink: &mut S,
    ) {
        // Reject invalid input before emitting acceptance or changing book state.
        if let Err(reason) = self.validate(order_id, price_tick, qty) {
            sink.emit(Event::OrderRejected { order_id, reason });
            return;
        }
        sink.emit(Event::OrderAccepted {
            order_id,
            side,
            price_tick,
            qty,
        });

        // Matching spends down the incoming quantity; only the remainder can
        // become a new resting order.
        let mut remaining = qty;
        match side {
            // Incoming buy crosses resting asks priced at or below the limit.
            Side::Buy => Self::match_incoming(
                &mut self.pool,
                &mut self.ask_levels,
                &mut self.ask_bitset,
                &mut self.id_map,
                order_id,
                price_tick,
                &mut remaining,
                sink,
                Crossing::TakerBuy,
            ),
            // Incoming sell crosses resting bids priced at or above the limit.
            Side::Sell => Self::match_incoming(
                &mut self.pool,
                &mut self.bid_levels,
                &mut self.bid_bitset,
                &mut self.id_map,
                order_id,
                price_tick,
                &mut remaining,
                sink,
                Crossing::TakerSell,
            ),
        }

        let filled = qty - remaining;
        if remaining == 0 {
            sink.emit(Event::OrderFilled { order_id });
            self.sequence = self.sequence.wrapping_add(1);
            return;
        }

        if filled > 0 {
            sink.emit(Event::OrderPartiallyFilled {
                order_id,
                remaining_qty: remaining,
            });
        }

        // Rest the remainder in the book.
        match self
            .pool
            .allocate(order_id, side, price_tick, remaining, self.sequence)
        {
            Some(idx) => {
                self.id_map.insert(order_id, idx);
                let rested = match side {
                    Side::Buy => {
                        let ok =
                            self.bid_levels[price_tick as usize].push_back(&mut self.pool, idx);
                        if ok {
                            self.bid_bitset.set(price_tick as usize);
                        }
                        ok
                    }
                    Side::Sell => {
                        let ok =
                            self.ask_levels[price_tick as usize].push_back(&mut self.pool, idx);
                        if ok {
                            self.ask_bitset.set(price_tick as usize);
                        }
                        ok
                    }
                };
                if !rested {
                    // Adversarially large quantity would have overflowed
                    // this level's `total_qty` accounting; back out the
                    // allocation entirely rather than resting a corrupt
                    // level.
                    self.id_map.remove(order_id);
                    self.pool.free(idx);
                    sink.emit(Event::OrderRejected {
                        order_id,
                        reason: RejectReason::LevelQuantityOverflow,
                    });
                }
            }
            None => {
                sink.emit(Event::OrderRejected {
                    order_id,
                    reason: RejectReason::BookFull,
                });
            }
        }
        self.sequence = self.sequence.wrapping_add(1);
    }

    /// Generic crossing loop shared by buy/sell taker orders, operating on
    /// the *opposing* side's arena/levels/bitset. `direction` decides
    /// whether we walk the opposing bitset from its lowest (taker is a
    /// buyer crossing resting asks) or highest (taker is a seller crossing
    /// resting bids) active tick, and whether a given tick is marketable
    /// against `price_tick`.
    ///
    /// Takes disjoint field references directly (rather than going through
    /// `&mut Engine` accessor closures) so the borrow checker can see that
    /// `pool`, `levels`, and `bitset` never alias one another.
    #[allow(clippy::too_many_arguments)]
    fn match_incoming<S: EventSink + ?Sized>(
        pool: &mut OrderPool,
        levels: &mut [PriceLevel],
        bitset: &mut TwoLevelBitset,
        id_map: &mut FlatHashMap,
        taker_order_id: u64,
        price_tick: u32,
        remaining: &mut u64,
        sink: &mut S,
        direction: Crossing,
    ) {
        loop {
            if *remaining == 0 {
                break;
            }
            // The best eligible price is always matched first: lowest ask
            // for a buy taker, highest bid for a sell taker.
            let Some(tick) = (match direction {
                Crossing::TakerBuy => bitset.lowest(),
                Crossing::TakerSell => bitset.highest(),
            }) else {
                break;
            };
            let marketable = match direction {
                Crossing::TakerBuy => tick as u32 <= price_tick,
                Crossing::TakerSell => tick as u32 >= price_tick,
            };
            if !marketable {
                // `tick` is the best opposing price. If it cannot cross the
                // limit, every later tick in this direction is worse too.
                break;
            }

            loop {
                if *remaining == 0 {
                    break;
                }
                let level = &mut levels[tick];
                if level.is_empty() {
                    break;
                }
                // Price levels are FIFO queues, so the head is the oldest
                // resting order and receives the next execution.
                let maker_idx = level.head;
                let maker_qty = pool.get(maker_idx).remaining_qty;
                let maker_order_id = pool.get(maker_idx).order_id;
                // A trade can consume the maker, the taker, or both exactly.
                let trade_qty = (*remaining).min(maker_qty);

                sink.emit(Event::Trade {
                    maker_order_id,
                    taker_order_id,
                    price_tick: tick as u32,
                    qty: trade_qty,
                });
                *remaining -= trade_qty;

                if trade_qty == maker_qty {
                    // A fully filled maker leaves every index structure and
                    // its arena slot is returned to the pool's free list.
                    level.remove(pool, maker_idx);
                    pool.free(maker_idx);
                    id_map.remove(maker_order_id);
                    if level.is_empty() {
                        // Drop the price from the summary index *before*
                        // reporting the fill: the sink must only ever see a
                        // consistent book.
                        bitset.clear(tick);
                    }
                    sink.emit(Event::OrderFilled {
                        order_id: maker_order_id,
                    });
                } else {
                    // The maker remains at the head; both its remaining
                    // quantity and the level's aggregate must be reduced.
                    level.reduce_qty(pool, maker_idx, trade_qty);
                    sink.emit(Event::OrderPartiallyFilled {
                        order_id: maker_order_id,
                        remaining_qty: maker_qty - trade_qty,
                    });
                }
            }

            if !levels[tick].is_empty() {
                // Maker side still has quantity (taker fully consumed);
                // nothing more to match at this or any worse tick.
                break;
            }
            // The level drained and its bit is already clear, so the next
            // bitset lookup finds the next best price.
        }
    }

    /// Cancels a resting order by ID.
    ///
    /// The ID map locates the arena slot, then the order is unlinked from its
    /// price-level FIFO queue. If it was the last order at that price, the
    /// price is removed from the active-level bitset. When an order was found
    /// and removed, `OrderCancelled` is emitted to `sink` and `true` is
    /// returned; otherwise nothing is emitted and the result is `false`.
    ///
    /// Lookup and unlink are constant-time in the usual case; hash-table
    /// probing can require additional work when resolving collisions.
    pub fn cancel_order<S: EventSink + ?Sized>(&mut self, order_id: u64, sink: &mut S) -> bool {
        let Some(idx) = self.id_map.remove(order_id) else {
            return false;
        };
        let order = *self.pool.get(idx);
        match order.side {
            Side::Buy => {
                let level = &mut self.bid_levels[order.price_tick as usize];
                level.remove(&mut self.pool, idx);
                if level.is_empty() {
                    self.bid_bitset.clear(order.price_tick as usize);
                }
            }
            Side::Sell => {
                let level = &mut self.ask_levels[order.price_tick as usize];
                level.remove(&mut self.pool, idx);
                if level.is_empty() {
                    self.ask_bitset.clear(order.price_tick as usize);
                }
            }
        }
        self.pool.free(idx);
        sink.emit(Event::OrderCancelled {
            order_id,
            side: order.side,
            price_tick: order.price_tick,
            cancelled_qty: order.remaining_qty,
        });
        true
    }

    /// Returns the number of currently resting orders across both sides.
    #[inline(always)]
    pub fn live_order_count(&self) -> usize {
        self.pool.live_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventBatch, MAX_EVENTS_PER_CALL};

    /// Places an order and returns every event it produced.
    fn place(
        engine: &mut Engine,
        order_id: u64,
        side: Side,
        price_tick: u32,
        qty: u64,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        engine.process_order(order_id, side, price_tick, qty, &mut events);
        events
    }

    /// The acceptance event for an order with these terms.
    fn accepted(order_id: u64, side: Side, price_tick: u32, qty: u64) -> Event {
        Event::OrderAccepted {
            order_id,
            side,
            price_tick,
            qty,
        }
    }

    /// Cancels an order and returns whether it was found plus its events.
    fn cancel(engine: &mut Engine, order_id: u64) -> (bool, Vec<Event>) {
        let mut events = Vec::new();
        let found = engine.cancel_order(order_id, &mut events);
        (found, events)
    }

    #[test]
    fn resting_order_with_no_match() {
        let mut engine = Engine::new(16);
        let events = place(&mut engine, 1, Side::Buy, 100, 10);
        assert_eq!(events, vec![accepted(1, Side::Buy, 100, 10)]);
        assert_eq!(engine.best_bid(), Some(100));
        assert_eq!(engine.best_ask(), None);
        assert_eq!(engine.depth_at(Side::Buy, 100), 10);
    }

    #[test]
    fn acceptance_repeats_the_order_terms_even_when_it_fills_at_once() {
        // The event stream alone must say what was ordered: a consumer that
        // never sees the request still learns side, limit and size.
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 100, 3);
        let events = place(&mut engine, 2, Side::Sell, 99, 3);
        assert_eq!(events.first(), Some(&accepted(2, Side::Sell, 99, 3)));
        assert_eq!(events.last(), Some(&Event::OrderFilled { order_id: 2 }));
    }

    #[test]
    fn full_fill_taker_and_maker() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 10);
        let events = place(&mut engine, 2, Side::Buy, 100, 10);
        assert_eq!(
            events,
            vec![
                accepted(2, Side::Buy, 100, 10),
                Event::Trade {
                    maker_order_id: 1,
                    taker_order_id: 2,
                    price_tick: 100,
                    qty: 10
                },
                Event::OrderFilled { order_id: 1 },
                Event::OrderFilled { order_id: 2 },
            ]
        );
        assert_eq!(engine.best_bid(), None);
        assert_eq!(engine.best_ask(), None);
        assert_eq!(engine.live_order_count(), 0);
    }

    #[test]
    fn partial_fill_leaves_maker_resting() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 10);
        let events = place(&mut engine, 2, Side::Buy, 100, 4);
        assert_eq!(
            events,
            vec![
                accepted(2, Side::Buy, 100, 4),
                Event::Trade {
                    maker_order_id: 1,
                    taker_order_id: 2,
                    price_tick: 100,
                    qty: 4
                },
                Event::OrderPartiallyFilled {
                    order_id: 1,
                    remaining_qty: 6
                },
                Event::OrderFilled { order_id: 2 },
            ]
        );
        assert_eq!(engine.best_ask(), Some(100));
        assert_eq!(engine.depth_at(Side::Sell, 100), 6);
    }

    #[test]
    fn partial_fill_leaves_taker_resting_remainder() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 4);
        let events = place(&mut engine, 2, Side::Buy, 100, 10);
        assert_eq!(
            events,
            vec![
                accepted(2, Side::Buy, 100, 10),
                Event::Trade {
                    maker_order_id: 1,
                    taker_order_id: 2,
                    price_tick: 100,
                    qty: 4
                },
                Event::OrderFilled { order_id: 1 },
                Event::OrderPartiallyFilled {
                    order_id: 2,
                    remaining_qty: 6
                },
            ]
        );
        assert_eq!(engine.best_bid(), Some(100));
        assert_eq!(engine.best_ask(), None);
        assert_eq!(engine.depth_at(Side::Buy, 100), 6);
    }

    #[test]
    fn cancel_head_middle_tail_of_price_queue() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 50, 1);
        place(&mut engine, 2, Side::Buy, 50, 1);
        place(&mut engine, 3, Side::Buy, 50, 1);
        assert_eq!(engine.depth_at(Side::Buy, 50), 3);

        // Cancel middle.
        let (ok, events) = cancel(&mut engine, 2);
        assert!(ok);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderCancelled {
                order_id: 2,
                side: Side::Buy,
                price_tick: 50,
                cancelled_qty: 1
            }]
        );
        assert_eq!(engine.depth_at(Side::Buy, 50), 2);

        // Cancel head.
        let (ok, _) = cancel(&mut engine, 1);
        assert!(ok);
        assert_eq!(engine.depth_at(Side::Buy, 50), 1);
        assert_eq!(engine.best_bid(), Some(50));

        // Cancel tail (last remaining order at this level).
        let (ok, _) = cancel(&mut engine, 3);
        assert!(ok);
        assert_eq!(engine.best_bid(), None);
        assert_eq!(engine.live_order_count(), 0);

        // Cancelling again fails cleanly.
        let (ok, events) = cancel(&mut engine, 3);
        assert!(!ok);
        assert!(events.is_empty());
    }

    #[test]
    fn cancel_reports_only_what_is_left_after_a_partial_fill() {
        // A consumer that applies every event to its own depth view must end
        // with the engine's depth: the cancel removes the 6 left, not the 10
        // originally ordered.
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 120, 10);
        place(&mut engine, 2, Side::Buy, 120, 4);
        let (ok, events) = cancel(&mut engine, 1);
        assert!(ok);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderCancelled {
                order_id: 1,
                side: Side::Sell,
                price_tick: 120,
                cancelled_qty: 6
            }]
        );
        assert_eq!(engine.depth_at(Side::Sell, 120), 0);
    }

    #[test]
    fn crosses_multiple_ask_price_levels_in_price_time_priority() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 5);
        place(&mut engine, 2, Side::Sell, 101, 5);
        place(&mut engine, 3, Side::Sell, 102, 5);

        // Aggressive buy sweeps three levels: 100, 101, and partially 102.
        let events = place(&mut engine, 4, Side::Buy, 102, 12);
        let trades: Vec<_> = events
            .as_slice()
            .iter()
            .filter_map(|e| match e {
                Event::Trade {
                    price_tick, qty, ..
                } => Some((*price_tick, *qty)),
                _ => None,
            })
            .collect();
        assert_eq!(trades, vec![(100, 5), (101, 5), (102, 2)]);
        assert_eq!(engine.best_ask(), Some(102));
        assert_eq!(engine.depth_at(Side::Sell, 102), 3);
        assert_eq!(engine.best_bid(), None); // taker fully filled, nothing rests
    }

    #[test]
    fn crosses_multiple_bid_price_levels_for_incoming_sell() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 102, 5);
        place(&mut engine, 2, Side::Buy, 101, 5);
        place(&mut engine, 3, Side::Buy, 100, 5);

        let events = place(&mut engine, 4, Side::Sell, 100, 12);
        let trades: Vec<_> = events
            .as_slice()
            .iter()
            .filter_map(|e| match e {
                Event::Trade {
                    price_tick, qty, ..
                } => Some((*price_tick, *qty)),
                _ => None,
            })
            .collect();
        assert_eq!(trades, vec![(102, 5), (101, 5), (100, 2)]);
        assert_eq!(engine.best_bid(), Some(100));
        assert_eq!(engine.depth_at(Side::Buy, 100), 3);
    }

    #[test]
    fn price_time_priority_within_a_level_is_fifo() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 5);
        place(&mut engine, 2, Side::Sell, 100, 5);

        let events = place(&mut engine, 3, Side::Buy, 100, 6);
        let fills: Vec<_> = events
            .as_slice()
            .iter()
            .filter_map(|e| match e {
                Event::Trade { maker_order_id, .. } => Some(*maker_order_id),
                _ => None,
            })
            .collect();
        assert_eq!(fills, vec![1, 2]); // order 1 (first in) fills before order 2
    }

    #[test]
    fn rejects_duplicate_order_id_and_zero_qty() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 100, 10);
        let events = place(&mut engine, 1, Side::Buy, 100, 5);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderRejected {
                order_id: 1,
                reason: RejectReason::DuplicateOrderId
            }]
        );

        let events = place(&mut engine, 2, Side::Buy, 100, 0);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderRejected {
                order_id: 2,
                reason: RejectReason::ZeroQuantity
            }]
        );
    }

    #[test]
    fn rejects_out_of_range_price_tick() {
        let mut engine = Engine::new(16);
        let events = place(&mut engine, 1, Side::Sell, MAX_TICKS as u32, 5);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderRejected {
                order_id: 1,
                reason: RejectReason::PriceOutOfRange
            }]
        );
        assert_eq!(engine.best_ask(), None);
    }

    #[test]
    fn input_checks_run_in_a_fixed_order() {
        // Zero quantity, reserved id and bad tick at once: the first check wins.
        let mut engine = Engine::new(16);
        let events = place(&mut engine, u64::MAX, Side::Buy, u32::MAX, 0);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderRejected {
                order_id: u64::MAX,
                reason: RejectReason::ZeroQuantity
            }]
        );
    }

    #[test]
    fn depth_snapshot_orders_best_to_worst() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 100, 1);
        place(&mut engine, 2, Side::Buy, 105, 2);
        place(&mut engine, 3, Side::Buy, 102, 3);

        let snapshot = engine.depth_snapshot(Side::Buy, 10);
        assert_eq!(snapshot, vec![(105, 2), (102, 3), (100, 1)]);
    }

    #[test]
    fn arena_exhaustion_rejects_resting_order() {
        let mut engine = Engine::new(2);
        place(&mut engine, 1, Side::Buy, 100, 1);
        place(&mut engine, 2, Side::Buy, 101, 1);
        // Arena is full; a third non-matching order can't be rested.
        let events = place(&mut engine, 3, Side::Buy, 99, 1);
        assert_eq!(
            events.as_slice(),
            &[
                accepted(3, Side::Buy, 99, 1),
                Event::OrderRejected {
                    order_id: 3,
                    reason: RejectReason::BookFull
                }
            ]
        );
        assert_eq!(engine.live_order_count(), 2);
    }

    #[test]
    fn rejects_reserved_order_id_sentinel() {
        // u64::MAX is reserved internally as the flat hash map's "empty
        // slot" sentinel; accepting it as a real order id would make the
        // order permanently unreachable via cancel/duplicate-detection
        // while still consuming a pool slot forever (a resource-exhaustion
        // vector for an attacker who just picks order_id = u64::MAX).
        let mut engine = Engine::new(16);
        let events = place(&mut engine, u64::MAX, Side::Buy, 100, 10);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderRejected {
                order_id: u64::MAX,
                reason: RejectReason::ReservedOrderId
            }]
        );
        assert_eq!(engine.live_order_count(), 0);
        assert_eq!(engine.best_bid(), None);

        // Cancelling the reserved id must also fail cleanly, never panic.
        let (ok, events) = cancel(&mut engine, u64::MAX);
        assert!(!ok);
        assert!(events.is_empty());
    }

    #[test]
    fn rejects_resting_order_that_would_overflow_level_total_qty() {
        // An adversarially large quantity must not silently wrap
        // `PriceLevel::total_qty` around u64::MAX; the second order should
        // be rejected and its pool slot released rather than corrupting the
        // level's bookkeeping.
        let mut engine = Engine::new(16);
        let events = place(&mut engine, 1, Side::Buy, 100, u64::MAX);
        assert_eq!(events.as_slice(), &[accepted(1, Side::Buy, 100, u64::MAX)]);
        assert_eq!(engine.depth_at(Side::Buy, 100), u64::MAX);
        assert_eq!(engine.live_order_count(), 1);

        let events = place(&mut engine, 2, Side::Buy, 100, 1);
        assert_eq!(
            events.as_slice(),
            &[
                accepted(2, Side::Buy, 100, 1),
                Event::OrderRejected {
                    order_id: 2,
                    reason: RejectReason::LevelQuantityOverflow
                }
            ]
        );
        // The rejected order must not have left a dangling pool slot or
        // corrupted the resting level's depth.
        assert_eq!(engine.live_order_count(), 1);
        assert_eq!(engine.depth_at(Side::Buy, 100), u64::MAX);

        let (ok, _) = cancel(&mut engine, 2);
        assert!(!ok, "rejected order must not have been inserted");
    }

    #[test]
    fn deep_sweep_reports_every_execution() {
        // Regression: a fixed 64-event buffer used to drop every report after
        // the 32nd fully filled maker although the book had executed them.
        let mut engine = Engine::new(128);
        for maker in 0..40u64 {
            place(&mut engine, maker, Side::Sell, 100, 1);
        }

        let events = place(&mut engine, 1_000, Side::Buy, 100, 40);
        let trades = events
            .iter()
            .filter(|e| matches!(e, Event::Trade { .. }))
            .count();
        let maker_fills = events
            .iter()
            .filter(|e| matches!(e, Event::OrderFilled { order_id } if *order_id < 40))
            .count();
        assert_eq!(trades, 40);
        assert_eq!(maker_fills, 40);
        assert_eq!(events.first(), Some(&accepted(1_000, Side::Buy, 100, 40)));
        assert_eq!(events.last(), Some(&Event::OrderFilled { order_id: 1_000 }));
        assert_eq!(engine.live_order_count(), 0);
    }

    #[test]
    fn bounded_batch_sink_reports_truncation() {
        let mut engine = Engine::new(128);
        for maker in 0..40u64 {
            place(&mut engine, maker, Side::Sell, 100, 1);
        }

        let mut batch = EventBatch::new();
        engine.process_order(1_000, Side::Buy, 100, 40, &mut batch);
        // Accepted + 40 x (Trade, maker OrderFilled) + taker OrderFilled.
        assert_eq!(batch.len() + batch.dropped(), 82);
        assert_eq!(batch.len(), MAX_EVENTS_PER_CALL);
        assert!(batch.is_truncated());
    }

    #[test]
    fn panicking_sink_leaves_the_book_consistent() {
        // `emit` is only called between complete state changes. A sink that
        // panics on the maker's fill must therefore not leave the drained
        // price in the bitset (which would make `best_ask` report an empty
        // level). `AssertUnwindSafe` is needed because `&mut Engine` is not
        // `UnwindSafe`: Rust warns that state may be half-updated.
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 5);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine.process_order(2, Side::Buy, 100, 5, &mut |event: Event| {
                if event == (Event::OrderFilled { order_id: 1 }) {
                    panic!("sink failed");
                }
            });
        }));
        assert!(result.is_err());
        assert_eq!(engine.best_ask(), None);
        assert!(engine.depth_snapshot(Side::Sell, usize::MAX).is_empty());
        assert_eq!(engine.live_order_count(), 0);
    }

    #[test]
    fn depth_queries_accept_any_input_without_panicking() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 100, 5);
        place(&mut engine, 2, Side::Sell, 200, 7);

        assert_eq!(engine.depth_at(Side::Buy, MAX_TICKS as u32), 0);
        assert_eq!(engine.depth_at(Side::Sell, u32::MAX), 0);
        assert_eq!(engine.depth_snapshot(Side::Buy, usize::MAX), vec![(100, 5)]);
        assert_eq!(
            engine.depth_snapshot(Side::Sell, usize::MAX),
            vec![(200, 7)]
        );
    }
}
