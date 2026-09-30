//! `matching_engine`: an ultra-low-latency, zero-allocation Central Limit
//! Order Book (CLOB) matching engine.
//!
//! # Architecture
//! - [`pool`]: pre-allocated order arena + intrusive doubly-linked FIFO
//!   queues (index-based, no pointer chasing).
//! - [`bitset`]: hierarchical 2-level bitset price ladder for O(1) best
//!   bid/ask resolution via hardware bit-scan intrinsics.
//! - [`hashmap`]: fixed-capacity open-addressing flat hash map for O(1)
//!   `OrderId -> OrderIndex` lookups (cancellation).
//! - [`engine`]: the single-threaded matching core tying the above
//!   together (`process_order`, `cancel_order`).
//! - [`events`]: allocation-free event batch describing what happened
//!   during a single engine call.
//! - [`ringbuffer`]: lock-free SPSC ring buffer (Disruptor-style) input
//!   queue for feeding the engine from a producer thread.
//! - [`wal`]: memory-mapped binary write-ahead log for deterministic
//!   replay.
//!
//! All data structures used on the hot path (`Engine::process_order`,
//! `Engine::cancel_order`) are pre-allocated up front; no heap allocation,
//! `malloc`, or `Box::new` occurs while processing an order.

pub mod bitset;
pub mod engine;
pub mod events;
pub mod hashmap;
pub mod pool;
pub mod ringbuffer;
pub mod wal;

pub use engine::Engine;
pub use events::{Event, EventBatch};
pub use pool::Side;
