# 10. Testing and Performance

> **New word?** Every technical term in these chapters is explained in the
> [glossary](glossary.md).

Sources: the unit tests inside each `src/*.rs`, the integration tests in
[`tests/`](../tests), the doctests, [`benches/matching_engine_bench.rs`](../benches/matching_engine_bench.rs)
and [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)

```sh
cargo test                      # everything: unit, integration and doc tests
cargo test --no-fail-fast       # keep going after a failing test binary
cargo test --test model_check   # just the differential test
cargo test --test zero_alloc    # just the zero-allocation proof
cargo test -- --list            # list every test by name
cargo bench                     # criterion micro-benchmarks
```

> [!NOTE]
> By default `cargo test` stops at the first test **binary** that fails, so a
> failing library test would hide the integration tests. Use `--no-fail-fast`, or
> name one test binary explicitly as above, when you want every result.

## 10.1 What each unit test proves

Every module has its own `#[cfg(test)] mod tests` at the bottom of the file.
Reading them is a good way to learn the behaviour each module promises.

### `engine.rs`

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
| `rejects_duplicate_order_id_and_zero_qty` | Validation rejects a live duplicate id (`DuplicateOrderId`) and `qty == 0` (`ZeroQuantity`). |
| `rejects_out_of_range_price_tick` | A tick of 4096 is rejected with `PriceOutOfRange` and leaves the book empty. |
| `input_checks_run_in_a_fixed_order` | An order with several problems gets the reason of the first check (`ZeroQuantity`). |
| `depth_snapshot_orders_best_to_worst` | Snapshots are sorted best first. |
| `depth_queries_accept_any_input_without_panicking` | `depth_at` with an out-of-range tick returns 0, and `depth_snapshot(side, usize::MAX)` returns every level on both sides. |
| `arena_exhaustion_rejects_resting_order` | A full arena rejects the order that would need to rest (`Accepted`, `Rejected(BookFull)`). |
| `rejects_reserved_order_id_sentinel` | `u64::MAX` is rejected with `ReservedOrderId`, and cancelling it is safe. |
| `rejects_resting_order_that_would_overflow_level_total_qty` | A `u64::MAX` quantity fills a level; the next order there is rejected with `LevelQuantityOverflow` and leaves no dangling slot. |
| `deep_sweep_reports_every_execution` | One taker sweeping 40 makers reports every trade and fill through a `Vec` sink: the regression test for the old 64-event cap. |
| `bounded_batch_sink_reports_truncation` | The same sweep into an `EventBatch` keeps 64 events and counts the other 18 as dropped. |
| `panicking_sink_leaves_the_book_consistent` | A sink that panics on a maker's fill (caught with `catch_unwind`) leaves no stale best price behind. |

### Other modules

| Module | Tests | What they prove |
|---|---|---|
| `pool.rs` | `allocate_and_free_roundtrip`, `exhausts_capacity`, `fifo_queue_push_and_remove_head_mid_tail`, `double_free_is_detected`, `removing_from_an_empty_level_is_detected` | Freed slots can be allocated again and `live_count` tracks it; capacity is enforced; the linked list stays consistent when removing from head, middle and tail; a double free and an impossible removal panic in every build profile. |
| `bitset.rs` | `empty_bitset_has_no_extrema`, `set_and_query_single_tick`, `tracks_lowest_and_highest_across_groups` | `lowest`/`highest` are right across 64-tick word boundaries (ticks 0, 63, 64, 2000, 4095) and after clears. |
| `hashmap.rs` | `insert_get_remove`, `update_existing_key`, `survives_many_collisions_and_deletions`, `insert_rejects_reserved_sentinel_key_in_all_build_profiles`, `get_and_remove_are_safe_for_reserved_sentinel_key`, `keys_crafted_for_one_seed_spread_out_under_another`, `seeded_maps_behave_identically`, `insert_refuses_to_fill_the_last_empty_slot`, `absurd_capacity_is_rejected_instead_of_wrapping` | Basic operations and backward-shift deletion under collisions; the reserved key is handled; ids crafted against one seed scatter under another (chapter 5); the seed never changes a result; a full table and an absurd size fail loudly instead of hanging or wrapping. |
| `events.rs` | `full_batch_counts_dropped_events_until_cleared`, `closures_and_vectors_are_sinks` | A full `EventBatch` counts what it drops until `clear`; closures and `Vec`s work as sinks. |
| `ringbuffer.rs` | `push_pop_single_thread`, `capacity_rounds_up_to_a_power_of_two`, `zero_capacity_is_rejected`, `spsc_across_threads_preserves_order`, `consumer_reports_disconnect_only_after_draining`, `producer_sees_a_dropped_consumer` | FIFO order, full/empty behaviour and wrap-around; 100,000 values cross two real threads complete and in order (failing fast if the producer stops); disconnection is reported only once nothing is left to read. |
| `wal.rs` | `encode_decode_roundtrip`, `decode_rejects_zeroed_and_corrupted_records`, `append_grow_finalize_and_read_back`, `dropping_a_writer_finalizes_it`, `finalizing_an_empty_log_keeps_only_the_header`, `create_refuses_to_overwrite_an_existing_log`, `read_stops_at_zeroed_preallocated_tail`, `read_stops_before_a_torn_record`, `read_ignores_a_partial_trailing_record`, `open_append_resumes_after_the_last_valid_record`, `rejects_files_without_a_valid_header` | Records survive encoding; a flipped bit anywhere is caught; growth, `finalize` and `Drop` leave exactly the header plus the records; `create` never touches an existing file; readers stop at the first zeroed, torn or partial record; `open_append` continues after the last valid record; a bad header is `InvalidData`. |

The WAL's crash tests do not crash anything. They write **crash images** by hand
(a valid header followed by chosen bytes: a zero tail, a torn record, half a
record) and check what the readers make of them. That is deterministic and fast,
and it covers the states a crash can leave behind.

### Doctests

`cargo test` also compiles and runs the code in documentation comments:

| Where | What it checks |
|---|---|
| `README.md` (included by `src/lib.rs`) | The quick start and API examples compile and run, so the README cannot drift from the API. |
| `src/events.rs` module docs | The three kinds of event sink, exactly as chapter 7 shows them. |
| `src/ringbuffer.rs` module docs | A producer thread and a consumer that stops on disconnection, plus two `compile_fail` examples: cloning a `Producer`, and pushing through a shared `&Producer`, must **not** compile (chapter 8.5). |

A `compile_fail` doctest is how you test a *type-level* guarantee: `cargo test`
fails if the forbidden code ever starts compiling.

## 10.2 Differential testing: `tests/model_check.rs`

Hand-written tests only cover the cases their author thought of; bugs hide in
combinations. The strongest test in this repository therefore compares the
engine with a second, **deliberately naive** order book on long random request
streams, and requires them to agree on everything observable.

The reference book is written for obviousness, not speed: a `BTreeMap` of price
levels per side, a `VecDeque` of `(order_id, qty)` per level, and a `HashMap` of
live orders. It is short enough to check by eye.

The request generator (`OpGenerator` in [`tests/common/mod.rs`](../tests/common/mod.rs))
is seeded, so every run is reproducible. It produces:

- mostly placements in a narrow band of 16 ticks, so orders cross each other often;
- cancels (about 30%), of live, filled and never-issued ids, and of the reserved id;
- occasional edge inputs: zero quantity, out-of-range ticks, reused ids, sweeps
  across the whole book, and oversized quantities at far-away ticks that make a
  level's total overflow.

After **every** request the test compares the exact event stream, the best bid and
ask, the live order count and the depth at every tick the generator can use. Every
1,000 requests it also compares full `depth_snapshot`s. It runs 4 seeds × 20,000
requests with a tight arena (64 orders, so remainders are often rejected) and 4
more with an ample one (8,192 orders, so books get deep).

Two details make it trustworthy:

- **Coverage counters.** The test counts how often interesting paths were hit
  (input rejections, rejected remainders, maker partial fills, successful cancels,
  calls producing more than 64 events) and fails if any count is zero. A change to
  the generator that silently stops exercising a path cannot go unnoticed.
- **Reproducible failures.** Every assertion message includes the seed, the step
  and the request, so a failure can be replayed exactly.

This is the single best way to gain confidence in an optimised data structure:
the fast implementation and the obvious one must never disagree.

## 10.3 Replay determinism: `tests/wal_replay.rs`

This test runs 10,000 generated requests through a primary engine while logging
each one to a WAL *before* applying it (with a small preallocation, so the file
grows several times). It finalizes the log, reads it back, checks that the
sequence numbers run from 0 to 9,999, and replays the records into a **second**
engine. The replica must produce exactly the same events, live count and depth.

The replica builds its own hash map, with its own **random seed**. The test
therefore also proves the claim from chapter 5: the seed changes where entries
are stored, never what the engine does.

## 10.4 Proving "zero allocation"

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
  1. Build an engine with 5,000 pre-seeded asks, and a second  <- allocations allowed here (setup)
     engine with 500 one-lot asks at a single price
  2. before = ALLOC_COUNT                                      <- take a snapshot
  3. 5,000 resting buys (no match)                             <- the measured work,
     5,000 buys priced in the ask range                           through one reused EventBatch
     5,000 cancellations
     one taker sweeping all 500 makers of the second engine,      through a counting closure
  4. after = ALLOC_COUNT
  5. assert_eq!(after, before) and that 500 trades were seen   <- not one allocation in between
```

Details worth copying when you write your own:

- **Warm up and pre-allocate first**, so one-time setup is not counted.
- **Exercise every path** you want to prove allocation-free: resting, matching,
  cancelling and a sweep far deeper than any fixed buffer all take different code.
- **Choose sinks that do not allocate.** The measurement is about the engine, so
  the test reuses one `EventBatch` (calling `clear` before each call) and uses a
  closure that only counts.
- **`std::hint::black_box`** around the results stops the optimiser from
  deleting calls whose results are unused.
- **`realloc` is covered too:** the default `GlobalAlloc::realloc` calls our
  `alloc`, so growing a `Vec` would be counted.

What it does **not** prove:

- It counts allocations **for the whole process**, from every thread. That is
  only sound while it is the single test in its binary (the file says so), since
  the test harness runs tests on parallel threads.
- It exercises the paths above, not every possible input (the rejection paths,
  for example).
- `depth_snapshot` *does* allocate and is deliberately excluded.

Run it in both profiles if you like; it passes in each:

```sh
cargo test --test zero_alloc
cargo test --release --test zero_alloc
```

## 10.5 Build profiles

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
  violation (an `expect` in `PriceLevel`, the `assert!`s in `FlatHashMap::insert`
  and `OrderPool::free`) should kill the process immediately instead of
  unwinding, in keeping with "fail loudly rather than continue with a corrupt
  book". Recovery is then a WAL replay (chapter 9).

  > [!NOTE]
  > **This setting only takes effect in a binary you build from this package.**
  > `matching_engine` is a library. Cargo ignores a dependency's `[profile]`
  > settings, so a program that *uses* the crate must set `panic = "abort"` in
  > **its own** `Cargo.toml` to get this behaviour. Cargo also ignores the `panic`
  > setting for tests and benchmarks, which always unwind. (That is why tests such
  > as `double_free_is_detected` can use `#[should_panic]`, and why
  > `panicking_sink_leaves_the_book_consistent` can catch a panic.)
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

## 10.6 The benchmarks

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
    || (Engine::new(20_000), EventBatch::new()),   // setup: fresh state, NOT timed
    |(mut engine, mut events)| {                    // routine: timed
        for i in 0..10_000u64 {
            events.clear();                          // one reused sink, as a real caller would
            engine.process_order(i, Side::Buy, (i % 4000) as u32, 10, &mut events);
            black_box(&events);
        }
        engine                                       // returned, so it is dropped outside the timing
    },
    BatchSize::LargeInput,                           // "setup output is big, don't batch many together"
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
  meaning the whole batch of 5,000 or 10,000 orders, not one order.

To get a per-order figure, **divide by the number of orders**.

### A sample run (for orientation only)

Measured on the machine used to write these docs: Intel Core i9-12900H, Windows
11, Rust 1.98.1, `cargo bench`, one run, no attempt to quiet the machine.

| Benchmark | Reported time (estimate) | Orders | ≈ per order |
|---|---|---|---|
| `place_10k_resting_orders` | 495 µs | 10,000 | ≈ 50 ns |
| `cross_spread_matching` | 346 µs | 5,000 | ≈ 69 ns |
| `cancel_orders` | 403 µs | 10,000 | ≈ 40 ns |

An earlier version of this chapter recorded 746, 390 and 734 µs on the same
machine, with the old API that returned a fresh 2 KiB `EventBatch` from every
call. Criterion compares each run with the previous one and reported the new
numbers as improvements of about 36%, 13% and 46%. The cheapest operations gained
the most, which is what you would expect if initialising and copying that batch
was a large share of their cost: a measured version of chapter 7's point that
"zero allocation" is not "zero cost". With one run on a laptop, read this as
"clearly faster", not as exact percentages.

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
- Each call writes its events into one reused `EventBatch`. An earlier version of
  the engine returned a fresh 2 KiB batch by value from every call instead; the
  sink API removed that copy from every order (chapter 7).
- Criterion is configured with `default-features = false` (its `html_reports`
  feature is off), so it prints text only; raw data is still written under
  `target/criterion/<benchmark>/`.
- Criterion may warn *"Unable to complete 100 samples in 5.0s"*. That is harmless:
  each routine is long (thousands of orders), so it simply takes a little longer.

## 10.7 Continuous integration

Every push and pull request runs [`.github/workflows/ci.yml`](../.github/workflows/ci.yml):

| Job | Runs on | Checks |
|---|---|---|
| **Test** | `ubuntu-latest`, `windows-latest`, `ubuntu-24.04-arm`, Rust 1.98.1 | `cargo fmt --check`; `clippy -D warnings`; `cargo doc` with `-D warnings`; `cargo test` in debug and release; benchmarks compile; `examples/pipeline.rs` runs |
| **MSRV** | `ubuntu-latest`, Rust 1.88 | `cargo check --lib`: the `rust-version` promise in `Cargo.toml` |
| **Miri** | `ubuntu-latest`, nightly | `cargo miri test --lib`: undefined behaviour in the `unsafe` code (tests that map files are skipped) |

Each choice exists to catch a specific kind of failure:

- **Windows** memory-maps files differently (it refuses to shrink a mapped file),
  so the WAL is tested there as well as on Linux.
- **ARM** reorders memory accesses far more freely than x86. A ring buffer with a
  missing `Acquire` usually still passes on x86 and can fail on ARM, so the
  threaded SPSC test runs on real ARM hardware too. That is still probabilistic;
  `loom` (chapter 11) would be exhaustive.
- **A pinned toolchain, a committed `Cargo.lock`, and `--locked`** make CI
  reproducible: a new compiler lint or a new dependency release cannot turn the
  build red without a change in this repository. Upgrades become deliberate
  commits.
- **Timeouts** make a hung test fail in minutes, not hours.
- **The README doctest and `tests/docs_sync.rs`** keep the docs honest: the
  README's examples and chapter 9's complete program are compiled and run, so they
  cannot silently fall out of date.

## 10.8 What the tests do *not* cover

Knowing the gaps is part of reading tests critically.

| Gap | Why it matters | What would close it |
|---|---|---|
| **Memory-ordering bugs are probabilistic** | The ARM job runs the threaded ring-buffer test on a weakly ordered CPU, and Miri checks the `unsafe` code, but both only see the thread interleavings that happen to occur. | The [`loom`](https://docs.rs/loom) model checker explores interleavings exhaustively (an exercise in chapter 11). |
| **Real crashes and power loss** | The WAL tests use hand-built crash images; nothing kills a process mid-write or cuts the power. | A harness that runs a writer in a child process and kills it at random points; power-loss testing needs a VM or a fault-injecting block device. |
| **Performance regressions** | CI only *compiles* the benchmarks, so a slowdown would go unnoticed. | Criterion baselines (`cargo bench -- --save-baseline before`, later `--baseline before`), or a benchmark job on a quiet, dedicated machine. |
| **`depth_snapshot` corner cases** | `model_check` compares full snapshots and a unit test covers `usize::MAX`, but `levels = 0` and "fewer levels than exist" are untested. | Two small unit tests. |
| **Two writers on one WAL file** | It is documented as unsupported, but nothing enforces or tests it. | A file lock (`File::try_lock`) and a test for it (chapter 11). |

## 10.9 Where to go from here with performance work

| Question | Tool |
|---|---|
| Where does the time go? | A sampling profiler: `perf` + flamegraphs on Linux, Visual Studio's profiler or `samply` on Windows. |
| Did the compiler really emit `tzcnt`? | `cargo asm` / `cargo show-asm`, or Godbolt. |
| Are there hidden allocations on other paths? | The counting allocator from 10.4; `heaptrack` / DHAT. |
| Is cache behaviour the bottleneck? | `perf stat` cache-miss counters, `valgrind --tool=cachegrind`. |
| What does the tail look like? | Record per-call durations into an HdrHistogram. |
| Is the ring buffer really correct on weak memory models? | The ARM CI job and Miri (both in place); `loom` for an exhaustive check. |

**Next:** [11. Limitations and exercises](11-limitations-and-exercises.md)
