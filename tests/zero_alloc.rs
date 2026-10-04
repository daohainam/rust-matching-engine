//! Verifies that `Engine::process_order`, `Engine::reduce_order` and
//! `Engine::cancel_order` never allocate on the heap, by installing a
//! counting global allocator for the whole test binary and asserting the
//! allocation counter is unchanged across a sustained processing loop.
//!
//! The counter is process-wide: it sees allocations from *every* thread, not
//! only the one under test. That is sound only while this is the binary's
//! single test, because the test harness runs tests on parallel threads.
//! Put any new test in a different file.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use matching_engine::{Engine, Event, EventBatch, Side};

struct CountingAllocator;

/// Allocations made by any thread of this process (see the module docs).
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn hot_path_performs_zero_heap_allocations() {
    // All pre-allocation happens here, before we start measuring. One batch
    // is reused (cleared before each call) as the event sink.
    let mut engine = Engine::new(50_000);
    let mut events = EventBatch::new();
    for i in 0..5_000u64 {
        let tick = (i % 2000) as u32;
        events.clear();
        engine.process_order(i, Side::Sell, 2000 + tick, 10, &mut events);
    }
    // A second book for one deep sweep, much longer than any bounded batch,
    // streamed through a closure sink.
    let mut deep_book = Engine::new(1_024);
    for i in 0..500u64 {
        events.clear();
        deep_book.process_order(i, Side::Sell, 100, 1, &mut events);
    }

    let before = ALLOC_COUNT.load(Ordering::SeqCst);

    // Resting orders (no matches).
    for i in 5_000..10_000u64 {
        let tick = (i % 500) as u32;
        events.clear();
        engine.process_order(i, Side::Buy, tick, 10, &mut events);
        std::hint::black_box(&events);
    }
    // Crossing orders (matches against pre-seeded asks).
    for i in 10_000..15_000u64 {
        let tick = 2000 + (i % 2000) as u32;
        events.clear();
        engine.process_order(i, Side::Buy, tick, 10, &mut events);
        std::hint::black_box(&events);
    }
    // Reductions that keep the orders resting.
    for i in 5_000..10_000u64 {
        events.clear();
        std::hint::black_box(engine.reduce_order(i, 1, &mut events));
    }
    // Cancellations across the resting book.
    for i in 5_000..10_000u64 {
        events.clear();
        std::hint::black_box(engine.cancel_order(i, &mut events));
    }
    // One taker sweeping 500 makers; every report reaches the sink.
    let mut trades = 0usize;
    deep_book.process_order(1_000, Side::Buy, 100, 500, &mut |event: Event| {
        if matches!(event, Event::Trade { .. }) {
            trades += 1;
        }
    });

    let after = ALLOC_COUNT.load(Ordering::SeqCst);
    assert_eq!(
        after, before,
        "process_order/cancel_order must not perform heap allocations"
    );
    assert_eq!(trades, 500);
}
