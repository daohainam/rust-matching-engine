//! Verifies that `Engine::process_order` and `Engine::cancel_order` never
//! allocate on the heap, by installing a counting global allocator for the
//! whole test binary and asserting the allocation counter is unchanged
//! across a sustained processing loop.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use matching_engine::{Engine, Side};

struct CountingAllocator;

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
    // All pre-allocation happens here, before we start measuring.
    let mut engine = Engine::new(50_000);
    for i in 0..5_000u64 {
        let tick = (i % 2000) as u32;
        engine.process_order(i, Side::Sell, 2000 + tick, 10);
    }

    let before = ALLOC_COUNT.load(Ordering::SeqCst);

    // Resting orders (no matches).
    for i in 5_000..10_000u64 {
        let tick = (i % 500) as u32;
        std::hint::black_box(engine.process_order(i, Side::Buy, tick, 10));
    }
    // Crossing orders (matches against pre-seeded asks).
    for i in 10_000..15_000u64 {
        let tick = 2000 + (i % 2000) as u32;
        std::hint::black_box(engine.process_order(i, Side::Buy, tick, 10));
    }
    // Cancellations across the resting book.
    for i in 5_000..10_000u64 {
        std::hint::black_box(engine.cancel_order(i));
    }

    let after = ALLOC_COUNT.load(Ordering::SeqCst);
    assert_eq!(
        after, before,
        "process_order/cancel_order must not perform heap allocations"
    );
}
