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
    OrderAccepted        { order_id: u64 },
    Trade                { maker_order_id: u64, taker_order_id: u64, price_tick: u32, qty: u64 },
    OrderPartiallyFilled { order_id: u64, remaining_qty: u64 },
    OrderFilled          { order_id: u64 },
    OrderCancelled       { order_id: u64 },
    OrderRejected        { order_id: u64, reason: RejectReason },
}

pub enum RejectReason {
    // Input errors: the order is rejected alone, before OrderAccepted.
    ZeroQuantity,          // qty was 0
    ReservedOrderId,       // order_id was u64::MAX
    PriceOutOfRange,       // price_tick was 4096 or more
    DuplicateOrderId,      // a resting order already has this id
    // The remainder could not rest: always after OrderAccepted.
    BookFull,              // every arena slot is in use
    LevelQuantityOverflow, // the level's total would not fit in a u64
}
```

| Event | Emitted when | Who it is about |
|---|---|---|
| `OrderAccepted` | A new order passed validation. | the incoming order |
| `Trade` | Two orders executed against each other. | maker **and** taker, at the maker's price |
| `OrderPartiallyFilled` | An order traded but still has quantity left. | a maker (after each partial fill), or the taker (once, at the end) |
| `OrderFilled` | An order has nothing left. | a maker (when it is consumed), or the taker |
| `OrderCancelled` | A resting order was cancelled on request. | the cancelled order |
| `OrderRejected` | An order was refused: bad input, or no room for it to rest. `reason` says which. | the incoming order |

`Event` derives `Debug, Clone, Copy, PartialEq, Eq`, so events are cheap to copy
and trivial to compare in tests. One `Event` is 32 bytes (the biggest variant,
`Trade`, holds `8 + 8 + 4 + 8 = 28` bytes plus the tag and padding). Adding a
`RejectReason` to `OrderRejected` costs nothing: `8 + 1` bytes still fits in the
space `Trade` needs anyway.

## 7.2 Ordering rules

For one `process_order` call the events always follow this shape:

```text
   [OrderAccepted]                                   <- always first (unless rejected up front)
   ( Trade, <maker Filled | maker PartiallyFilled> ) <- repeated once per execution, in execution order
   [ taker OrderFilled | taker OrderPartiallyFilled ]<- at most one, describing the taker's final state
   [ OrderRejected ]                                 <- only if the order could not rest (it never traded)
```

Key points:

- A `Trade` is **always immediately followed** by the event describing the
  *maker's* new state.
- The **taker's** `OrderPartiallyFilled` or `OrderFilled` appears **once**, at the
  end, not after every trade. To follow a taker's progress, add up the `qty` of
  its `Trade` events.
- A taker that does not trade at all and simply rests produces **only**
  `OrderAccepted`. There is no separate "order is now resting" event.
- Rejections caused by bad input come **alone**; rejections caused by capacity
  come **after** `OrderAccepted`, and only for an order that did not trade
  (see [6.4](06-matching-algorithm.md#64-phase-4-resting-the-remainder)).
- The engine itself **never drops an event**. Only a bounded sink, such as
  `EventBatch`, can run out of room (7.4).

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
| Order can't rest (arena full / overflow) | `Accepted`, `Rejected(BookFull / LevelQuantityOverflow)` (no trades happened) |
| Cancel succeeds | `Cancelled` |
| Cancel of unknown id | *(no events; the returned `bool` is `false`)* |

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
use matching_engine::{Engine, Event, EventBatch, Side};

let mut engine = Engine::new(16);

// 1. A `Vec<Event>` keeps everything. Reuse it (with `clear`) and it stops
//    allocating once it has grown to fit the largest call.
let mut events = Vec::new();
engine.process_order(1, Side::Sell, 100, 5, &mut events);
assert_eq!(events, [Event::OrderAccepted { order_id: 1 }]);

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

**Size** (measured): `64 × 32 + 8 + 8 = 2,064` bytes.

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
  recover by replaying the write-ahead log, as after any crash (chapter 8). This
  crate's release profile sets it, but that only applies to binaries built from
  this package (its tests, examples and benches). Cargo ignores a dependency's
  profile, so if you use the engine as a library, *your* `Cargo.toml` decides;
  see [9.5](09-testing-and-performance.md#95-build-profiles).
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
use matching_engine::{Engine, Event, Side};

fn log_event(event: Event) {
    match event {
        Event::OrderAccepted { order_id } => println!("accepted  #{order_id}"),
        Event::Trade { maker_order_id, taker_order_id, price_tick, qty } => println!(
            "trade     {qty} @ {price_tick} (maker #{maker_order_id}, taker #{taker_order_id})"
        ),
        Event::OrderPartiallyFilled { order_id, remaining_qty } => {
            println!("partial   #{order_id}, {remaining_qty} left")
        }
        Event::OrderFilled { order_id } => println!("filled    #{order_id}"),
        Event::OrderCancelled { order_id } => println!("cancelled #{order_id}"),
        Event::OrderRejected { order_id, reason } => println!("rejected  #{order_id}: {reason:?}"),
    }
}

let mut engine = Engine::new(16);
engine.process_order(1, Side::Sell, 100, 10, &mut log_event);
engine.process_order(2, Side::Buy, 100, 4, &mut log_event);
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
        Event::OrderAccepted { order_id: 2 },
        Event::Trade { maker_order_id: 1, taker_order_id: 2, price_tick: 100, qty: 4 },
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

**Next:** [8. Ring buffer and WAL](08-ringbuffer-and-wal.md)
