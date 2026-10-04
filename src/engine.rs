//! The matching engine core: price-time priority limit order book with
//! O(1) place/cancel/match operations and O(1) best-bid/best-ask lookup.
//!
//! This module intentionally performs **no heap allocation** inside
//! [`Engine::process_order`] or [`Engine::cancel_order`]: all storage
//! (order arena, hash map, price-level arrays, bitsets) is allocated once
//! up front in [`Engine::new`], and execution reports are streamed to a
//! caller-supplied [`EventSink`] instead of being collected internally.

use crate::bitset::{TickBitset, MAX_LADDER_TICKS};
use crate::events::{CancelRejectReason, Event, EventSink, RejectReason};
use crate::hashmap::FlatHashMap;
use crate::pool::{OrderPool, PriceLevel, Side, NIL};
use crate::snapshot::{Snapshot, SnapshotError};

/// The number of price ticks of an engine made with [`Engine::new`]: valid
/// ticks are `0..4096`. For this size the [`TickBitset`] has two levels.
/// [`Engine::with_ticks`] chooses another number.
pub const DEFAULT_TICKS: usize = 4096;

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

/// A copy of one resting order's current state, returned by
/// [`Engine::resting_order`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestingOrder {
    /// The caller's identifier for the order.
    pub order_id: u64,
    /// The account that owns the order.
    pub account: u32,
    /// The side of the book it rests on.
    pub side: Side,
    /// The price tick it rests at.
    pub price_tick: u32,
    /// Quantity not yet executed; less than ordered after partial fills.
    pub remaining_qty: u64,
}

/// What an order does with quantity it cannot trade on arrival.
///
/// Every type trades the same way: against the best resting prices first,
/// at the maker's price, oldest maker first at each price. The types differ
/// only in what happens to the remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    /// Trades at `price_tick` or better; the remainder rests in the book
    /// (good-till-cancelled).
    Limit,
    /// Immediate-or-cancel: trades at `price_tick` or better; the remainder
    /// expires instead of resting.
    ImmediateOrCancel,
    /// Trades at any price; the remainder expires. The `price_tick` passed
    /// in is ignored: a market buy gets the highest tick as its limit and a
    /// market sell tick 0, and `OrderAccepted` reports that limit.
    Market,
    /// Must not trade on arrival: rejected with
    /// [`PostOnlyWouldTrade`](RejectReason::PostOnlyWouldTrade) if its price
    /// reaches the other side's best price, otherwise rests like a limit
    /// order. Market makers use it to be sure they only ever add liquidity.
    PostOnly,
}

impl OrderType {
    /// Whether unfilled quantity may rest in the book.
    #[inline(always)]
    fn rests(self) -> bool {
        matches!(self, OrderType::Limit | OrderType::PostOnly)
    }
}

/// What happens when an incoming order would trade with a resting order of
/// its own account. The incoming order's setting decides.
///
/// Trading with yourself moves no risk, but it prints a trade that other
/// traders see, and regulators treat deliberate self-trades as market
/// manipulation (wash trading), so venues let traders block them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfTradePrevention {
    /// Trade as usual: no prevention. The default.
    Allow,
    /// Cancel the resting order and keep matching ("cancel oldest").
    CancelResting,
    /// Stop matching; the incoming order's remainder expires, even for a
    /// limit order ("cancel newest").
    CancelIncoming,
    /// Cancel the resting order, stop matching, and let the incoming
    /// order's remainder expire.
    CancelBoth,
}

impl SelfTradePrevention {
    /// Whether the resting order of a prevented self-trade is cancelled.
    #[inline(always)]
    fn cancels_resting(self) -> bool {
        matches!(self, Self::CancelResting | Self::CancelBoth)
    }

    /// Whether a prevented self-trade stops the incoming order.
    #[inline(always)]
    fn stops_incoming(self) -> bool {
        matches!(self, Self::CancelIncoming | Self::CancelBoth)
    }
}

/// Pre-trade risk limits: checks every new order must pass before it may
/// trade. They catch "fat finger" mistakes, such as an extra zero on the
/// quantity or a price far through the market, before they cost anything.
///
/// Limits are engine state, set with [`Engine::set_risk_limits`]. They
/// change the outcome of later orders, so a change must be logged like any
/// other request for a replay to reproduce the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskLimits {
    /// The largest quantity one order may have; a larger order is rejected
    /// with [`QuantityAboveLimit`](RejectReason::QuantityAboveLimit).
    pub max_order_qty: u64,
    /// How many ticks past the best opposing price an order may reach: a
    /// buy may be priced at most `best ask + band`, a sell at least
    /// `best bid - band`, or it is rejected with
    /// [`PriceOutsideBand`](RejectReason::PriceOutsideBand). A market order
    /// gets exactly that price as its limit, so it can sweep no further.
    /// With no opposing order there is no reference, and no band applies.
    /// `None` means no band at all.
    pub price_band: Option<u32>,
}

impl RiskLimits {
    /// No limits: any quantity, any price. The default.
    pub const NONE: Self = Self {
        max_order_qty: u64::MAX,
        price_band: None,
    };
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self::NONE
    }
}

/// A request to submit an order, as [`Engine::submit_order`] takes it.
///
/// Start from [`limit`](Self::limit) or [`market`](Self::market) and adjust
/// with [`with_type`](Self::with_type), [`with_account`](Self::with_account)
/// and [`with_self_trade_prevention`](Self::with_self_trade_prevention);
/// every field is public, so struct update syntax works too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewOrder {
    /// The caller's identifier, unique among resting orders.
    pub order_id: u64,
    /// The account placing the order: a trader, a firm, or whatever unit
    /// the caller tracks. The engine stores and reports it; `0` is an
    /// ordinary account like any other.
    pub account: u32,
    /// Buy or sell.
    pub side: Side,
    /// What happens to quantity that cannot trade at once.
    pub order_type: OrderType,
    /// The limit price; ignored for a market order.
    pub price_tick: u32,
    /// How much to trade.
    pub qty: u64,
    /// What to do on meeting a resting order of the same account.
    pub self_trade_prevention: SelfTradePrevention,
}

impl NewOrder {
    /// A limit order: trades at `price_tick` or better, then rests.
    pub const fn limit(order_id: u64, side: Side, price_tick: u32, qty: u64) -> Self {
        Self {
            order_id,
            account: 0,
            side,
            order_type: OrderType::Limit,
            price_tick,
            qty,
            self_trade_prevention: SelfTradePrevention::Allow,
        }
    }

    /// A market order: trades at any price, and the rest expires.
    pub const fn market(order_id: u64, side: Side, qty: u64) -> Self {
        Self::limit(order_id, side, 0, qty).with_type(OrderType::Market)
    }

    /// The same order with another [`OrderType`].
    pub const fn with_type(self, order_type: OrderType) -> Self {
        Self { order_type, ..self }
    }

    /// The same order for another account.
    pub const fn with_account(self, account: u32) -> Self {
        Self { account, ..self }
    }

    /// The same order with another [`SelfTradePrevention`] mode.
    pub const fn with_self_trade_prevention(self, mode: SelfTradePrevention) -> Self {
        Self {
            self_trade_prevention: mode,
            ..self
        }
    }
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
    bid_bitset: TickBitset,
    ask_bitset: TickBitset,
    bid_levels: Box<[PriceLevel]>,
    ask_levels: Box<[PriceLevel]>,
    /// Counts accepted orders and is stamped into each resting order as
    /// audit metadata. Matching never reads it: time priority comes from the
    /// position in a level's FIFO queue, not from comparing this number.
    sequence: u64,
    /// Number of trades executed so far; the last one's `trade_id`.
    trades: u64,
    /// The checks every new order must pass.
    limits: RiskLimits,
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
    /// The book has [`DEFAULT_TICKS`] (4096) price ticks; use
    /// [`with_ticks`](Self::with_ticks) for another number.
    ///
    /// # Panics
    /// Panics if `order_capacity` is zero or does not fit in a `u32` arena
    /// index (`order_capacity >= u32::MAX`).
    pub fn new(order_capacity: usize) -> Self {
        Self::with_ticks(order_capacity, DEFAULT_TICKS)
    }

    /// Creates an empty book for up to `order_capacity` resting orders, with
    /// price ticks `0..ticks`.
    ///
    /// Choose `ticks` for the instrument: the price range divided by the
    /// tick size. Memory grows with it, two price levels of 24 bytes per
    /// tick, so 4096 ticks take 196 KB and 262,144 ticks 12.6 MB; every
    /// operation stays O(1), with one more bit-scan for every factor of 64.
    ///
    /// # Panics
    /// Panics if `order_capacity` is invalid (see [`new`](Self::new)), or if
    /// `ticks` is zero or above [`MAX_LADDER_TICKS`] (2^24).
    pub fn with_ticks(order_capacity: usize, ticks: usize) -> Self {
        Self {
            pool: OrderPool::new(order_capacity),
            id_map: FlatHashMap::with_capacity(order_capacity),
            bid_bitset: TickBitset::new(ticks),
            ask_bitset: TickBitset::new(ticks),
            bid_levels: vec![PriceLevel::empty(); ticks].into_boxed_slice(),
            ask_levels: vec![PriceLevel::empty(); ticks].into_boxed_slice(),
            sequence: 0,
            trades: 0,
            limits: RiskLimits::NONE,
        }
    }

    /// Returns the pre-trade risk limits new orders are checked against.
    #[inline(always)]
    pub fn risk_limits(&self) -> RiskLimits {
        self.limits
    }

    /// Replaces the pre-trade risk limits; orders already resting are not
    /// affected. Like an order, this is a request that changes later
    /// results: log it, so that a replay applies it at the same point.
    pub fn set_risk_limits(&mut self, limits: RiskLimits) {
        self.limits = limits;
    }

    /// Returns how many price ticks the book has: valid ticks are
    /// `0..tick_count()`.
    #[inline(always)]
    pub fn tick_count(&self) -> usize {
        self.bid_bitset.ticks()
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
    /// price level, or a `tick` outside the book's range, returns zero.
    #[inline]
    pub fn depth_at(&self, side: Side, tick: u32) -> u64 {
        let levels = match side {
            Side::Buy => &self.bid_levels,
            Side::Sell => &self.ask_levels,
        };
        levels.get(tick as usize).map_or(0, |level| level.total_qty)
    }

    /// Looks up an order that is resting in the book.
    ///
    /// Returns `None` if no resting order has this ID: it was never
    /// accepted, has been filled or cancelled, or its remainder was
    /// rejected. A hash lookup, like the one [`cancel_order`](Self::cancel_order)
    /// starts with; nothing is allocated.
    #[inline]
    pub fn resting_order(&self, order_id: u64) -> Option<RestingOrder> {
        let idx = self.id_map.get(order_id)?;
        let order = self.pool.get(idx);
        Some(RestingOrder {
            order_id,
            account: order.account,
            side: order.side,
            price_tick: order.price_tick,
            remaining_qty: order.remaining_qty,
        })
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
    /// only to the number of returned levels. At most
    /// [`tick_count`](Self::tick_count) levels exist, so a larger `levels`
    /// (e.g. `usize::MAX`) returns every level.
    pub fn depth_snapshot(&self, side: Side, levels: usize) -> Vec<(u32, u64)> {
        let mut out = Vec::with_capacity(levels.min(self.tick_count()));
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

    fn next_lower_active(&self, bitset: &TickBitset, tick: usize) -> Option<usize> {
        // Used for later bid levels after the best bid; inspect only lower
        // ticks and stop as soon as another active one is found.
        (0..tick).rev().find(|&t| bitset.test(t))
    }

    fn next_higher_active(&self, bitset: &TickBitset, tick: usize) -> Option<usize> {
        // Used for later ask levels after the best ask; inspect only higher
        // ticks and stop as soon as another active one is found.
        (tick + 1..self.tick_count()).find(|&t| bitset.test(t))
    }

    /// Checks an incoming order before it can touch the book, returning the
    /// first problem found. The checks run in a fixed order, so an order with
    /// several problems always gets the same reason.
    fn validate(
        &self,
        order_id: u64,
        side: Side,
        order_type: OrderType,
        price_tick: u32,
        qty: u64,
    ) -> Result<(), RejectReason> {
        if qty == 0 {
            Err(RejectReason::ZeroQuantity)
        } else if order_id == crate::hashmap::RESERVED_KEY {
            Err(RejectReason::ReservedOrderId)
        } else if (price_tick as usize) >= self.tick_count() {
            Err(RejectReason::PriceOutOfRange)
        } else if self.id_map.get(order_id).is_some() {
            Err(RejectReason::DuplicateOrderId)
        } else if qty > self.limits.max_order_qty {
            Err(RejectReason::QuantityAboveLimit)
        } else if self.is_outside_band(side, price_tick) {
            Err(RejectReason::PriceOutsideBand)
        } else if order_type == OrderType::PostOnly && self.is_marketable(side, price_tick) {
            Err(RejectReason::PostOnlyWouldTrade)
        } else {
            Ok(())
        }
    }

    /// Whether an order on `side` limited at `price_tick` reaches more than
    /// the price band past the best opposing price.
    fn is_outside_band(&self, side: Side, price_tick: u32) -> bool {
        let Some(band) = self.limits.price_band else {
            return false;
        };
        match side {
            Side::Buy => self
                .best_ask()
                .is_some_and(|ask| price_tick > ask.saturating_add(band)),
            Side::Sell => self
                .best_bid()
                .is_some_and(|bid| price_tick < bid.saturating_sub(band)),
        }
    }

    /// The limit a market order on `side` gets: as far as the price band
    /// allows past the best opposing price, or the end of the ladder.
    fn market_limit(&self, side: Side) -> u32 {
        let top = self.tick_count() as u32 - 1;
        match (side, self.limits.price_band) {
            (Side::Buy, Some(band)) => self
                .best_ask()
                .map_or(top, |ask| ask.saturating_add(band).min(top)),
            (Side::Sell, Some(band)) => self.best_bid().map_or(0, |bid| bid.saturating_sub(band)),
            (Side::Buy, None) => top,
            (Side::Sell, None) => 0,
        }
    }

    /// Whether an order on `side` limited at `price_tick` would trade at
    /// once: its price reaches the best price on the other side.
    fn is_marketable(&self, side: Side, price_tick: u32) -> bool {
        match side {
            Side::Buy => self.best_ask().is_some_and(|ask| ask <= price_tick),
            Side::Sell => self.best_bid().is_some_and(|bid| bid >= price_tick),
        }
    }

    /// Submits a new limit order: the same as
    /// [`submit_order`](Self::submit_order) with [`OrderType::Limit`], which
    /// describes the checks, the matching and the events in full.
    #[inline]
    pub fn process_order<S: EventSink + ?Sized>(
        &mut self,
        order_id: u64,
        side: Side,
        price_tick: u32,
        qty: u64,
        sink: &mut S,
    ) {
        self.submit_order(NewOrder::limit(order_id, side, price_tick, qty), sink);
    }

    /// Submits a new order of any [`OrderType`], described by a [`NewOrder`].
    ///
    /// The order is first checked for a nonzero quantity, a non-reserved ID,
    /// a valid price tick and an unused ID, and a post-only order also for
    /// not trading at once; then it is accepted. A failed check
    /// emits only `OrderRejected`, with the [`RejectReason`]. It trades at resting
    /// maker prices, consuming the best eligible price first and preserving
    /// FIFO order among orders at that price. A buy may trade at asks no
    /// higher than `price_tick`; a sell may trade at bids no lower than it.
    /// The unfilled quantity of a [`Limit`](OrderType::Limit) or
    /// [`PostOnly`](OrderType::PostOnly) order is added
    /// to the book if an order slot is available and the price-level
    /// quantity can be represented; that of an
    /// [`ImmediateOrCancel`](OrderType::ImmediateOrCancel) or
    /// [`Market`](OrderType::Market) order expires.
    ///
    /// Every event is emitted to `sink` in order: acceptance, then each trade
    /// followed by the maker's fill update, then the taker's final fill or
    /// partial-fill state. A remainder that may not rest is then reported
    /// with `OrderExpired`.
    ///
    /// With [`SelfTradePrevention`] on, meeting a resting order of the same
    /// account emits `SelfTradePrevented` instead of a trade, then cancels
    /// the resting order (`OrderCancelled`), stops the incoming one (its
    /// remainder expires), or both, as the mode says. If a remainder cannot rest, an `OrderRejected`
    /// event (reason [`BookFull`](RejectReason::BookFull) or
    /// [`LevelQuantityOverflow`](RejectReason::LevelQuantityOverflow)) is
    /// also emitted; trades already completed by this call are not
    /// undone. The engine does not allocate, so the call is allocation-free
    /// whenever `sink` is.
    ///
    /// O(1) amortized to place; each matched maker node is processed in
    /// O(1), so total cost is O(1) + O(fills).
    pub fn submit_order<S: EventSink + ?Sized>(&mut self, order: NewOrder, sink: &mut S) {
        let NewOrder {
            order_id,
            account,
            side,
            order_type,
            price_tick,
            qty,
            self_trade_prevention: _,
        } = order;
        // A market order may trade at any price: give it the worst limit
        // its side allows, and every later step treats it as usual.
        let price_tick = match order_type {
            OrderType::Market => self.market_limit(side),
            _ => price_tick,
        };
        let order = NewOrder {
            price_tick,
            ..order
        };
        // Reject invalid input before emitting acceptance or changing book state.
        if let Err(reason) = self.validate(order_id, side, order_type, price_tick, qty) {
            sink.emit(Event::OrderRejected { order_id, reason });
            return;
        }
        sink.emit(Event::OrderAccepted {
            order_id,
            account,
            side,
            order_type,
            price_tick,
            qty,
        });

        // Matching spends down the incoming quantity; only the remainder can
        // become a new resting order.
        let mut remaining = qty;
        let stopped_by_self_trade = match side {
            // Incoming buy crosses resting asks priced at or below the limit.
            Side::Buy => Self::match_incoming(
                &mut self.pool,
                &mut self.ask_levels,
                &mut self.ask_bitset,
                &mut self.id_map,
                &mut self.trades,
                &order,
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
                &mut self.trades,
                &order,
                &mut remaining,
                sink,
                Crossing::TakerSell,
            ),
        };

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

        if stopped_by_self_trade || !order_type.rests() {
            // The remainder never enters the book, so it needs no slot.
            sink.emit(Event::OrderExpired {
                order_id,
                expired_qty: remaining,
            });
            self.sequence = self.sequence.wrapping_add(1);
            return;
        }

        // Rest the remainder in the book.
        let remainder = RestingOrder {
            order_id,
            account,
            side,
            price_tick,
            remaining_qty: remaining,
        };
        if let Err(reason) = self.rest(remainder) {
            sink.emit(Event::OrderRejected { order_id, reason });
        }
        self.sequence = self.sequence.wrapping_add(1);
    }

    /// Puts `order` at the back of its price level's queue, or says why it
    /// cannot rest: every slot is in use (`BookFull`), or the level's total
    /// quantity would overflow (`LevelQuantityOverflow`). Nothing changes
    /// when it fails.
    fn rest(&mut self, order: RestingOrder) -> Result<(), RejectReason> {
        let RestingOrder {
            order_id,
            account,
            side,
            price_tick,
            remaining_qty,
        } = order;
        let idx = self
            .pool
            .allocate(
                order_id,
                account,
                side,
                price_tick,
                remaining_qty,
                self.sequence,
            )
            .ok_or(RejectReason::BookFull)?;
        let (levels, bitset) = match side {
            Side::Buy => (&mut self.bid_levels, &mut self.bid_bitset),
            Side::Sell => (&mut self.ask_levels, &mut self.ask_bitset),
        };
        if !levels[price_tick as usize].push_back(&mut self.pool, idx) {
            // An adversarially large quantity would have overflowed this
            // level's `total_qty`; back out the allocation rather than rest
            // a corrupt level.
            self.pool.free(idx);
            return Err(RejectReason::LevelQuantityOverflow);
        }
        bitset.set(price_tick as usize);
        self.id_map.insert(order_id, idx);
        Ok(())
    }

    /// Captures the engine's complete state: its capacity, its counters and
    /// every resting order in priority order (bids best price first, then
    /// asks best price first, oldest first at each price).
    ///
    /// [`restore`](Self::restore) turns it back into an engine that behaves
    /// exactly like this one from now on. Each order's arrival stamp, audit
    /// metadata that nothing reads, is not kept. Walks the book like
    /// [`depth_snapshot`](Self::depth_snapshot) and allocates the order list,
    /// so it is not for the hot path.
    pub fn snapshot(&self) -> Snapshot {
        let mut orders = Vec::with_capacity(self.live_order_count());
        for side in [Side::Buy, Side::Sell] {
            let (levels, mut tick) = match side {
                Side::Buy => (&self.bid_levels, self.bid_bitset.highest()),
                Side::Sell => (&self.ask_levels, self.ask_bitset.lowest()),
            };
            while let Some(t) = tick {
                let mut idx = levels[t].head;
                while idx != NIL {
                    let order = self.pool.get(idx);
                    orders.push(RestingOrder {
                        order_id: order.order_id,
                        account: order.account,
                        side: order.side,
                        price_tick: order.price_tick,
                        remaining_qty: order.remaining_qty,
                    });
                    idx = order.next_idx;
                }
                tick = match side {
                    Side::Buy => self.next_lower_active(&self.bid_bitset, t),
                    Side::Sell => self.next_higher_active(&self.ask_bitset, t),
                };
            }
        }
        Snapshot {
            capacity: self.capacity(),
            tick_count: self.tick_count(),
            accepted_orders: self.sequence,
            trades: self.trades,
            risk_limits: self.limits,
            orders,
        }
    }

    /// Builds an engine from a [`Snapshot`]: with the same capacity and
    /// counters, and every order resting at its price in the order the
    /// snapshot lists it, so time priority is kept.
    ///
    /// The new engine emits exactly the events the original would have for
    /// any later request (its hash seed differs, which never matters).
    /// A snapshot that could not come from a live book is refused with a
    /// [`SnapshotError`] instead of becoming a corrupt one: a bad capacity
    /// or tick count, too many orders, an order no engine would rest, or a crossed book.
    pub fn restore(snapshot: &Snapshot) -> Result<Engine, SnapshotError> {
        let capacity = snapshot.capacity;
        if capacity == 0 || capacity >= u32::MAX as usize {
            return Err(SnapshotError::InvalidCapacity(capacity));
        }
        if !(1..=MAX_LADDER_TICKS).contains(&snapshot.tick_count) {
            return Err(SnapshotError::InvalidTickCount(snapshot.tick_count));
        }
        if snapshot.orders.len() > capacity {
            return Err(SnapshotError::TooManyOrders {
                orders: snapshot.orders.len(),
                capacity,
            });
        }
        let mut engine = Engine::with_ticks(capacity, snapshot.tick_count);
        engine.sequence = snapshot.accepted_orders;
        engine.trades = snapshot.trades;
        engine.limits = snapshot.risk_limits;
        for &order in &snapshot.orders {
            let check = if order.remaining_qty == 0 {
                Err(RejectReason::ZeroQuantity)
            } else if order.order_id == crate::hashmap::RESERVED_KEY {
                Err(RejectReason::ReservedOrderId)
            } else if order.price_tick as usize >= engine.tick_count() {
                Err(RejectReason::PriceOutOfRange)
            } else if engine.id_map.get(order.order_id).is_some() {
                Err(RejectReason::DuplicateOrderId)
            } else {
                engine.rest(order)
            };
            check.map_err(|reason| SnapshotError::InvalidOrder {
                order_id: order.order_id,
                reason,
            })?;
        }
        if let (Some(best_bid), Some(best_ask)) = (engine.best_bid(), engine.best_ask()) {
            if best_bid >= best_ask {
                return Err(SnapshotError::CrossedBook { best_bid, best_ask });
            }
        }
        Ok(engine)
    }

    /// Generic crossing loop shared by buy/sell taker orders, operating on
    /// the *opposing* side's arena/levels/bitset. `direction` decides
    /// whether we walk the opposing bitset from its lowest (taker is a
    /// buyer crossing resting asks) or highest (taker is a seller crossing
    /// resting bids) active tick, and whether a given tick is marketable
    /// against the taker's `price_tick`. Returns whether self-trade
    /// prevention stopped the taker.
    ///
    /// Takes disjoint field references directly (rather than going through
    /// `&mut Engine` accessor closures) so the borrow checker can see that
    /// `pool`, `levels`, and `bitset` never alias one another.
    #[allow(clippy::too_many_arguments)]
    fn match_incoming<S: EventSink + ?Sized>(
        pool: &mut OrderPool,
        levels: &mut [PriceLevel],
        bitset: &mut TickBitset,
        id_map: &mut FlatHashMap,
        trades: &mut u64,
        taker: &NewOrder,
        remaining: &mut u64,
        sink: &mut S,
        direction: Crossing,
    ) -> bool {
        let taker_order_id = taker.order_id;
        let price_tick = taker.price_tick;
        let taker_side = match direction {
            Crossing::TakerBuy => Side::Buy,
            Crossing::TakerSell => Side::Sell,
        };
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
                let maker = *pool.get(maker_idx);
                let maker_qty = maker.remaining_qty;
                let maker_order_id = maker.order_id;

                let mode = taker.self_trade_prevention;
                if mode != SelfTradePrevention::Allow && maker.account == taker.account {
                    // Report the prevented trade while the book is still
                    // untouched, then apply the mode.
                    sink.emit(Event::SelfTradePrevented {
                        taker_order_id,
                        maker_order_id,
                    });
                    if mode.cancels_resting() {
                        // The same removal as a full fill, then the same
                        // event as a cancel request.
                        level.remove(pool, maker_idx);
                        pool.free(maker_idx);
                        id_map.remove(maker_order_id);
                        if level.is_empty() {
                            bitset.clear(tick);
                        }
                        sink.emit(Event::OrderCancelled {
                            order_id: maker_order_id,
                            side: maker.side,
                            price_tick: maker.price_tick,
                            cancelled_qty: maker_qty,
                        });
                    }
                    if mode.stops_incoming() {
                        return true;
                    }
                    continue;
                }

                // A trade can consume the maker, the taker, or both exactly.
                let trade_qty = (*remaining).min(maker_qty);

                *trades += 1;
                sink.emit(Event::Trade {
                    trade_id: *trades,
                    maker_order_id,
                    taker_order_id,
                    taker_side,
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
        false
    }

    /// Cancels a resting order by ID.
    ///
    /// The ID map locates the arena slot, then the order is unlinked from its
    /// price-level FIFO queue. If it was the last order at that price, the
    /// price is removed from the active-level bitset. When an order was found
    /// and removed, `OrderCancelled` is emitted to `sink` and `true` is
    /// returned. Otherwise `CancelRejected` (reason
    /// [`NotResting`](CancelRejectReason::NotResting)) is emitted, the book
    /// is unchanged and the result is `false`, so the event stream records
    /// every cancel request either way.
    ///
    /// Lookup and unlink are constant-time in the usual case; hash-table
    /// probing can require additional work when resolving collisions.
    pub fn cancel_order<S: EventSink + ?Sized>(&mut self, order_id: u64, sink: &mut S) -> bool {
        let Some(idx) = self.id_map.remove(order_id) else {
            sink.emit(Event::CancelRejected {
                order_id,
                reason: CancelRejectReason::NotResting,
            });
            return false;
        };
        self.remove_cancelled(idx, sink);
        true
    }

    /// Unlinks the resting order in slot `idx` from its level, frees the
    /// slot and reports `OrderCancelled`. The caller has already removed
    /// the order's ID from the ID map.
    fn remove_cancelled<S: EventSink + ?Sized>(&mut self, idx: u32, sink: &mut S) {
        let order = *self.pool.get(idx);
        let (levels, bitset) = match order.side {
            Side::Buy => (&mut self.bid_levels, &mut self.bid_bitset),
            Side::Sell => (&mut self.ask_levels, &mut self.ask_bitset),
        };
        let level = &mut levels[order.price_tick as usize];
        level.remove(&mut self.pool, idx);
        if level.is_empty() {
            bitset.clear(order.price_tick as usize);
        }
        self.pool.free(idx);
        sink.emit(Event::OrderCancelled {
            order_id: order.order_id,
            side: order.side,
            price_tick: order.price_tick,
            cancelled_qty: order.remaining_qty,
        });
    }

    /// Cancels every resting order of `account`, for example when its
    /// trader disconnects ("cancel on disconnect").
    ///
    /// Emits one `OrderCancelled` per order, in a fixed order (bids best
    /// price first, then asks best price first, oldest first at each
    /// price), and then `MassCancelled` with the count, even when it is 0.
    /// Returns that count.
    ///
    /// The engine keeps no list of orders per account, so this walks the
    /// whole book: O(ticks between the best and worst price + resting
    /// orders), at most [`tick_count`](Self::tick_count) ticks. It does not
    /// allocate.
    pub fn cancel_account<S: EventSink + ?Sized>(&mut self, account: u32, sink: &mut S) -> u64 {
        self.mass_cancel(Some(account), sink)
    }

    /// Cancels every resting order, for example at the end of a trading
    /// session or when trading halts. Works exactly like
    /// [`cancel_account`](Self::cancel_account), for all accounts at once.
    pub fn cancel_all<S: EventSink + ?Sized>(&mut self, sink: &mut S) -> u64 {
        self.mass_cancel(None, sink)
    }

    fn mass_cancel<S: EventSink + ?Sized>(&mut self, account: Option<u32>, sink: &mut S) -> u64 {
        let mut cancelled = 0;
        for side in [Side::Buy, Side::Sell] {
            let mut tick = match side {
                Side::Buy => self.bid_bitset.highest(),
                Side::Sell => self.ask_bitset.lowest(),
            };
            while let Some(t) = tick {
                // Find the next level before cancelling empties this one.
                tick = match side {
                    Side::Buy => self.next_lower_active(&self.bid_bitset, t),
                    Side::Sell => self.next_higher_active(&self.ask_bitset, t),
                };
                let mut idx = match side {
                    Side::Buy => self.bid_levels[t].head,
                    Side::Sell => self.ask_levels[t].head,
                };
                while idx != NIL {
                    // Copy the order out: cancelling it clears its links.
                    let order = *self.pool.get(idx);
                    if account.is_none_or(|account| account == order.account) {
                        self.id_map.remove(order.order_id);
                        self.remove_cancelled(idx, sink);
                        cancelled += 1;
                    }
                    idx = order.next_idx;
                }
            }
        }
        sink.emit(Event::MassCancelled {
            account,
            cancelled_orders: cancelled,
        });
        cancelled
    }

    /// Lowers a resting order's quantity by `reduce_by`, keeping its place in
    /// the queue.
    ///
    /// A smaller order is never more attractive to trade with, so it keeps
    /// its time priority. (Raising the quantity would jump the queue: cancel
    /// and submit a new order for that.) The level's total drops with it.
    ///
    /// - If `reduce_by` is less than the remaining quantity, `OrderReduced`
    ///   is emitted.
    /// - If it is the remaining quantity or more, the order is cancelled
    ///   exactly as by [`cancel_order`](Self::cancel_order), and
    ///   `OrderCancelled` is emitted.
    /// - If `reduce_by` is zero, or no resting order has this ID,
    ///   `CancelRejected` says which, checked in that order, and the book is
    ///   unchanged.
    ///
    /// Returns whether the order was reduced or cancelled. Like a cancel,
    /// this is one hash lookup and O(1) work, and it does not allocate.
    pub fn reduce_order<S: EventSink + ?Sized>(
        &mut self,
        order_id: u64,
        reduce_by: u64,
        sink: &mut S,
    ) -> bool {
        let reject = |sink: &mut S, reason| {
            sink.emit(Event::CancelRejected { order_id, reason });
            false
        };
        if reduce_by == 0 {
            return reject(sink, CancelRejectReason::ZeroQuantity);
        }
        let Some(idx) = self.id_map.get(order_id) else {
            return reject(sink, CancelRejectReason::NotResting);
        };
        let order = *self.pool.get(idx);
        if reduce_by >= order.remaining_qty {
            return self.cancel_order(order_id, sink);
        }
        let levels = match order.side {
            Side::Buy => &mut self.bid_levels,
            Side::Sell => &mut self.ask_levels,
        };
        levels[order.price_tick as usize].reduce_qty(&mut self.pool, idx, reduce_by);
        sink.emit(Event::OrderReduced {
            order_id,
            side: order.side,
            price_tick: order.price_tick,
            reduced_qty: reduce_by,
            remaining_qty: order.remaining_qty - reduce_by,
        });
        true
    }

    /// Returns how many orders can rest at once: the `order_capacity` this
    /// engine was created with.
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.pool.capacity()
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

    /// Submits an order of any type and returns every event it produced.
    fn submit(
        engine: &mut Engine,
        order_id: u64,
        side: Side,
        order_type: OrderType,
        price_tick: u32,
        qty: u64,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        let order = NewOrder::limit(order_id, side, price_tick, qty).with_type(order_type);
        engine.submit_order(order, &mut events);
        events
    }

    /// The acceptance event for a limit order with these terms.
    fn accepted(order_id: u64, side: Side, price_tick: u32, qty: u64) -> Event {
        Event::OrderAccepted {
            order_id,
            account: 0,
            side,
            order_type: OrderType::Limit,
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
    fn immediate_or_cancel_trades_what_it_can_and_expires_the_rest() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 4);
        let events = submit(
            &mut engine,
            2,
            Side::Buy,
            OrderType::ImmediateOrCancel,
            101,
            10,
        );
        assert_eq!(
            events,
            vec![
                Event::OrderAccepted {
                    order_id: 2,
                    account: 0,
                    side: Side::Buy,
                    order_type: OrderType::ImmediateOrCancel,
                    price_tick: 101,
                    qty: 10
                },
                Event::Trade {
                    trade_id: 1,
                    maker_order_id: 1,
                    taker_order_id: 2,
                    taker_side: Side::Buy,
                    price_tick: 100,
                    qty: 4
                },
                Event::OrderFilled { order_id: 1 },
                Event::OrderPartiallyFilled {
                    order_id: 2,
                    remaining_qty: 6
                },
                Event::OrderExpired {
                    order_id: 2,
                    expired_qty: 6
                },
            ]
        );
        assert_eq!(engine.best_bid(), None);
        assert_eq!(engine.resting_order(2), None);
        assert_eq!(engine.live_order_count(), 0);
    }

    #[test]
    fn immediate_or_cancel_that_cannot_trade_expires_without_a_slot() {
        // The only slot is taken, which would reject a limit order's
        // remainder with BookFull. An IOC remainder never needs one.
        let mut engine = Engine::new(1);
        place(&mut engine, 1, Side::Sell, 100, 4);
        let events = submit(
            &mut engine,
            2,
            Side::Buy,
            OrderType::ImmediateOrCancel,
            99,
            5,
        );
        assert_eq!(events.len(), 2, "accepted and expired, no trade");
        assert_eq!(
            events.last(),
            Some(&Event::OrderExpired {
                order_id: 2,
                expired_qty: 5
            })
        );
        assert_eq!(engine.depth_at(Side::Sell, 100), 4);
        assert_eq!(engine.live_order_count(), 1);
    }

    #[test]
    fn market_buy_sweeps_asks_at_every_price_and_never_rests() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 0, 1);
        place(&mut engine, 2, Side::Sell, 2_000, 2);
        place(&mut engine, 3, Side::Sell, DEFAULT_TICKS as u32 - 1, 3);
        // The tick passed in is ignored, even an out-of-range one.
        let events = submit(&mut engine, 4, Side::Buy, OrderType::Market, u32::MAX, 10);
        assert_eq!(
            events.first(),
            Some(&Event::OrderAccepted {
                order_id: 4,
                account: 0,
                side: Side::Buy,
                order_type: OrderType::Market,
                price_tick: DEFAULT_TICKS as u32 - 1,
                qty: 10
            })
        );
        let traded: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Trade {
                    price_tick, qty, ..
                } => Some((*price_tick, *qty)),
                _ => None,
            })
            .collect();
        assert_eq!(traded, [(0, 1), (2_000, 2), (DEFAULT_TICKS as u32 - 1, 3)]);
        assert_eq!(
            events.last(),
            Some(&Event::OrderExpired {
                order_id: 4,
                expired_qty: 4
            })
        );
        assert_eq!(engine.best_ask(), None);
        assert_eq!(engine.best_bid(), None);
    }

    #[test]
    fn market_order_on_an_empty_side_only_expires() {
        let mut engine = Engine::new(16);
        let events = submit(&mut engine, 1, Side::Sell, OrderType::Market, 500, 7);
        assert_eq!(
            events,
            vec![
                Event::OrderAccepted {
                    order_id: 1,
                    account: 0,
                    side: Side::Sell,
                    order_type: OrderType::Market,
                    price_tick: 0,
                    qty: 7
                },
                Event::OrderExpired {
                    order_id: 1,
                    expired_qty: 7
                },
            ]
        );
        assert_eq!(engine.live_order_count(), 0);
    }

    #[test]
    fn post_only_that_would_trade_is_rejected_and_the_book_is_unchanged() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 5);
        place(&mut engine, 2, Side::Buy, 98, 5);
        for (order_id, side, tick) in [(3, Side::Buy, 100), (4, Side::Sell, 98)] {
            let events = submit(&mut engine, order_id, side, OrderType::PostOnly, tick, 1);
            assert_eq!(
                events,
                vec![Event::OrderRejected {
                    order_id,
                    reason: RejectReason::PostOnlyWouldTrade
                }]
            );
        }
        assert_eq!(engine.depth_at(Side::Sell, 100), 5);
        assert_eq!(engine.depth_at(Side::Buy, 98), 5);
        assert_eq!(engine.live_order_count(), 2);
    }

    #[test]
    fn post_only_that_does_not_cross_rests_like_a_limit_order() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 5);
        let events = submit(&mut engine, 2, Side::Buy, OrderType::PostOnly, 99, 3);
        assert_eq!(
            events,
            vec![Event::OrderAccepted {
                order_id: 2,
                account: 0,
                side: Side::Buy,
                order_type: OrderType::PostOnly,
                price_tick: 99,
                qty: 3
            }]
        );
        assert_eq!(engine.best_bid(), Some(99));
        assert_eq!(
            engine.resting_order(2).map(|order| order.remaining_qty),
            Some(3)
        );
    }

    #[test]
    fn the_account_is_reported_on_acceptance_and_kept_while_resting() {
        let mut engine = Engine::new(16);
        let mut events = Vec::new();
        engine.submit_order(
            NewOrder::limit(1, Side::Sell, 100, 5).with_account(7),
            &mut events,
        );
        assert!(matches!(
            events.as_slice(),
            [Event::OrderAccepted {
                order_id: 1,
                account: 7,
                ..
            }]
        ));
        assert_eq!(engine.resting_order(1).map(|order| order.account), Some(7));
    }

    /// A book with an ask of 5 for account 2, then one of 5 for account 1,
    /// both at tick 100, and the events of a buy of `qty` from account 1.
    fn self_trade(mode: SelfTradePrevention, qty: u64) -> (Engine, Vec<Event>) {
        let mut engine = Engine::new(16);
        let mut events = Vec::new();
        for (order_id, account) in [(1, 2), (2, 1)] {
            let ask = NewOrder::limit(order_id, Side::Sell, 100, 5).with_account(account);
            engine.submit_order(ask, &mut events);
        }
        events.clear();
        let buy = NewOrder::limit(3, Side::Buy, 100, qty)
            .with_account(1)
            .with_self_trade_prevention(mode);
        engine.submit_order(buy, &mut events);
        (engine, events)
    }

    /// The events after acceptance, as short labels.
    fn labels(events: &[Event]) -> Vec<String> {
        events[1..]
            .iter()
            .map(|event| match event {
                Event::Trade { maker_order_id, .. } => format!("trade {maker_order_id}"),
                Event::OrderFilled { order_id } => format!("filled {order_id}"),
                Event::OrderPartiallyFilled { order_id, .. } => format!("partial {order_id}"),
                Event::SelfTradePrevented { maker_order_id, .. } => {
                    format!("prevented {maker_order_id}")
                }
                Event::OrderCancelled { order_id, .. } => format!("cancelled {order_id}"),
                Event::OrderExpired {
                    order_id,
                    expired_qty,
                } => format!("expired {order_id}: {expired_qty}"),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn self_trades_happen_unless_prevention_is_asked_for() {
        let (engine, events) = self_trade(SelfTradePrevention::Allow, 10);
        assert_eq!(
            labels(&events),
            ["trade 1", "filled 1", "trade 2", "filled 2", "filled 3"]
        );
        assert_eq!(engine.best_ask(), None);
    }

    #[test]
    fn cancel_resting_removes_the_own_order_and_keeps_matching() {
        let (engine, events) = self_trade(SelfTradePrevention::CancelResting, 10);
        assert_eq!(
            labels(&events),
            [
                "trade 1",
                "filled 1",
                "prevented 2",
                "cancelled 2",
                "partial 3"
            ]
        );
        // The buy's remainder rests: only the own order was in its way.
        assert_eq!(engine.depth_at(Side::Buy, 100), 5);
        assert_eq!(engine.best_ask(), None);
    }

    #[test]
    fn cancel_incoming_expires_the_remainder_even_of_a_limit_order() {
        let (engine, events) = self_trade(SelfTradePrevention::CancelIncoming, 10);
        assert_eq!(
            labels(&events),
            [
                "trade 1",
                "filled 1",
                "prevented 2",
                "partial 3",
                "expired 3: 5"
            ]
        );
        assert_eq!(engine.depth_at(Side::Sell, 100), 5); // #2 untouched
        assert_eq!(engine.best_bid(), None);
    }

    #[test]
    fn cancel_both_removes_the_own_order_and_stops_the_incoming_one() {
        let (engine, events) = self_trade(SelfTradePrevention::CancelBoth, 10);
        assert_eq!(
            labels(&events),
            [
                "trade 1",
                "filled 1",
                "prevented 2",
                "cancelled 2",
                "partial 3",
                "expired 3: 5"
            ]
        );
        assert_eq!(engine.live_order_count(), 0);
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
                    trade_id: 1,
                    maker_order_id: 1,
                    taker_order_id: 2,
                    taker_side: Side::Buy,
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
                    trade_id: 1,
                    maker_order_id: 1,
                    taker_order_id: 2,
                    taker_side: Side::Buy,
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
                    trade_id: 1,
                    maker_order_id: 1,
                    taker_order_id: 2,
                    taker_side: Side::Buy,
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

        // Cancelling again fails cleanly, and says so in the event stream.
        let (ok, events) = cancel(&mut engine, 3);
        assert!(!ok);
        assert_eq!(
            events.as_slice(),
            &[Event::CancelRejected {
                order_id: 3,
                reason: CancelRejectReason::NotResting
            }]
        );
        assert_eq!(engine.live_order_count(), 0);
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
    fn resting_order_follows_an_order_through_its_life() {
        let mut engine = Engine::new(16);
        assert_eq!(engine.resting_order(1), None);
        place(&mut engine, 1, Side::Sell, 120, 10);
        let resting = |remaining_qty| RestingOrder {
            order_id: 1,
            account: 0,
            side: Side::Sell,
            price_tick: 120,
            remaining_qty,
        };
        assert_eq!(engine.resting_order(1), Some(resting(10)));
        place(&mut engine, 2, Side::Buy, 120, 4);
        assert_eq!(engine.resting_order(1), Some(resting(6)));
        // The taker filled completely, so it never rested.
        assert_eq!(engine.resting_order(2), None);
        cancel(&mut engine, 1);
        assert_eq!(engine.resting_order(1), None);
        // The reserved id is never stored, and looking it up is harmless.
        assert_eq!(engine.resting_order(u64::MAX), None);
    }

    #[test]
    fn cancelling_a_filled_order_is_rejected_without_touching_the_book() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 5);
        place(&mut engine, 2, Side::Sell, 100, 5);
        place(&mut engine, 3, Side::Buy, 100, 5); // fills #1
        let (ok, events) = cancel(&mut engine, 1);
        assert!(!ok);
        assert_eq!(
            events.as_slice(),
            &[Event::CancelRejected {
                order_id: 1,
                reason: CancelRejectReason::NotResting
            }]
        );
        assert_eq!(engine.depth_at(Side::Sell, 100), 5);
        assert_eq!(engine.live_order_count(), 1);
    }

    /// Reduces an order and returns whether it was found plus its events.
    fn reduce(engine: &mut Engine, order_id: u64, reduce_by: u64) -> (bool, Vec<Event>) {
        let mut events = Vec::new();
        let found = engine.reduce_order(order_id, reduce_by, &mut events);
        (found, events)
    }

    #[test]
    fn reducing_an_order_keeps_its_time_priority() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 5);
        place(&mut engine, 2, Side::Sell, 100, 5);
        let (ok, events) = reduce(&mut engine, 1, 3);
        assert!(ok);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderReduced {
                order_id: 1,
                side: Side::Sell,
                price_tick: 100,
                reduced_qty: 3,
                remaining_qty: 2
            }]
        );
        assert_eq!(engine.depth_at(Side::Sell, 100), 7);

        // #1 is still first in line, with its smaller quantity.
        let events = place(&mut engine, 3, Side::Buy, 100, 4);
        let fills: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Trade {
                    maker_order_id,
                    qty,
                    ..
                } => Some((*maker_order_id, *qty)),
                _ => None,
            })
            .collect();
        assert_eq!(fills, [(1, 2), (2, 2)]);
    }

    #[test]
    fn reducing_by_the_whole_remainder_cancels_the_order() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 90, 5);
        let (ok, events) = reduce(&mut engine, 1, 9);
        assert!(ok);
        assert_eq!(
            events.as_slice(),
            &[Event::OrderCancelled {
                order_id: 1,
                side: Side::Buy,
                price_tick: 90,
                cancelled_qty: 5
            }]
        );
        assert_eq!(engine.best_bid(), None);
        assert_eq!(engine.live_order_count(), 0);
    }

    #[test]
    fn reduce_rejects_zero_and_orders_that_are_not_resting() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 90, 5);
        let rejected = |order_id, reason| vec![Event::CancelRejected { order_id, reason }];
        assert_eq!(
            reduce(&mut engine, 1, 0),
            (false, rejected(1, CancelRejectReason::ZeroQuantity))
        );
        assert_eq!(
            reduce(&mut engine, 2, 1),
            (false, rejected(2, CancelRejectReason::NotResting))
        );
        // Zero is checked first, even for an unknown order.
        assert_eq!(
            reduce(&mut engine, 2, 0),
            (false, rejected(2, CancelRejectReason::ZeroQuantity))
        );
        assert_eq!(engine.depth_at(Side::Buy, 90), 5);
    }

    #[test]
    fn cancel_account_removes_only_that_accounts_orders_in_book_order() {
        let mut engine = Engine::new(16);
        let mut events = Vec::new();
        for (order_id, account, side, tick) in [
            (1, 1, Side::Buy, 100),
            (2, 2, Side::Buy, 100),
            (3, 1, Side::Buy, 101),
            (4, 1, Side::Sell, 105),
            (5, 2, Side::Sell, 106),
        ] {
            let order = NewOrder::limit(order_id, side, tick, 5).with_account(account);
            engine.submit_order(order, &mut events);
        }
        events.clear();
        assert_eq!(engine.cancel_account(1, &mut events), 3);
        let cancelled: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::OrderCancelled { order_id, .. } => Some(*order_id),
                _ => None,
            })
            .collect();
        // Best bid first, then the asks.
        assert_eq!(cancelled, [3, 1, 4]);
        assert_eq!(
            events.last(),
            Some(&Event::MassCancelled {
                account: Some(1),
                cancelled_orders: 3
            })
        );
        assert_eq!(engine.live_order_count(), 2);
        assert_eq!(engine.best_bid(), Some(100));
        assert_eq!(engine.depth_at(Side::Buy, 100), 5);
        assert_eq!(engine.resting_order(1), None);
    }

    #[test]
    fn cancel_all_empties_the_book_and_always_reports() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 100, 5);
        place(&mut engine, 2, Side::Sell, 105, 5);
        let mut events = Vec::new();
        assert_eq!(engine.cancel_all(&mut events), 2);
        assert_eq!(engine.live_order_count(), 0);
        assert_eq!((engine.best_bid(), engine.best_ask()), (None, None));

        // On an empty book the summary still arrives.
        events.clear();
        assert_eq!(engine.cancel_all(&mut events), 0);
        assert_eq!(
            events,
            vec![Event::MassCancelled {
                account: None,
                cancelled_orders: 0
            }]
        );
    }

    #[test]
    fn a_restored_engine_keeps_priority_and_counters() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 101, 3);
        place(&mut engine, 2, Side::Sell, 100, 4);
        place(&mut engine, 3, Side::Sell, 100, 5); // behind #2
        place(&mut engine, 4, Side::Buy, 98, 6);
        place(&mut engine, 5, Side::Buy, 99, 7);
        place(&mut engine, 6, Side::Buy, 100, 1); // trade 1, with #2

        let snapshot = engine.snapshot();
        let ids: Vec<_> = snapshot.orders.iter().map(|o| o.order_id).collect();
        assert_eq!(ids, [5, 4, 2, 3, 1], "bids best first, then asks, FIFO");
        assert_eq!((snapshot.capacity, snapshot.trades), (16, 1));

        let mut restored = Engine::restore(&snapshot).unwrap();
        assert_eq!(restored.snapshot(), snapshot);
        // The same sweep produces the same events: #2 still goes before #3,
        // and trade ids continue from 2.
        let original = place(&mut engine, 7, Side::Buy, 101, 12);
        assert_eq!(place(&mut restored, 7, Side::Buy, 101, 12), original);
        assert!(matches!(
            original[1],
            Event::Trade {
                trade_id: 2,
                maker_order_id: 2,
                ..
            }
        ));
    }

    #[test]
    fn restore_refuses_snapshots_no_live_book_could_have() {
        let order = |order_id, side, price_tick, remaining_qty| RestingOrder {
            order_id,
            account: 0,
            side,
            price_tick,
            remaining_qty,
        };
        let snapshot = |capacity, orders: Vec<RestingOrder>| Snapshot {
            capacity,
            tick_count: DEFAULT_TICKS,
            accepted_orders: 0,
            trades: 0,
            risk_limits: RiskLimits::NONE,
            orders,
        };
        let restore = |s: &Snapshot| Engine::restore(s).err();

        assert_eq!(
            restore(&snapshot(0, vec![])),
            Some(SnapshotError::InvalidCapacity(0))
        );
        assert_eq!(
            restore(&snapshot(
                1,
                vec![order(1, Side::Buy, 9, 1), order(2, Side::Buy, 9, 1)]
            )),
            Some(SnapshotError::TooManyOrders {
                orders: 2,
                capacity: 1
            })
        );
        assert_eq!(
            restore(&snapshot(
                4,
                vec![order(1, Side::Buy, 9, 1), order(1, Side::Sell, 20, 1)]
            )),
            Some(SnapshotError::InvalidOrder {
                order_id: 1,
                reason: RejectReason::DuplicateOrderId
            })
        );
        assert_eq!(
            restore(&snapshot(4, vec![order(1, Side::Buy, 9, 0)])),
            Some(SnapshotError::InvalidOrder {
                order_id: 1,
                reason: RejectReason::ZeroQuantity
            })
        );
        assert_eq!(
            restore(&snapshot(
                4,
                vec![order(1, Side::Buy, 9, u64::MAX), order(2, Side::Buy, 9, 1)]
            )),
            Some(SnapshotError::InvalidOrder {
                order_id: 2,
                reason: RejectReason::LevelQuantityOverflow
            })
        );
        assert_eq!(
            restore(&snapshot(
                4,
                vec![order(1, Side::Buy, 20, 1), order(2, Side::Sell, 20, 1)]
            )),
            Some(SnapshotError::CrossedBook {
                best_bid: 20,
                best_ask: 20
            })
        );
    }

    #[test]
    fn a_wide_ladder_trades_and_restores_far_beyond_4096_ticks() {
        let ticks = 300_000; // four bitset levels
        let mut engine = Engine::with_ticks(16, ticks);
        assert_eq!(engine.tick_count(), ticks);
        place(&mut engine, 1, Side::Sell, 250_000, 5);
        place(&mut engine, 2, Side::Sell, 4_096, 5);
        place(&mut engine, 3, Side::Buy, 100, 5);
        assert_eq!(
            (engine.best_bid(), engine.best_ask()),
            (Some(100), Some(4_096))
        );

        let rejected = place(&mut engine, 4, Side::Buy, ticks as u32, 1);
        assert!(matches!(
            rejected[..],
            [Event::OrderRejected {
                reason: RejectReason::PriceOutOfRange,
                ..
            }]
        ));

        let mut restored = Engine::restore(&engine.snapshot()).unwrap();
        assert_eq!(restored.tick_count(), ticks);
        // A market buy sweeps both asks, up to the top of this ladder.
        let sweep = |engine: &mut Engine| {
            let mut events = Vec::new();
            engine.submit_order(NewOrder::market(5, Side::Buy, 10), &mut events);
            events
        };
        let events = sweep(&mut restored);
        assert_eq!(events, sweep(&mut engine));
        assert!(matches!(
            events[0],
            Event::OrderAccepted {
                price_tick: 299_999,
                ..
            }
        ));
        assert_eq!(restored.best_ask(), None);
    }

    #[test]
    fn risk_limits_reject_oversized_orders_and_prices_far_through_the_market() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 50);
        place(&mut engine, 2, Side::Buy, 90, 50);
        engine.set_risk_limits(RiskLimits {
            max_order_qty: 10,
            price_band: Some(2),
        });
        let reason = |events: Vec<Event>| match events[..] {
            [Event::OrderRejected { reason, .. }] => Some(reason),
            _ => None,
        };
        let too_big = reason(place(&mut engine, 3, Side::Buy, 100, 11));
        assert_eq!(too_big, Some(RejectReason::QuantityAboveLimit));
        let buy_too_high = reason(place(&mut engine, 4, Side::Buy, 103, 1));
        assert_eq!(buy_too_high, Some(RejectReason::PriceOutsideBand));
        let sell_too_low = reason(place(&mut engine, 5, Side::Sell, 87, 1));
        assert_eq!(sell_too_low, Some(RejectReason::PriceOutsideBand));
        // Exactly at the edge of the band is allowed, and trades as usual.
        assert_eq!(reason(place(&mut engine, 6, Side::Buy, 102, 10)), None);
        assert_eq!(reason(place(&mut engine, 7, Side::Sell, 88, 10)), None);
        // Orders resting before the limits came are untouched.
        assert_eq!(engine.depth_at(Side::Sell, 100), 40);
        assert_eq!(engine.depth_at(Side::Buy, 90), 40);
    }

    #[test]
    fn the_price_band_stops_a_market_order_sweep() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Sell, 100, 1);
        place(&mut engine, 2, Side::Sell, 101, 1);
        place(&mut engine, 3, Side::Sell, 150, 1);
        engine.set_risk_limits(RiskLimits {
            price_band: Some(5),
            ..RiskLimits::NONE
        });
        let mut events = Vec::new();
        engine.submit_order(NewOrder::market(4, Side::Buy, 5), &mut events);
        assert!(matches!(
            events[0],
            Event::OrderAccepted {
                price_tick: 105,
                ..
            }
        ));
        let traded: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Trade { price_tick, .. } => Some(*price_tick),
                _ => None,
            })
            .collect();
        assert_eq!(traded, [100, 101]);
        assert_eq!(
            events.last(),
            Some(&Event::OrderExpired {
                order_id: 4,
                expired_qty: 3
            })
        );
        assert_eq!(engine.best_ask(), Some(150));

        // The limits are part of the state a snapshot keeps.
        let restored = Engine::restore(&engine.snapshot()).unwrap();
        assert_eq!(restored.risk_limits(), engine.risk_limits());
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
    fn trade_names_the_aggressor_side() {
        let mut engine = Engine::new(16);
        place(&mut engine, 1, Side::Buy, 100, 5);
        let events = place(&mut engine, 2, Side::Sell, 100, 5);
        assert!(events.contains(&Event::Trade {
            trade_id: 1,
            maker_order_id: 1,
            taker_order_id: 2,
            taker_side: Side::Sell,
            price_tick: 100,
            qty: 5
        }));
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
    fn trade_ids_count_up_across_calls() {
        let mut engine = Engine::new(16);
        let trade_ids = |events: &[Event]| -> Vec<u64> {
            events
                .iter()
                .filter_map(|e| match e {
                    Event::Trade { trade_id, .. } => Some(*trade_id),
                    _ => None,
                })
                .collect()
        };
        place(&mut engine, 1, Side::Sell, 100, 1);
        place(&mut engine, 2, Side::Sell, 101, 1);
        place(&mut engine, 3, Side::Sell, 102, 1);
        let sweep = place(&mut engine, 4, Side::Buy, 101, 5);
        assert_eq!(trade_ids(&sweep), [1, 2]);
        // Rejections, resting orders and cancels do not use up an id.
        place(&mut engine, 5, Side::Buy, 100, 0);
        place(&mut engine, 6, Side::Buy, 90, 1);
        cancel(&mut engine, 6);
        let next = place(&mut engine, 7, Side::Sell, 101, 1);
        assert_eq!(trade_ids(&next), [3]);
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
        let events = place(&mut engine, 1, Side::Sell, DEFAULT_TICKS as u32, 5);
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
        assert_eq!(
            events.as_slice(),
            &[Event::CancelRejected {
                order_id: u64::MAX,
                reason: CancelRejectReason::NotResting
            }]
        );
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

        assert_eq!(engine.depth_at(Side::Buy, DEFAULT_TICKS as u32), 0);
        assert_eq!(engine.depth_at(Side::Sell, u32::MAX), 0);
        assert_eq!(engine.depth_snapshot(Side::Buy, usize::MAX), vec![(100, 5)]);
        assert_eq!(
            engine.depth_snapshot(Side::Sell, usize::MAX),
            vec![(200, 7)]
        );
    }
}
