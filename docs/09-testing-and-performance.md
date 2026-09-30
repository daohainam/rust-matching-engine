# 9. Testing and Performance

Sources: unit tests inside each `src/*.rs`, [`tests/zero_alloc.rs`](../tests/zero_alloc.rs),
[`benches/matching_engine_bench.rs`](../benches/matching_engine_bench.rs)

```sh
cargo test                     # 29 unit tests + 1 integration test
cargo test --no-fail-fast      # keep going after a failing test binary
cargo test --test zero_alloc   # just the zero-allocation proof
cargo bench                    # criterion micro-benchmarks
```

> [!NOTE]
> By default `cargo test` stops at the first test **binary** that fails, so a
> failing library test would hide `tests/zero_alloc.rs`. Use `--no-fail-fast`, or
> name the integration test explicitly as in the third line, when you want both
> results.

## 9.1 What each unit test proves

Every module except `events.rs` has its own `#[cfg(test)] mod tests` at the bottom
of the file (the event types are exercised through the engine's tests).
Reading them is a good way to learn the behaviour each module promises.

### `engine.rs` (13 tests)

| Test | What it proves |
|---|---|
| `resting_order_with_no_match` | A non-crossing order rests; best bid and depth are right; only `OrderAccepted` is emitted. |
| `full_fill_taker_and_maker` | An exact match emits `Trade`, `Filled(maker)`, `Filled(taker)` and leaves the book empty. |
| `partial_fill_leaves_maker_resting` | A smaller taker fills completely; the maker stays with reduced quantity. |
| `partial_fill_leaves_taker_resting_remainder` | A bigger taker consumes the maker, then rests its remainder. |
| `cancel_head_middle_tail_of_price_queue` | Cancelling from the head, middle and tail of a FIFO queue all work; a second cancel returns `false` with no events. |
| `crosses_multiple_ask_price_levels_in_price_time_priority` | A buy sweeps asks from the cheapest level upward. |
| `crosses_multiple_bid_price_levels_for_incoming_sell` | A sell sweeps bids from the highest level downward (the mirror case). |
| `price_time_priority_within_a_level_is_fifo` | Within one price, the older order fills first. |
| `rejects_duplicate_order_id_and_zero_qty` | Validation rejects a live duplicate id and `qty == 0`. |
| `depth_snapshot_orders_best_to_worst` | Snapshots are sorted best first. |
| `arena_exhaustion_rejects_resting_order` | A full arena rejects the order that would need to rest (`Accepted`, `Rejected`). |
| `rejects_reserved_order_id_sentinel` | `u64::MAX` is rejected, and cancelling it is safe. |
| `rejects_resting_order_that_would_overflow_level_total_qty` | A `u64::MAX` quantity fills a level; the next order there is rejected and leaves no dangling slot. |

### Other modules

| Module | Tests | What they prove |
|---|---|---|
| `pool.rs` | `allocate_and_free_roundtrip`, `exhausts_capacity`, `fifo_queue_push_and_remove_head_mid_tail` | Freed slots can be allocated again and `live_count` tracks it; capacity is enforced; the linked list stays consistent when removing from head, middle and tail; `total_qty` returns to 0. |
| `bitset.rs` | `empty_bitset_has_no_extrema`, `set_and_query_single_tick`, `tracks_lowest_and_highest_across_groups` | `lowest`/`highest` are right across 64-tick word boundaries (ticks 0, 63, 64, 2000, 4095) and after clears. |
| `hashmap.rs` | `insert_get_remove`, `update_existing_key`, `survives_many_collisions_and_deletions`, `insert_rejects_reserved_sentinel_key_in_all_build_profiles`, `get_and_remove_are_safe_for_reserved_sentinel_key` | Basic operations; 100 keys in a table sized for 64 with every other key removed (this exercises backward-shift deletion under collisions); the reserved key is handled. |
| `ringbuffer.rs` | `push_pop_single_thread`, `spsc_across_threads_preserves_order` | FIFO order, full/empty behaviour and wrap-around; 100,000 values across two real threads through a 1024-slot buffer arrive complete and in order. |
| `wal.rs` | `append_and_replay_roundtrip`, `finalize_truncates_file_and_closes_writer`, `finalize_on_empty_writer_leaves_empty_file` | Appends (including growth beyond the initial capacity) survive a write / read-back round trip, with the writer finalized by `Drop`. An explicit `finalize()` returns `Ok`, leaves the file exactly `len() * WAL_RECORD_SIZE` bytes long, can be repeated, and makes later `append` / `flush` return an error. Finalizing a writer with no records leaves an empty file. See [chapter 8](08-ringbuffer-and-wal.md#811-caveats-read-before-relying-on-the-wal). |

Current status on Windows 11 / Rust 1.98.1: **all 29 unit tests pass**, and so does
the zero-allocation integration test. The WAL tests pass on Windows because
`finalize` unmaps the file before truncating it.

## 9.2 What the tests do *not* cover

Knowing the gaps is part of reading tests critically.

| Gap | Why it matters | What would close it |
|---|---|---|
| **No randomized / differential test** | Hand-written cases cover the cases the author thought of. Bugs hide in combinations. | Compare against a simple reference book on random input (exercise 13). |
| **No test of event truncation** | The 64-event cap is silent ([chapter 7](07-events.md#74-what-happens-when-there-are-too-many-events)). | Exercise 4. |
| **Memory-ordering bugs are invisible on x86** | x86 CPUs are "strongly ordered": code that uses the wrong `Ordering` often works there and fails on ARM. The threaded ring-buffer test therefore proves less than it seems. | Run on ARM, or use the [`loom`](https://docs.rs/loom) model checker; run `cargo miri test ringbuffer` for undefined behaviour in the ring buffer's `unsafe` blocks (Miri cannot run the WAL test, which memory-maps a file). |
| **`depth_snapshot` edge cases** | Only the Buy side with more levels requested than exist is tested. Not covered: the Sell side, asking for fewer levels than exist, `levels = 0`, or a huge `levels` argument (it pre-allocates `Vec::with_capacity(levels)`). | A few more unit tests. |
| **WAL crash behaviour** | Nothing simulates a crash mid-write or a torn record. | Write a test that truncates or corrupts the file, then replays. |

## 9.3 Proving "zero allocation"

The claim *"`process_order` and `cancel_order` never allocate"* is easy to write
in a comment and easy to break later by accident (one innocent `Vec::new()` or
`.to_vec()`). So it is **tested**, by `tests/zero_alloc.rs`.

### The technique: a counting global allocator

Rust lets a program replace its global allocator. The test installs a wrapper
that counts every allocation and then delegates to the system allocator:

```rust
struct CountingAllocator;
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);   // count it...
        System.alloc(layout)                          // ...then really allocate
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;
```

`#[global_allocator]` affects the **whole test binary**: every `Box`, `Vec` and
`String` anywhere goes through it.

### The test itself

```text
  1. Build the engine and pre-seed 5,000 asks          <- allocations allowed here (setup)
  2. before = ALLOC_COUNT                              <- take a snapshot
  3. 5,000 resting buys (no match)                     <- the measured work:
     5,000 buys priced in the ask range (3,334 of     all three code paths
       them trade, the other 1,666 rest instead)
     5,000 cancellations
  4. after = ALLOC_COUNT
  5. assert_eq!(after, before)                         <- not one allocation in between
```

Details worth copying when you write your own:

- **Warm up and pre-allocate first**, so one-time setup is not counted.
- **Exercise every path** you want to prove allocation-free: resting, matching
  and cancelling take different code.
- **`std::hint::black_box`** around the results stops the optimiser from
  deleting calls whose results are unused.
- **`realloc` is covered too:** the default `GlobalAlloc::realloc` calls our
  `alloc`, so growing a `Vec` would be counted.

What it does **not** prove:

- It counts allocations **for the whole process** (fine here: it is the only test
  in its binary, so nothing else is allocating while it measures).
- It exercises the paths above, not every possible input (for example, a sweep
  of more than 31 makers, or the rejection paths).
- `depth_snapshot` *does* allocate and is deliberately excluded.

Run it in both profiles if you like; it passes in each:

```sh
cargo test --test zero_alloc
cargo test --release --test zero_alloc
```

## 9.4 The benchmarks

[`benches/matching_engine_bench.rs`](../benches/matching_engine_bench.rs) uses the
[criterion](https://docs.rs/criterion) library. Three benchmarks, each working
on a fresh engine:

| Benchmark | Setup (not timed) | Timed work | Orders per run |
|---|---|---|---|
| `place_10k_resting_orders` | empty engine, capacity 20,000 | 10,000 buys, each at tick `i % 4000`; nothing crosses, so every order rests | 10,000 |
| `cross_spread_matching` | 5,000 asks of size 1 at tick 100 | 5,000 buys of size 1 at tick 100; each consumes exactly one maker | 5,000 |
| `cancel_orders` | 10,000 resting buys | cancel all 10,000 by id | 10,000 |

### How criterion measures

```rust
b.iter_batched(
    || Engine::new(20_000),           // setup: builds fresh state, NOT timed
    |mut engine| { /* timed work */ engine },   // routine: timed; returns the engine
    BatchSize::LargeInput,            // "setup output is big, don't batch many together"
);
```

- **`iter_batched`** builds fresh state for each run of the routine, so the timing
  does not include `Engine::new` and each run starts from the same state. Returning
  `engine` from the routine makes its destruction happen outside the timed
  region.
- **`black_box`** keeps the compiler from optimising the work away.
- Criterion first **warms up** (3 s), then collects 100 samples and reports a
  **confidence interval**: `time: [low estimate high]`. The "estimate" is
  criterion's regression-slope estimate of the **time for one run of the routine**,
  meaning the whole batch of 5,000 or 10,000 orders, not one order. (It is close
  to, but not identical to, the plain mean: for these runs the means were 751,
  407 and 710 µs against slopes of 746, 390 and 734 µs.)

To get a per-order figure, **divide by the number of orders**.

### A sample run (for orientation only)

Measured on the machine used to write these docs: Intel Core i9-12900H, Windows
11, Rust 1.98.1, `cargo bench`, one run, no attempt to quiet the machine.

| Benchmark | Reported time (estimate) | Orders | ≈ per order |
|---|---|---|---|
| `place_10k_resting_orders` | 746 µs | 10,000 | ≈ 75 ns |
| `cross_spread_matching` | 390 µs | 5,000 | ≈ 78 ns |
| `cancel_orders` | 734 µs | 10,000 | ≈ 73 ns |

> [!IMPORTANT]
> Treat these as an order-of-magnitude sketch, **not a promise**. Your hardware,
> OS, power settings and background load change the numbers, and they should not
> be compared across machines.

### What these numbers do and do not tell you

- They are **averages over a batch**, i.e. *throughput*. They say nothing about the
  **tail**: the slowest 1 in 1,000 orders. For a latency-sensitive system the
  tail matters most; measuring it needs a per-call timer recording into a
  histogram (for example the `hdrhistogram` crate).
- The workload is **synthetic**: orders cycle through a few ticks, the working set
  is small and hot in cache, and there is no network and no WAL. Real traffic is
  messier.
- A chunk of each per-order figure is **not matching logic**: every call also
  initialises and returns a 2 KiB `EventBatch` ([chapter 7](07-events.md)).
  Exercise 7 lets you measure how much.
- Criterion is configured with `default-features = false` (its `html_reports`
  feature is off). In the run above only text output appeared in the terminal and
  no HTML report was produced; raw data is still written under
  `target/criterion/<benchmark>/`.
- Criterion may warn *"Unable to complete 100 samples in 5.0s"*. That is harmless:
  each routine is long (thousands of orders), so it simply takes a little longer.

## 9.5 Build profiles

From `Cargo.toml`:

```toml
[profile.release]
opt-level = 3        # full optimisation
lto = true           # optimise across crate boundaries (including dependencies like memmap2)
codegen-units = 1    # one big compilation unit: slower build, better optimisation
panic = "abort"      # no unwinding tables; a panic terminates the process (see below)

[profile.bench]
opt-level = 3
lto = true
```

- **`panic = "abort"`** expresses this project's philosophy: an invariant
  violation (`expect` in `PriceLevel`, the `assert!` in `FlatHashMap::insert`)
  should kill the process immediately instead of unwinding, in keeping with "fail
  loudly rather than continue with a corrupt book".

  > [!NOTE]
  > **This setting only takes effect in a binary you build from this package.**
  > `matching_engine` is a library. Cargo ignores a dependency's `[profile]`
  > settings, so a program that *uses* the crate must set `panic = "abort"` in
  > **its own** `Cargo.toml` to get this behaviour. Cargo also ignores the `panic`
  > setting for tests and benchmarks, which always unwind. (That is why the test
  > `insert_rejects_reserved_sentinel_key_in_all_build_profiles` can use
  > `#[should_panic]`.)
- **`lto` and `codegen-units = 1`** let the compiler optimise across crate
  boundaries, which matters for calls from the benchmark or your own binary into
  the library (and from the library into `memmap2`). `#[inline(always)]` already
  works inside the crate without them.
- To let the compiler use the newest bit-scan instructions (`tzcnt`, `lzcnt`;
  see [chapter 4](04-price-ladder-bitset.md)), build for your own CPU:

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench          # bash / zsh
```

```powershell
$env:RUSTFLAGS = "-C target-cpu=native"; cargo bench  # PowerShell
```

(A binary built this way may not run on older CPUs.)

## 9.6 Where to go from here with performance work

| Question | Tool |
|---|---|
| Where does the time go? | A sampling profiler: `perf` + flamegraphs on Linux, Visual Studio's profiler or `samply` on Windows. |
| Did the compiler really emit `tzcnt`? | `cargo asm` / `cargo show-asm`, or Godbolt. |
| Are there hidden allocations on other paths? | The counting allocator from 9.3; `heaptrack` / DHAT. |
| Is cache behaviour the bottleneck? | `perf stat` cache-miss counters, `valgrind --tool=cachegrind`. |
| What does the tail look like? | Record per-call durations into an HdrHistogram. |
| Is the ring buffer really correct on weak memory models? | Test on ARM; `loom`; `cargo miri`. |

**Next:** [10. Limitations and exercises](10-limitations-and-exercises.md)
