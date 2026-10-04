//! `matching_engine`: an ultra-low-latency, zero-allocation Central Limit
//! Order Book (CLOB) matching engine.
//!
//! # Architecture
//! - [`pool`]: pre-allocated order arena + intrusive doubly-linked FIFO
//!   queues (index-based, no pointer chasing).
//! - [`bitset`]: hierarchical 2-level bitset price ladder for O(1) best
//!   bid/ask resolution via hardware bit-scan intrinsics.
//! - [`hashmap`]: fixed-capacity, randomly seeded open-addressing flat hash
//!   map from order ID (`u64`) to arena slot (`u32`), for O(1) cancellation.
//! - [`engine`]: the single-threaded matching core tying the above
//!   together (`process_order`, `cancel_order`).
//! - [`events`]: execution report events, the [`EventSink`] trait the
//!   engine streams them through, and a bounded allocation-free
//!   [`EventBatch`] sink.
//! - [`ringbuffer`]: lock-free SPSC ring buffer (Disruptor-style) input
//!   queue with separate producer and consumer handles.
//! - [`wal`]: memory-mapped, checksummed binary write-ahead log for
//!   deterministic replay.
//!
//! All data structures used on the hot path (`Engine::process_order`,
//! `Engine::cancel_order`) are pre-allocated up front; the engine performs
//! no heap allocation, `malloc`, or `Box::new` while processing an order.
//! Events are streamed to a caller-supplied [`EventSink`], so a call stays
//! allocation-free as long as the sink does (for example a reused
//! [`EventBatch`] or a `Vec<Event>` with reserved capacity).

pub mod bitset;
pub mod engine;
pub mod events;
pub mod hashmap;
pub mod pool;
pub mod ringbuffer;
pub mod wal;

pub use engine::{Engine, NewOrder, OrderType, RestingOrder, SelfTradePrevention};
pub use events::{CancelRejectReason, Event, EventBatch, EventSink, RejectReason};
pub use pool::Side;

// Compiles and runs every Rust example in README.md as a doctest, so the
// README cannot drift from the API without failing `cargo test`.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;
