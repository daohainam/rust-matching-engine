# 7. Events: How the Engine Reports What Happened

> **New word?** Every technical term in these chapters is explained in the
> [glossary](glossary.md).

Source: [`src/events.rs`](../src/events.rs)

The engine does not print anything, does not write to a socket, and does not
decide where its results go. Every call to `process_order` or `cancel_order`
takes one extra argument, an **event sink**, and the engine hands each event to
that sink the moment it happens. The caller decides what to do with them
(publish market data, update accounts, write logs...).

This keeps the engine a pure, deterministic state machine:

```text
   (book state, request)  --->  Engine  --->  (new book state, events into the sink)
```

## 7.1 The `Event` enum

```rust
pub enum Event {
    OrderAccepted        { order_id: u64, account: u32, side: Side,
                           order_type: OrderType, price_tick: u32, qty: u64 },
    Trade                { trade_id: u64, maker_order_id: u64, taker_order_id: u64,
                           taker_side: Side, price_tick: u32, qty: u64 },
    OrderPartiallyFilled { order_id: u64, remaining_qty: u64 },
    OrderFilled          { order_id: u64 },
    OrderExpired         { order_id: u64, expired_qty: u64 },
    OrderCancelled       { order_id: u64, side: Side, price_tick: u32, cancelled_qty: u64 },
    OrderReduced         { order_id: u64, side: Side, price_tick: u32,
                           reduced_qty: u64, remaining_qty: u64 },
    OrderRejected        { order_id: u64, reason: RejectReason },
    SelfTradePrevented   { taker_order_id: u64, maker_order_id: u64 },
    MassCancelled        { account: Option<u32>, cancelled_orders: u64 },
    CancelRejected       { order_id: u64, reason: CancelRejectReason },
}

pub enum RejectReason {
    // Found before acceptance: the order is rejected alone, the book unchanged.
    ZeroQuantity,          // qty was 0
    ReservedOrderId,       // order_id was u64::MAX
    PriceOutOfRange,       // price_tick was 4096 or more
    DuplicateOrderId,      // a resting order already has this id
    PostOnlyWouldTrade,    // a post-only order's price reaches the other side
    // The remainder could not rest: always after OrderAccepted.
    BookFull,              // every arena slot is in use
    LevelQuantityOverflow, // the level's total would not fit in a u64
}

pub enum CancelRejectReason {
    NotResting,            // no resting order has this id
    ZeroQuantity,          // a reduce of 0
}
```

| Event | Emitted when | Who it is about |
|---|---|---|
| `OrderAccepted` | A new order passed validation. It repeats the order's account, side, type, limit price and quantity (for a market order, the worst limit its side allows). | the incoming order |
| `Trade` | Two orders executed against each other. `taker_side` is the aggressor's side; `trade_id` numbers trades 1, 2, 3, ... | maker **and** taker, at the maker's price |
| `OrderPartiallyFilled` | An order traded but still has quantity left. | a maker (after each partial fill), or the taker (once, at the end) |
| `OrderFilled` | An order has nothing left. | a maker (when it is consumed), or the taker |
| `OrderExpired` | The remainder of an immediate-or-cancel or market order was dropped; it never entered the book. | the taker (once, last) |
| `OrderCancelled` | A resting order was cancelled on request (or reduced by all it had). It says where the order rested and how much quantity left the book. | the cancelled order |
| `OrderReduced` | A resting order's quantity was lowered on request; it keeps its place in the queue. | the reduced order |
| `OrderRejected` | An order was refused: bad input, a post-only order that would trade, or no room for it to rest. `reason` says which. | the incoming order |
| `SelfTradePrevented` | An incoming order met a resting order of its own account, and its self-trade prevention mode blocked the trade. An `OrderCancelled` for the maker, the taker's `OrderExpired`, or both follow, as the mode says. | taker **and** maker |
| `MassCancelled` | A `cancel_account` (`account: Some(..)`) or `cancel_all` (`None`) finished; `cancelled_orders` counts the `OrderCancelled` events before it. | the request |
| `CancelRejected` | A cancel or reduce was refused: no resting order has the id, or the reduction was zero. `reason` says which. | the id in the request |

`Event` derives `Debug, Clone, Copy, PartialEq, Eq`, so events are cheap to copy
and trivial to compare in tests. One `Event` is 40 bytes (the biggest variant,
`Trade`, holds `8 + 8 + 8 + 8 + 4 + 1 = 37` bytes, plus the tag, rounded up to a
multiple of 8). The other variants fit in the space `Trade` needs anyway.

## 7.2 Ordering rules

For one `submit_order` (or `process_order`) call the events always follow this
shape:

```text
   [OrderAccepted]                                   <- always first (unless rejected up front)
   ( Trade, <maker Filled | maker PartiallyFilled> ) <- repeated once per execution, in execution order
   [ taker OrderFilled | taker OrderPartiallyFilled ]<- at most one, describing the taker's final state
   [ OrderExpired ]                                  <- IOC and market orders with quantity left, or a stopped self-trade
   [ OrderRejected ]                                 <- only if the order could not rest (it never traded)
```

Key points:

- A `Trade` is **always immediately followed** by the event describing the
  *maker's* new state.
- With self-trade prevention, a `SelfTradePrevented` takes the place of a
  `Trade` in that sequence. It is followed by the own maker's `OrderCancelled`
  (modes `CancelResting` and `CancelBoth`), and for `CancelIncoming` and
  `CancelBoth` matching then stops: the taker's final event and `OrderExpired`
  come next, even for a limit order.
- The **taker's** `OrderPartiallyFilled` or `OrderFilled` appears **once**, at the
  end, not after every trade. To follow a taker's progress, add up the `qty` of
  its `Trade` events.
- A taker that does not trade at all and simply rests produces **only**
  `OrderAccepted`. There is no separate "order is now resting" event.
- Rejections found before acceptance (bad input, or a post-only order that
  would trade) come **alone**; rejections caused by capacity
  come **after** `OrderAccepted`, and only for an order that did not trade
  (see [6.4](06-matching-algorithm.md#64-phase-4-resting-the-remainder)).
- The engine itself **never drops an event**. Only a bounded sink, such as
  `EventBatch`, can run out of room (7.4).
- A `cancel_order` or `reduce_order` call emits **exactly one** event:
  `OrderCancelled` or `OrderReduced` if it changed the book, `CancelRejected` if
  it did not. Its `bool` result says the same thing, for callers that only want
  to branch on it.
- A `cancel_account` or `cancel_all` call emits one `OrderCancelled` per order,
  bids best first, then asks best first, oldest first at each price, and always
  ends with `MassCancelled`.

### Following the book from events alone

The events carry enough to keep a copy of the book's depth without ever seeing
the requests. That is what a market-data publisher or a risk system does:

| Event | Change to the copy |
|---|---|
| `OrderAccepted { side, price_tick, qty, .. }` | remember these terms until the call ends |
| `Trade { taker_side, price_tick, qty, .. }` | the maker's level loses `qty`; it is on the side **opposite** `taker_side`, at `price_tick` |
| `OrderCancelled { side, price_tick, cancelled_qty, .. }` | that level loses `cancelled_qty` |
| `OrderReduced { side, price_tick, reduced_qty, .. }` | that level loses `reduced_qty` |
| `SelfTradePrevented`, `MassCancelled` | nothing: the `OrderCancelled` events around them carry the changes |
| end of a `submit_order` call | unless the taker's last event is `OrderFilled`, `OrderExpired` or `OrderRejected`, its remainder (`qty` minus its trades) now rests at its own `price_tick` |

`trade_id` helps with the gaps and duplicates of [9.5](09-write-ahead-log.md#95-reading-and-replaying):
the engine numbers trades in the order they happen, and the engine is
deterministic, so a replay of the same requests assigns exactly the same ids. A
consumer that has seen trade 41 can ignore any trade with a lower id, and a jump
from 41 to 43 means it missed one.

### Cookbook of event sequences

These are taken straight from the unit tests in `engine.rs`.

| Scenario | Events |
|---|---|
| Order rests, nothing crosses | `Accepted` |
| Exact full match, taker = maker | `Accepted`, `Trade`, `Filled(maker)`, `Filled(taker)` |
| Taker smaller than maker | `Accepted`, `Trade`, `PartiallyFilled(maker, left)`, `Filled(taker)` |
| Taker bigger than maker; remainder rests | `Accepted`, `Trade`, `Filled(maker)`, `PartiallyFilled(taker, left)` |
| Taker sweeps several levels | `Accepted`, then `Trade`+maker-event per fill, then the taker's final event |
| Invalid input / duplicate id | `Rejected(ZeroQuantity / ReservedOrderId / PriceOutOfRange / DuplicateOrderId)` |
| Post-only order whose price reaches the other side | `Rejected(PostOnlyWouldTrade)` |
| Order can't rest (arena full / overflow) | `Accepted`, `Rejected(BookFull / LevelQuantityOverflow)` (no trades happened) |
| IOC or market order with quantity left | `Accepted`, then `Trade`+maker-event per fill, then `PartiallyFilled(taker, left)` if it traded, then `Expired(left)` |
| Cancel succeeds | `Cancelled` |
| Cancel of an order that is not resting | `CancelRejected(NotResting)` (and the returned `bool` is `false`) |
| Reduce by less than what is left | `Reduced` |
| Reduce by all that is left, or more | `Cancelled` |
| Reduce by 0 | `CancelRejected(ZeroQuantity)` |
| Taker meets its own account's order, mode `CancelResting` | ..., `SelfTradePrevented`, `Cancelled(maker)`, then matching goes on |
| Taker meets its own account's order, mode `CancelIncoming` | ..., `SelfTradePrevented`, `PartiallyFilled(taker, left)` if it traded, `Expired(left)` |
| Mass cancel | `Cancelled` per order, in book order, then `MassCancelled(count)` |

## 7.3 The `EventSink` trait

```rust
pub trait EventSink {
    /// Accepts the next event.
    fn emit(&mut self, event: Event);
}
```

One method: "here is the next event". The crate implements it for three kinds of
sink, and you can implement it for your own types too.

| Sink | Allocates? | Keeps | Use it when |
|---|---|---|---|
| `Vec<Event>` | only while it grows; reuse it (with `clear`) and it stops after warm-up | every event | tests, simple callers, anything that wants the full list |
| a closure `FnMut(Event)` | only if the closure does | nothing by itself | forwarding each event straight on: into a queue, a counter, a log |
| `EventBatch` | **never** | the first 64 events of a call, counting the rest | a strictly allocation-free caller that checks for truncation |

Here are all three in action. The same example is in the rustdoc of
[`src/events.rs`](../src/events.rs), where `cargo test` compiles and runs it:

```rust
use matching_engine::{Engine, Event, EventBatch, OrderType, Side};

let mut engine = Engine::new(16);

// 1. A `Vec<Event>` keeps everything. Reuse it (with `clear`) and it stops
//    allocating once it has grown to fit the largest call.
let mut events = Vec::new();
engine.process_order(1, Side::Sell, 100, 5, &mut events);
assert_eq!(
    events,
    [Event::OrderAccepted {
        order_id: 1,
        account: 0,
        side: Side::Sell,
        order_type: OrderType::Limit,
        price_tick: 100,
        qty: 5,
    }]
);

// 2. A closure sees each event as it happens; this one counts trades.
let mut trades = 0;
engine.process_order(2, Side::Buy, 100, 2, &mut |event: Event| {
    if matches!(event, Event::Trade { .. }) {
        trades += 1;
    }
});
assert_eq!(trades, 1);

// 3. An `EventBatch` never allocates. It keeps the first 64 events of a
//    call and only counts the rest, so check `is_truncated` afterwards.
let mut batch = EventBatch::new();
engine.process_order(3, Side::Buy, 100, 3, &mut batch);
assert!(!batch.is_truncated());
assert_eq!(batch.as_slice().last(), Some(&Event::OrderFilled { order_id: 3 }));
```

### Why a sink instead of a return value?

An earlier version of the engine returned a fixed 64-slot `EventBatch` from every
call. That design had two costs, and both are instructive:

1. **Silent loss.** One taker that sweeps 32 or more makers produces more than
   64 events. The book was updated correctly, but the reports for the extra
   trades simply vanished, so anyone reconciling positions from events would have
   missed fills.
2. **Hidden work.** "No `malloc`" is not "no cost": every call initialised and
   copied a 2 KiB array, even for an order that produced a single event.

With a sink, the engine never needs to know how many events a call will
produce. And because `process_order` is generic over the sink type
(`S: EventSink + ?Sized`), the compiler builds a specialised copy for each sink
and inlines `emit`: a closure sink costs no more than code written by hand.

## 7.4 What happens when there are too many events?

The engine never drops events, but a **bounded** sink can run out of room. That
is what `EventBatch` is for: a fixed array that never allocates and is honest
about overflow.

```rust
pub const MAX_EVENTS_PER_CALL: usize = 64;

pub struct EventBatch {
    events: [Event; MAX_EVENTS_PER_CALL],   // fixed array, no heap
    len: usize,                             // how many are stored
    dropped: usize,                         // how many did not fit since the last clear()
}
```

```text
   events:  [ e0 | e1 | e2 | e3 | ... unused slots (stale or default values) ... ]     len = 4
              ^^^^^^^^^^^^^^^^^^^
              as_slice() returns only these
```

- `push` (called through `emit`) writes at `events[len]` while there is room;
  after that it only increments `dropped`. It never panics and never allocates.
- `dropped()` says how many events did not fit, and `is_truncated()` is
  `dropped() > 0`. **Check it after every call** when you use a batch.
- `clear()` resets `len` and `dropped` to 0 without wiping the array, so a batch
  is cheap to reuse: create one up front and `clear()` it before each call.
- `as_slice()`, `len()`, `is_empty()` and `IntoIterator for &EventBatch` give
  convenient read access.

**Size** (measured): `64 × 40 + 8 + 8 = 2,576` bytes.

How many makers fit? Each maker touched costs two events (`Trade` plus the
maker's `OrderFilled` or `OrderPartiallyFilled`), plus one `OrderAccepted` at the
start and one final taker event:

```text
   events = 1 + 2 × (makers touched) + 1      <=  64
                                               =>  at most 31 makers fully reported
```

| Makers swept by one taker | Events produced | Kept by an `EventBatch` | `dropped()` |
|---|---|---|---|
| 30 | 62 | 62 | 0 |
| 31 | 64 | 64 | 0 |
| 32 | 66 | 64 | 2 (the last maker's `OrderFilled` and the taker's `OrderFilled`) |
| 40 | 82 | 64 | 18 |

The 40-maker row is the unit test `bounded_batch_sink_reports_truncation`. Its
neighbour `deep_sweep_reports_every_execution` runs the same sweep into a
`Vec<Event>` and checks that all 40 trades and all 40 maker fills arrive, with
the taker's `OrderFilled` last.

When a batch reports truncation, the book is still exactly right; only the report
is incomplete. Your options:

1. Use an unbounded sink (`Vec`, or a closure that forwards each event onwards).
2. Treat `is_truncated()` as an error in your own code, for example by rebuilding
   your view of the book from `depth_snapshot`.
3. Limit how much one order can sweep, before it reaches the engine.

## 7.5 When a sink panics

The sink is the one place where the engine runs code it does not control. So the
engine follows a simple rule: **it only calls `emit` between complete state
changes.** That is why, in the matching loop, a drained price is cleared from the
bitset *before* the last maker's fill is reported ([6.3](06-matching-algorithm.md)).
Every structure agrees whenever your sink runs.

A sink should still not panic. If one does:

- If the program was built with `panic = "abort"`, the process stops. You
  recover by replaying the write-ahead log, as after any crash (chapter 9). This
  crate's release profile sets it, but that only applies to binaries built from
  this package (its tests, examples and benches). Cargo ignores a dependency's
  profile, so if you use the engine as a library, *your* `Cargo.toml` decides;
  see [10.5](10-testing-and-performance.md#105-build-profiles).
- If the panic is caught (with `catch_unwind`), the book is consistent, but the
  interrupted request was only **partly applied**: some trades happened, and the
  taker's remainder never rested. The engine now differs from what a replay of
  the logged requests would produce, so **drop it and rebuild it from the log**.

Rust warns you about this exact situation: `&mut Engine` is not `UnwindSafe`, so
`catch_unwind` refuses the closure until you wrap it in `AssertUnwindSafe`, which
is your signature on "I know the state may be half-updated". The unit test
`panicking_sink_leaves_the_book_consistent` does exactly that.

## 7.6 Consuming events

A plain function works as a sink too, because it implements `FnMut(Event)`:

```rust
use matching_engine::{Engine, Event, NewOrder, OrderType, Side};

fn log_event(event: Event) {
    match event {
        Event::OrderAccepted { order_id, account, side, order_type, price_tick, qty } => {
            println!("accepted  #{order_id} for {account}: {order_type:?} {side:?} {qty} @ {price_tick}")
        }
        Event::Trade { trade_id, maker_order_id, taker_order_id, taker_side, price_tick, qty } => {
            println!(
                "trade {trade_id}   {qty} @ {price_tick}, {taker_side:?} aggressor \
                 (maker #{maker_order_id}, taker #{taker_order_id})"
            )
        }
        Event::OrderPartiallyFilled { order_id, remaining_qty } => {
            println!("partial   #{order_id}, {remaining_qty} left")
        }
        Event::OrderFilled { order_id } => println!("filled    #{order_id}"),
        Event::OrderExpired { order_id, expired_qty } => {
            println!("expired   #{order_id}, {expired_qty} never rested")
        }
        Event::OrderCancelled { order_id, cancelled_qty, .. } => {
            println!("cancelled #{order_id}, {cancelled_qty} removed")
        }
        Event::OrderReduced { order_id, reduced_qty, remaining_qty, .. } => {
            println!("reduced   #{order_id} by {reduced_qty}, {remaining_qty} left")
        }
        Event::OrderRejected { order_id, reason } => println!("rejected  #{order_id}: {reason:?}"),
        Event::SelfTradePrevented { taker_order_id, maker_order_id } => {
            println!("prevented #{taker_order_id} trading with its own #{maker_order_id}")
        }
        Event::MassCancelled { account, cancelled_orders } => {
            println!("mass cancel of {account:?}: {cancelled_orders} orders")
        }
        Event::CancelRejected { order_id, reason } => {
            println!("cancel or reduce of #{order_id} rejected: {reason:?}")
        }
    }
}

let mut engine = Engine::new(16);
engine.process_order(1, Side::Sell, 100, 10, &mut log_event);
let ioc = NewOrder::limit(2, Side::Buy, 100, 14).with_type(OrderType::ImmediateOrCancel);
engine.submit_order(ioc, &mut log_event);
```

Testing with events is just as direct; this is how the engine's own tests are
written (they wrap the two lines in a small `place` helper):

```rust
let mut engine = Engine::new(16);
let mut events = Vec::new();
engine.process_order(1, Side::Sell, 100, 10, &mut events);
events.clear();
engine.process_order(2, Side::Buy, 100, 4, &mut events);

assert_eq!(
    events,
    [
        Event::OrderAccepted {
            order_id: 2,
            account: 0,
            side: Side::Buy,
            order_type: OrderType::Limit,
            price_tick: 100,
            qty: 4,
        },
        Event::Trade {
            trade_id: 1,
            maker_order_id: 1,
            taker_order_id: 2,
            taker_side: Side::Buy,
            price_tick: 100,
            qty: 4,
        },
        Event::OrderPartiallyFilled { order_id: 1, remaining_qty: 6 },
        Event::OrderFilled { order_id: 2 },
    ]
);
```

## 7.7 Try it yourself

Run the event tests and the two engine tests that compare sinks:

```sh
cargo test events::
cargo test deep_sweep_reports_every_execution
cargo test bounded_batch_sink_reports_truncation
```

Then try a closure sink. In [`examples/walkthrough.rs`](../examples/walkthrough.rs),
replace the `Vec<Event>` in `place` with a closure that prints only `Trade`
events as they happen, and check that the output still matches 1.10.

**Next:** [8. The ring buffer](08-ring-buffer.md)
