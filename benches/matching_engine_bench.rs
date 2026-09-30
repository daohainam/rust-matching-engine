//! Micro-benchmark harness demonstrating sustained order-processing
//! throughput. Run with `cargo bench`.
//!
//! To verify zero heap allocations on the hot path, run the accompanying
//! `tests/zero_alloc.rs` test (which uses a counting global allocator) via
//! `cargo test --test zero_alloc`.

use criterion::{criterion_group, criterion_main, Criterion};
use matching_engine::{Engine, EventBatch, Side};
use std::hint::black_box;

fn bench_resting_orders(c: &mut Criterion) {
    c.bench_function("place_10k_resting_orders", |b| {
        // `iter_batched` creates a fresh engine for each sample so the timed
        // closure measures order placement rather than engine initialization.
        // The event batch is reused across calls, as a production caller would.
        b.iter_batched(
            || (Engine::new(20_000), EventBatch::new()),
            |(mut engine, mut events)| {
                for i in 0..10_000u64 {
                    let tick = (i % 4000) as u32;
                    events.clear();
                    engine.process_order(i, Side::Buy, tick, 10, &mut events);
                    black_box(&events);
                }
                engine
            },
            criterion::BatchSize::LargeInput,
        );
    });
}

fn bench_matching_throughput(c: &mut Criterion) {
    c.bench_function("cross_spread_matching", |b| {
        // Seed the ask queue before each sample; the timed closure consumes
        // those makers with one-unit takers at the same price.
        b.iter_batched(
            || {
                let mut engine = Engine::new(20_000);
                let mut events = EventBatch::new();
                for i in 0..5_000u64 {
                    events.clear();
                    engine.process_order(i, Side::Sell, 100, 1, &mut events);
                }
                (engine, events)
            },
            |(mut engine, mut events)| {
                for i in 5_000..10_000u64 {
                    events.clear();
                    engine.process_order(i, Side::Buy, 100, 1, &mut events);
                    black_box(&events);
                }
                engine
            },
            criterion::BatchSize::LargeInput,
        );
    });
}

fn bench_cancel(c: &mut Criterion) {
    c.bench_function("cancel_orders", |b| {
        // Build the resting book outside the timed closure, then measure the
        // cost of looking up and unlinking every order by ID.
        b.iter_batched(
            || {
                let mut engine = Engine::new(20_000);
                let mut events = EventBatch::new();
                for i in 0..10_000u64 {
                    let tick = (i % 4000) as u32;
                    events.clear();
                    engine.process_order(i, Side::Buy, tick, 10, &mut events);
                }
                (engine, events)
            },
            |(mut engine, mut events)| {
                for i in 0..10_000u64 {
                    events.clear();
                    black_box(engine.cancel_order(i, &mut events));
                }
                engine
            },
            criterion::BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    bench_resting_orders,
    bench_matching_throughput,
    bench_cancel
);
criterion_main!(benches);
