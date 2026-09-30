# rust-matching-engine

An ultra-low-latency, zero-allocation Central Limit Order Book (CLOB)
matching engine written in Rust.

## Design highlights

- **Single-threaded hot path** — `Engine::process_order` / `Engine::cancel_order`
  perform no locking and no heap allocation.
- **Pre-allocated order arena** (`src/pool.rs`) — a flat `Order` array with an
  O(1) free-list and an intrusive doubly-linked FIFO list (`next_idx` /
  `prev_idx`, `u32` indices instead of pointers) per price level.
- **Hierarchical 2-level bitset price ladder** (`src/bitset.rs`) — O(1) best
  bid / best ask resolution via hardware bit-scan (`tzcnt` / `lzcnt`) over a
  dense 4096-tick space.
- **Flat open-addressing hash map** (`src/hashmap.rs`) — O(1) `OrderId -> OrderIndex`
  lookup for cancellation, fixed capacity, no allocation after construction.
- **Matching engine core** (`src/engine.rs`) — price-time priority limit order
  book supporting full/partial fills, resting remainders, and multi-level
  spread crossing; streams `OrderAccepted`, `Trade`, `OrderPartiallyFilled`,
  `OrderFilled`, `OrderCancelled`, and `OrderRejected` events to a
  caller-supplied `EventSink`, so no execution report is ever dropped.
- **Lock-free SPSC ring buffer** (`src/ringbuffer.rs`) — Disruptor-style input
  queue for feeding the engine from a producer thread.
- **Memory-mapped binary WAL** (`src/wal.rs`) — fixed-width append-only log for
  deterministic state replay.

## Usage

```rust
use matching_engine::{Engine, Event, Side};

let mut engine = Engine::new(10_000); // pre-allocates all storage up front
// Any `EventSink` works: a `Vec<Event>`, a reusable `EventBatch`, or a closure.
// Reusing a sink with enough capacity keeps every call allocation-free.
let mut events: Vec<Event> = Vec::with_capacity(64);

engine.process_order(1, Side::Sell, 100, 10, &mut events); // resting ask
events.clear();
engine.process_order(2, Side::Buy, 100, 10, &mut events); // crosses and fills
for event in &events {
    println!("{event:?}");
}
```

## Testing & benchmarking

```sh
cargo test                    # unit + integration tests
cargo test --test zero_alloc  # verifies zero heap allocations on the hot path
cargo bench                   # criterion micro-benchmarks
```
