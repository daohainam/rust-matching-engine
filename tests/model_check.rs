//! Differential test: drives the engine and a deliberately naive reference
//! order book with the same random requests, and requires identical event
//! streams and book state after every request.

mod common;

use std::collections::{BTreeMap, HashMap, VecDeque};

use common::{watched_ticks, Op, OpGenerator};
use matching_engine::engine::MAX_TICKS;
use matching_engine::events::MAX_EVENTS_PER_CALL;
use matching_engine::{
    CancelRejectReason, Engine, Event, NewOrder, OrderType, RejectReason, RestingOrder, Side,
};

/// Resting orders at one price, oldest first: `(order_id, remaining_qty)`.
type Queue = VecDeque<(u64, u64)>;

/// Price-time priority book written for obviousness, not speed.
struct ReferenceBook {
    capacity: usize,
    bids: BTreeMap<u32, Queue>,
    asks: BTreeMap<u32, Queue>,
    live: HashMap<u64, (Side, u32)>,
    /// Account of every order that ever rested; a reused id overwrites it.
    accounts: HashMap<u64, u32>,
    trades: u64,
}

impl ReferenceBook {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            live: HashMap::new(),
            accounts: HashMap::new(),
            trades: 0,
        }
    }

    fn place(&mut self, order: NewOrder) -> Vec<Event> {
        let NewOrder {
            order_id,
            account,
            side,
            order_type,
            price_tick,
            qty,
        } = order;
        let price_tick = match (order_type, side) {
            (OrderType::Market, Side::Buy) => MAX_TICKS as u32 - 1,
            (OrderType::Market, Side::Sell) => 0,
            _ => price_tick,
        };
        let invalid = if qty == 0 {
            Some(RejectReason::ZeroQuantity)
        } else if order_id == u64::MAX {
            Some(RejectReason::ReservedOrderId)
        } else if price_tick as usize >= MAX_TICKS {
            Some(RejectReason::PriceOutOfRange)
        } else if self.live.contains_key(&order_id) {
            Some(RejectReason::DuplicateOrderId)
        } else if order_type == OrderType::PostOnly
            && match side {
                Side::Buy => self.best_ask().is_some_and(|ask| ask <= price_tick),
                Side::Sell => self.best_bid().is_some_and(|bid| bid >= price_tick),
            }
        {
            Some(RejectReason::PostOnlyWouldTrade)
        } else {
            None
        };
        if let Some(reason) = invalid {
            return vec![Event::OrderRejected { order_id, reason }];
        }
        let mut events = vec![Event::OrderAccepted {
            order_id,
            account,
            side,
            order_type,
            price_tick,
            qty,
        }];

        let mut remaining = qty;
        let opposite = match side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        };
        while remaining > 0 {
            let best = match side {
                Side::Buy => opposite.first_key_value(),
                Side::Sell => opposite.last_key_value(),
            };
            let Some((&tick, _)) = best else { break };
            let marketable = match side {
                Side::Buy => tick <= price_tick,
                Side::Sell => tick >= price_tick,
            };
            if !marketable {
                break;
            }
            let queue = opposite.get_mut(&tick).expect("best level exists");
            while remaining > 0 {
                let Some(front) = queue.front_mut() else {
                    break;
                };
                let (maker, maker_qty) = *front;
                let traded = remaining.min(maker_qty);
                remaining -= traded;
                self.trades += 1;
                events.push(Event::Trade {
                    trade_id: self.trades,
                    maker_order_id: maker,
                    taker_order_id: order_id,
                    taker_side: side,
                    price_tick: tick,
                    qty: traded,
                });
                if traded == maker_qty {
                    queue.pop_front();
                    self.live.remove(&maker);
                    events.push(Event::OrderFilled { order_id: maker });
                } else {
                    front.1 -= traded;
                    events.push(Event::OrderPartiallyFilled {
                        order_id: maker,
                        remaining_qty: maker_qty - traded,
                    });
                }
            }
            if queue.is_empty() {
                opposite.remove(&tick);
            }
        }

        if remaining == 0 {
            events.push(Event::OrderFilled { order_id });
            return events;
        }
        if remaining < qty {
            events.push(Event::OrderPartiallyFilled {
                order_id,
                remaining_qty: remaining,
            });
        }
        if matches!(order_type, OrderType::ImmediateOrCancel | OrderType::Market) {
            events.push(Event::OrderExpired {
                order_id,
                expired_qty: remaining,
            });
            return events;
        }
        let own = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        let level_total: u128 = own
            .get(&price_tick)
            .map_or(0, |queue| queue.iter().map(|&(_, q)| u128::from(q)).sum());
        let book_full = self.live.len() >= self.capacity;
        let level_overflows = level_total + u128::from(remaining) > u128::from(u64::MAX);
        if book_full {
            events.push(Event::OrderRejected {
                order_id,
                reason: RejectReason::BookFull,
            });
        } else if level_overflows {
            events.push(Event::OrderRejected {
                order_id,
                reason: RejectReason::LevelQuantityOverflow,
            });
        } else {
            own.entry(price_tick)
                .or_default()
                .push_back((order_id, remaining));
            self.live.insert(order_id, (side, price_tick));
            self.accounts.insert(order_id, account);
        }
        events
    }

    fn cancel(&mut self, order_id: u64) -> (bool, Vec<Event>) {
        let Some((side, tick)) = self.live.remove(&order_id) else {
            let reason = CancelRejectReason::NotResting;
            return (false, vec![Event::CancelRejected { order_id, reason }]);
        };
        let book = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        let queue = book.get_mut(&tick).expect("live order has a level");
        let position = queue
            .iter()
            .position(|&(id, _)| id == order_id)
            .expect("live order is queued at its level");
        let (_, cancelled_qty) = queue.remove(position).expect("position is in range");
        if queue.is_empty() {
            book.remove(&tick);
        }
        let event = Event::OrderCancelled {
            order_id,
            side,
            price_tick: tick,
            cancelled_qty,
        };
        (true, vec![event])
    }

    fn reduce(&mut self, order_id: u64, reduce_by: u64) -> (bool, Vec<Event>) {
        let rejected = |reason| (false, vec![Event::CancelRejected { order_id, reason }]);
        if reduce_by == 0 {
            return rejected(CancelRejectReason::ZeroQuantity);
        }
        let Some(&(side, price_tick)) = self.live.get(&order_id) else {
            return rejected(CancelRejectReason::NotResting);
        };
        let book = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        let (_, qty) = book
            .get_mut(&price_tick)
            .and_then(|queue| queue.iter_mut().find(|(id, _)| *id == order_id))
            .expect("live order is queued at its level");
        if reduce_by >= *qty {
            return self.cancel(order_id);
        }
        *qty -= reduce_by;
        let event = Event::OrderReduced {
            order_id,
            side,
            price_tick,
            reduced_qty: reduce_by,
            remaining_qty: *qty,
        };
        (true, vec![event])
    }

    fn resting_order(&self, order_id: u64) -> Option<RestingOrder> {
        let &(side, price_tick) = self.live.get(&order_id)?;
        let &(_, remaining_qty) = self.book(side)[&price_tick]
            .iter()
            .find(|&&(id, _)| id == order_id)
            .expect("live order is queued at its level");
        Some(RestingOrder {
            order_id,
            account: self.accounts[&order_id],
            side,
            price_tick,
            remaining_qty,
        })
    }

    fn best_bid(&self) -> Option<u32> {
        self.bids.last_key_value().map(|(&tick, _)| tick)
    }

    fn best_ask(&self) -> Option<u32> {
        self.asks.first_key_value().map(|(&tick, _)| tick)
    }

    fn book(&self, side: Side) -> &BTreeMap<u32, Queue> {
        match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        }
    }

    fn depth(&self, side: Side, tick: u32) -> u64 {
        self.book(side)
            .get(&tick)
            .map_or(0, |queue| queue.iter().map(|&(_, q)| q).sum())
    }

    fn snapshot(&self, side: Side) -> Vec<(u32, u64)> {
        let level = |(&tick, queue): (&u32, &Queue)| (tick, queue.iter().map(|&(_, q)| q).sum());
        match side {
            Side::Buy => self.bids.iter().rev().map(level).collect(),
            Side::Sell => self.asks.iter().map(level).collect(),
        }
    }
}

/// Counts of interesting paths hit, so a generator change that stops
/// exercising one of them fails loudly instead of silently testing less.
#[derive(Default)]
struct Coverage {
    input_rejections: usize,
    rest_rejections: usize,
    maker_partial_fills: usize,
    cancels_found: usize,
    calls_beyond_batch_capacity: usize,
    expired_remainders: usize,
    post_only_rejections: usize,
    reductions: usize,
}

fn run(seed: u64, ops: usize, capacity: usize, coverage: &mut Coverage) {
    let mut engine = Engine::new(capacity);
    let mut model = ReferenceBook::new(capacity);
    let mut generator = OpGenerator::new(seed);
    let mut events = Vec::new();

    for step in 0..ops {
        let op = generator.next_op();
        let context = || format!("seed {seed}, step {step}: {op:?}");
        events.clear();
        let expected = match op {
            Op::Place(order) => {
                engine.submit_order(order, &mut events);
                model.place(order)
            }
            Op::Cancel { order_id } => {
                let found = engine.cancel_order(order_id, &mut events);
                let (expected_found, expected) = model.cancel(order_id);
                assert_eq!(found, expected_found, "{}", context());
                coverage.cancels_found += usize::from(found);
                expected
            }
            Op::Reduce {
                order_id,
                reduce_by,
            } => {
                let found = engine.reduce_order(order_id, reduce_by, &mut events);
                let (expected_found, expected) = model.reduce(order_id, reduce_by);
                assert_eq!(found, expected_found, "{}", context());
                expected
            }
        };

        assert_eq!(events, expected, "{}", context());
        assert_eq!(engine.best_bid(), model.best_bid(), "{}", context());
        assert_eq!(engine.best_ask(), model.best_ask(), "{}", context());
        assert_eq!(engine.live_order_count(), model.live.len(), "{}", context());
        for tick in watched_ticks() {
            for side in [Side::Buy, Side::Sell] {
                assert_eq!(
                    engine.depth_at(side, tick),
                    model.depth(side, tick),
                    "{} depth {side:?}@{tick}",
                    context()
                );
            }
        }
        if step % 1_000 == 0 {
            for side in [Side::Buy, Side::Sell] {
                assert_eq!(
                    engine.depth_snapshot(side, usize::MAX),
                    model.snapshot(side),
                    "{}",
                    context()
                );
            }
        }

        let taker_id = match op {
            Op::Place(order) => order.order_id,
            Op::Cancel { order_id } | Op::Reduce { order_id, .. } => order_id,
        };
        // The order this request named, and every maker it touched.
        let touched = events.iter().filter_map(|e| match e {
            Event::Trade { maker_order_id, .. } => Some(*maker_order_id),
            _ => None,
        });
        for order_id in std::iter::once(taker_id).chain(touched) {
            assert_eq!(
                engine.resting_order(order_id),
                model.resting_order(order_id),
                "{} resting order {order_id}",
                context()
            );
        }
        match events.as_slice() {
            [Event::OrderRejected { .. }] => coverage.input_rejections += 1,
            [Event::OrderAccepted { .. }, .., Event::OrderRejected { .. }] => {
                // Chapter 6.4: a remainder can only be rejected if the order
                // did not trade, because every trade frees or empties room.
                assert!(
                    !events.iter().any(|e| matches!(e, Event::Trade { .. })),
                    "{}: an order traded and then had its remainder rejected",
                    context()
                );
                coverage.rest_rejections += 1
            }
            _ => {}
        }
        coverage.maker_partial_fills += events
            .iter()
            .filter(|e| matches!(e, Event::OrderPartiallyFilled { order_id, .. } if *order_id != taker_id))
            .count();
        coverage.calls_beyond_batch_capacity += usize::from(events.len() > MAX_EVENTS_PER_CALL);
        coverage.expired_remainders +=
            usize::from(matches!(events.last(), Some(Event::OrderExpired { .. })));
        coverage.reductions +=
            usize::from(matches!(events.as_slice(), [Event::OrderReduced { .. }]));
        coverage.post_only_rejections += usize::from(matches!(
            events.as_slice(),
            [Event::OrderRejected {
                reason: RejectReason::PostOnlyWouldTrade,
                ..
            }]
        ));
    }
}

fn assert_all_paths_hit(coverage: &Coverage) {
    assert!(coverage.input_rejections > 0, "no input rejections");
    assert!(coverage.rest_rejections > 0, "no rejected remainders");
    assert!(coverage.maker_partial_fills > 0, "no maker partial fills");
    assert!(coverage.cancels_found > 0, "no successful cancels");
    assert!(
        coverage.calls_beyond_batch_capacity > 0,
        "no call produced more than {MAX_EVENTS_PER_CALL} events"
    );
    assert!(coverage.expired_remainders > 0, "no expired remainders");
    assert!(coverage.post_only_rejections > 0, "no post-only rejections");
    assert!(coverage.reductions > 0, "no reductions");
}

#[test]
fn engine_matches_reference_book_when_capacity_is_tight() {
    // A 64-order arena fills constantly, so remainders are often rejected.
    let mut coverage = Coverage::default();
    for seed in 1..=4 {
        run(seed, 20_000, 64, &mut coverage);
    }
    assert_all_paths_hit(&coverage);
}

#[test]
fn engine_matches_reference_book_with_ample_capacity() {
    // Deep books; remainders are rejected only when a level would overflow.
    let mut coverage = Coverage::default();
    for seed in 5..=8 {
        run(seed, 20_000, 8_192, &mut coverage);
    }
    assert_all_paths_hit(&coverage);
}
