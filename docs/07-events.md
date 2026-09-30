# 7. Events: How the Engine Reports What Happened

Source: [`src/events.rs`](../src/events.rs)

The engine does not call callbacks, write to a socket, or print anything. Every
call to `process_order` or `cancel_order` simply **returns an `EventBatch`**: a
list of things that happened, in order. The caller decides what to do with them
(publish market data, update accounts, write logs...).

This keeps the engine a pure, deterministic state machine:

```text
   (book state, request)  --->  Engine  --->  (new book state, events)
```

## 7.1 The `Event` enum

```rust
pub enum Event {
    OrderAccepted        { order_id: u64 },
    Trade                { maker_order_id: u64, taker_order_id: u64, price_tick: u32, qty: u64 },
    OrderPartiallyFilled { order_id: u64, remaining_qty: u64 },
    OrderFilled          { order_id: u64 },
    OrderCancelled       { order_id: u64 },
    OrderRejected        { order_id: u64 },
}
```

| Event | Emitted when | Who it is about |
|---|---|---|
| `OrderAccepted` | A new order passed validation. | the incoming order |
| `Trade` | Two orders executed against each other. | maker **and** taker, at the maker's price |
| `OrderPartiallyFilled` | An order traded but still has quantity left. | a maker (after each partial fill), or the taker (once, at the end) |
| `OrderFilled` | An order has nothing left. | a maker (when it is consumed), or the taker |
| `OrderCancelled` | A resting order was cancelled on request. | the cancelled order |
| `OrderRejected` | An order was refused: bad input, or no room for it to rest. | the incoming order |

`Event` derives `Debug, Clone, Copy, PartialEq, Eq`, so events are cheap to copy
and trivial to compare in tests.

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
  *maker's* new state, unless the batch overflowed right at that point (see 7.4).
- The **taker's** `OrderPartiallyFilled` or `OrderFilled` appears **once**, at the
  end, not after every trade. To follow a taker's progress, add up the `qty` of
  its `Trade` events.
- A taker that does not trade at all and simply rests produces **only**
  `OrderAccepted`. There is no separate "order is now resting" event.
- Rejections caused by bad input come **alone**; rejections caused by capacity
  come **after** `OrderAccepted`, and only for an order that did not trade
  (see [6.4](06-matching-algorithm.md#64-phase-4-resting-the-remainder)).

### Cookbook of event sequences

These are taken straight from the unit tests in `engine.rs`.

| Scenario | Events |
|---|---|
| Order rests, nothing crosses | `Accepted` |
| Exact full match, taker = maker | `Accepted`, `Trade`, `Filled(maker)`, `Filled(taker)` |
| Taker smaller than maker | `Accepted`, `Trade`, `PartiallyFilled(maker, left)`, `Filled(taker)` |
| Taker bigger than maker; remainder rests | `Accepted`, `Trade`, `Filled(maker)`, `PartiallyFilled(taker, left)` |
| Taker sweeps several levels | `Accepted`, then `Trade`+maker-event per fill, then the taker's final event |
| Invalid input / duplicate id | `Rejected` |
| Order can't rest (arena full / overflow) | `Accepted`, `Rejected` (no trades happened) |
| Cancel succeeds | `Cancelled` |
| Cancel of unknown id | *(no events; the returned `bool` is `false`)* |

## 7.3 `EventBatch`: a list that never allocates

A `Vec<Event>` would allocate on the first push, which we forbid on the hot
path. Instead:

```rust
pub const MAX_EVENTS_PER_CALL: usize = 64;

pub struct EventBatch {
    events: [Event; MAX_EVENTS_PER_CALL],   // fixed array, lives on the stack
    len: usize,                             // how many are valid
}
```

```text
   events:  [ e0 | e1 | e2 | e3 | ... unused slots (stale or default values) ... ]     len = 4
              ^^^^^^^^^^^^^^^^^^^
              as_slice() returns only these
```

- `push` writes at `events[len]` and increments `len`, **unless the array is full**.
- `as_slice()`, `len()`, `is_empty()`, `clear()` and `IntoIterator for &EventBatch`
  give convenient access.
- `clear()` only resets `len` to 0; it does not wipe the array.

**Sizes** (measured): one `Event` is 32 bytes (the biggest variant, `Trade`, holds
`8 + 8 + 4 + 8 = 28` bytes plus the tag and padding). An `EventBatch` is
`64 × 32 + 8 = 2056` bytes.

> [!TIP]
> **Zero allocation is not zero cost.** Each call creates a fresh 2 KiB batch and
> returns it by value. No `malloc` happens, but the initialisation and the copy
> are real memory traffic on every order, even one that produces a single event.
> A common refinement (see the exercises) is for the *caller* to own one batch
> and pass `&mut EventBatch` into the engine, reusing it forever.

## 7.4 What happens when there are too many events?

`MAX_EVENTS_PER_CALL` is 64. Pushing a 65th event is **silently dropped**; the
engine never panics or allocates, and it keeps matching. That decision is
intentional (no panic on the hot path) but it has a consequence worth knowing.

Each maker touched costs two events (`Trade` + `OrderFilled`, or `Trade` +
`OrderPartiallyFilled` for a maker left partly filled), plus one `OrderAccepted`
at the start and one final taker event:

```text
   events = 1 + 2 × (makers touched) + 1      <=  64
                                               =>  at most 31 makers fully reported
```

Verified against the real engine (every maker below is consumed completely):

| Makers swept by one taker | Events retained | Last retained event |
|---|---|---|
| 30 | 62 | `OrderFilled` (taker) |
| 31 | 64 | `OrderFilled` (taker) |
| 32 | 64 | `Trade` with maker 32 (**the `OrderFilled` events for maker 32 and the taker were dropped**) |
| 40 | 64 | `Trade` with maker 32 (**8 makers' trades missing**) |

In the 40-maker case the **book is still correct** (all 40 makers are gone, the
taker is filled), but the **event stream is incomplete**. Anything that relies on
events to reconcile positions or publish market data would miss 8 trades.

Options if you need more headroom:

1. Raise `MAX_EVENTS_PER_CALL`. The cost grows linearly: every call initialises a
   bigger batch.
2. Add an `overflowed` flag to `EventBatch` so consumers at least *know* (an
   exercise in chapter 10).
3. Cap the number of fills per call, or split large orders upstream.

## 7.5 Consuming events

```rust
use matching_engine::{Event, EventBatch};

fn log_events(batch: &EventBatch) {
    for event in batch {
        match *event {
            Event::OrderAccepted { order_id } => println!("accepted  #{order_id}"),
            Event::Trade { maker_order_id, taker_order_id, price_tick, qty } => println!(
                "trade     {qty} @ {price_tick} (maker #{maker_order_id}, taker #{taker_order_id})"
            ),
            Event::OrderPartiallyFilled { order_id, remaining_qty } => {
                println!("partial   #{order_id}, {remaining_qty} left")
            }
            Event::OrderFilled { order_id } => println!("filled    #{order_id}"),
            Event::OrderCancelled { order_id } => println!("cancelled #{order_id}"),
            Event::OrderRejected { order_id } => println!("rejected  #{order_id}"),
        }
    }
}
```

Testing with events (this is how the engine's own tests are written):

```rust
let mut engine = Engine::new(16);
engine.process_order(1, Side::Sell, 100, 10);
let events = engine.process_order(2, Side::Buy, 100, 4);

assert_eq!(
    events.as_slice(),
    &[
        Event::OrderAccepted { order_id: 2 },
        Event::Trade { maker_order_id: 1, taker_order_id: 2, price_tick: 100, qty: 4 },
        Event::OrderPartiallyFilled { order_id: 1, remaining_qty: 6 },
        Event::OrderFilled { order_id: 2 },
    ]
);
```

**Next:** [8. Ring buffer and WAL](08-ringbuffer-and-wal.md)
