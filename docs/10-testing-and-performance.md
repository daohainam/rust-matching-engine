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
| `resting_order_with_no_match` | A non-crossing order rests; best bid and depth are right; only `OrderAccepted` (with the order's side, tick and quantity) is emitted. |
| `acceptance_repeats_the_order_terms_even_when_it_fills_at_once` | `OrderAccepted` describes the order even when it never rests, so the event stream alone says what was ordered. |
| `immediate_or_cancel_trades_what_it_can_and_expires_the_rest` | An IOC order trades, then reports its remainder with `OrderExpired`; nothing rests. |
| `immediate_or_cancel_that_cannot_trade_expires_without_a_slot` | With the arena full, an IOC order that cannot trade expires instead of getting `BookFull`: its remainder never needs a slot. |
| `market_buy_sweeps_asks_at_every_price_and_never_rests` | A market buy ignores the tick passed in (even an out-of-range one), trades at ticks 0, 2000 and 4095, and expires the rest. |
| `market_order_on_an_empty_side_only_expires` | A market sell with nothing to trade against emits `Accepted` (with limit 0) and `Expired`. |
| `post_only_that_would_trade_is_rejected_and_the_book_is_unchanged` | A post-only buy at the best ask, and a post-only sell at the best bid, are rejected with `PostOnlyWouldTrade` before acceptance. |
| `post_only_that_does_not_cross_rests_like_a_limit_order` | A post-only order below the best ask is accepted and rests. |
| `the_account_is_reported_on_acceptance_and_kept_while_resting` | `OrderAccepted` and `resting_order` both carry the order's account. |
| `self_trades_happen_unless_prevention_is_asked_for` | With the default mode, `Allow`, a buy trades with a resting sell of its own account like any other. |
| `cancel_resting_removes_the_own_order_and_keeps_matching` | `CancelResting` cancels the own resting order after `SelfTradePrevented`; the buy then rests its remainder. |
| `cancel_incoming_expires_the_remainder_even_of_a_limit_order` | `CancelIncoming` stops a limit buy at its own order: its remainder expires, and the own order stays. |
| `cancel_both_removes_the_own_order_and_stops_the_incoming_one` | `CancelBoth` does both, and leaves an empty book. |
| `full_fill_taker_and_maker` | An exact match emits `Trade`, `Filled(maker)`, `Filled(taker)` and leaves the book empty. |
| `partial_fill_leaves_maker_resting` | A smaller taker fills completely; the maker stays with reduced quantity. |
| `partial_fill_leaves_taker_resting_remainder` | A bigger taker consumes the maker, then rests its remainder. |
| `cancel_head_middle_tail_of_price_queue` | Cancelling from the head, middle and tail of a FIFO queue all work; a second cancel returns `false` and emits `CancelRejected`. |
| `cancel_reports_only_what_is_left_after_a_partial_fill` | `OrderCancelled` carries the side, the tick and the quantity still resting (6 of 10), not the original size. |
| `resting_order_follows_an_order_through_its_life` | `resting_order` shows an order's remaining quantity after a partial fill, and `None` once it is cancelled, for a taker that never rested, and for the reserved id. |
| `cancelling_a_filled_order_is_rejected_without_touching_the_book` | A cancel for an order that has already filled emits `CancelRejected` and leaves depth and live count alone. |
| `reducing_an_order_keeps_its_time_priority` | A reduced order emits `OrderReduced`, lowers the level's depth, and still fills before the order queued behind it. |
| `reducing_by_the_whole_remainder_cancels_the_order` | Reducing by more than is left cancels the order, with `OrderCancelled` for what was left. |
| `reduce_rejects_zero_and_orders_that_are_not_resting` | A reduction of 0 and an unknown id get `CancelRejected` with `ZeroQuantity` and `NotResting`; zero is checked first. |
| `cancel_account_removes_only_that_accounts_orders_in_book_order` | `cancel_account` cancels one account's bids best first, then its asks, and leaves the other account's orders alone. |
| `cancel_all_empties_the_book_and_always_reports` | `cancel_all` empties the book, and on an empty book still emits `MassCancelled` with a count of 0. |
| `a_restored_engine_keeps_priority_and_counters` | A snapshot lists bids best first, then asks, oldest first at each price; the restored engine fills a sweep in the same order, with trade ids that continue where the original's stopped. |
| `restore_refuses_snapshots_no_live_book_could_have` | `restore` rejects a zero capacity, too many orders, a duplicate id, a zero quantity, a level that would overflow and a crossed book, each with its own `SnapshotError`. |
| `a_wide_ladder_trades_and_restores_far_beyond_4096_ticks` | An engine with 300,000 ticks rests and finds orders at ticks 100, 4096 and 250,000, rejects tick 300,000, keeps its tick count through a snapshot, and gives a market buy the limit 299,999. |
| `risk_limits_reject_oversized_orders_and_prices_far_through_the_market` | `max_order_qty` and the price band reject with their own reasons, on both sides; an order exactly at the edge of the band trades, and resting orders are untouched. |
| `the_price_band_stops_a_market_order_sweep` | With a band of 5, a market buy against asks at 100, 101 and 150 gets the limit 105, trades at 100 and 101, and expires the rest; a snapshot keeps the limits. |
| `crosses_multiple_ask_price_levels_in_price_time_priority` | A buy sweeps asks from the cheapest level upward. |
| `trade_names_the_aggressor_side` | A sell taker's `Trade` reports `taker_side: Sell`. |
| `crosses_multiple_bid_price_levels_for_incoming_sell` | A sell sweeps bids from the highest level downward (the mirror case). |
| `trade_ids_count_up_across_calls` | `trade_id` counts 1, 2, 3 across calls; rejections, resting orders and cancels do not use up an id. |
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
| `pool.rs` | `the_account_fits_in_existing_padding`, `allocate_and_free_roundtrip`, `exhausts_capacity`, `fifo_queue_push_and_remove_head_mid_tail`, `double_free_is_detected`, `removing_from_an_empty_level_is_detected` | `Order` stays 48 bytes; freed slots can be allocated again and `live_count` tracks it; capacity is enforced; the linked list stays consistent when removing from head, middle and tail; a double free and an impossible removal panic in every build profile. |
| `bitset.rs` | `empty_bitset_has_no_extrema`, `set_and_query_single_tick`, `tracks_lowest_and_highest_across_groups`, `the_number_of_levels_follows_the_number_of_ticks`, `a_ladder_too_long_for_four_levels_is_rejected`, `agrees_with_an_ordered_set_at_every_size` | `lowest`/`highest` are right across 64-tick word boundaries (ticks 0, 63, 64, 2000, 4095) and after clears; 1 to 4 levels are used exactly when needed; and at eight sizes from 1 to 300,000 ticks, thousands of random sets and clears give the same answers as a `BTreeSet`. |
| `hashmap.rs` | `insert_get_remove`, `update_existing_key`, `survives_many_collisions_and_deletions`, `insert_rejects_reserved_sentinel_key_in_all_build_profiles`, `get_and_remove_are_safe_for_reserved_sentinel_key`, `keys_crafted_for_one_seed_spread_out_under_another`, `seeded_maps_behave_identically`, `insert_refuses_to_fill_the_last_empty_slot`, `absurd_capacity_is_rejected_instead_of_wrapping` | Basic operations and backward-shift deletion under collisions; the reserved key is handled; ids crafted against one seed scatter under another (chapter 5); the seed never changes a result; a full table and an absurd size fail loudly instead of hanging or wrapping. |
| `events.rs` | `full_batch_counts_dropped_events_until_cleared`, `closures_and_vectors_are_sinks` | A full `EventBatch` counts what it drops until `clear`; closures and `Vec`s work as sinks. |
| `ringbuffer.rs` | `push_pop_single_thread`, `capacity_rounds_up_to_a_power_of_two`, `zero_capacity_is_rejected`, `spsc_across_threads_preserves_order`, `consumer_reports_disconnect_only_after_draining`, `producer_sees_a_dropped_consumer` | FIFO order, full/empty behaviour and wrap-around; 100,000 values cross two real threads complete and in order (failing fast if the producer stops); disconnection is reported only once nothing is left to read. |
| `wal.rs` | `encode_decode_roundtrip`, `decode_rejects_zeroed_and_corrupted_records`, `append_grow_finalize_and_read_back`, `dropping_a_writer_finalizes_it`, `finalizing_an_empty_log_keeps_only_the_header`, `create_refuses_to_overwrite_an_existing_log`, `read_stops_at_zeroed_preallocated_tail`, `read_stops_before_a_torn_record`, `read_ignores_a_partial_trailing_record`, `open_append_resumes_after_the_last_valid_record`, `open_append_upgrades_older_logs`, `a_record_for_account_zero_keeps_its_version_2_bytes`, `open_append_refuses_a_log_written_for_another_capacity`, `a_second_writer_is_refused_while_the_first_is_live`, `replay_rebuilds_the_book_with_the_recorded_capacity`, `replay_from_finishes_what_a_snapshot_started`, `rejects_files_without_a_valid_header` | Records of every kind, order type, self-trade mode and account survive encoding; an unknown order type or mode, and a flipped bit anywhere in the 40 bytes, are caught; a record for account 0 is byte for byte what version 2 wrote; growth, `finalize` and `Drop` leave exactly the header plus the records; `create` never touches an existing file; readers stop at the first zeroed, torn or partial record; `open_append` continues after the last valid record, upgrades an older log it continues (recording its capacity), and refuses a log written for another capacity or tick count without touching it; a second writer gets `WouldBlock` and the first carries on; `replay` builds the engine with the recorded capacity and tick count, and refuses a log without a capacity; a risk limits record survives encoding; `replay_from` applies only the records after a snapshot; a bad header is `InvalidData`. |
| `snapshot.rs` | `encode_decode_roundtrip`, `a_version_1_checkpoint_is_still_read`, `damage_anywhere_is_detected`, `writing_replaces_the_checkpoint_in_one_step` | A checkpoint survives encoding, with and without orders, with its tick count and risk limits; an older checkpoint reads back with the defaults; every flipped bit and every truncation is caught; writing replaces the old checkpoint and leaves no `.tmp` file, even when a crashed write left one behind. |

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
levels per side, a `VecDeque` of `(order_id, qty)` per level, a `HashMap` of
live orders and a trade counter. It is short enough to check by eye.

The request generator (`OpGenerator` in [`tests/common/mod.rs`](../tests/common/mod.rs))
is seeded, so every run is reproducible. It produces:

- mostly placements in a narrow band of 16 ticks, so orders cross each other often;
  about three in four are limit orders, the rest immediate-or-cancel, market and
  post-only orders; each comes from one of four accounts, and one in five asks
  for self-trade prevention, so orders often meet their own account's orders;
- cancels and reductions (about 30% together), of live, filled and never-issued
  ids, and of the reserved id; a reduction is sometimes zero and often the whole
  remainder or more;
- rare risk limit changes (about one request in 500): a maximum quantity, a
  price band, both or neither;
- rare mass cancels (about one request in 200), mostly of one account, sometimes
  of the whole book;
- occasional edge inputs: zero quantity, out-of-range ticks, reused ids, sweeps
  across the whole book, and oversized quantities at far-away ticks that make a
  level's total overflow.

After **every** request the test compares the exact event stream (trade ids
included), the best bid and ask, the live order count, the depth at every tick the
generator can use, and `resting_order` for the order the request named and every
maker it traded with. Every
1,000 requests it also compares full `depth_snapshot`s. It runs 4 seeds × 20,000
requests with a tight arena (64 orders, so remainders are often rejected) and 4
more with an ample one (8,192 orders, so books get deep), all with the default
4096 ticks, and 2 more on a **300,000-tick ladder** (four bitset levels), where
sweeps and oversized orders reach both ends of the ladder.

Two details make it trustworthy:

- **Coverage counters.** The test counts how often interesting paths were hit
  (input rejections, rejected remainders, maker partial fills, successful cancels,
  calls producing more than 64 events, expired remainders, post-only rejections,
  reductions, prevented self-trades, orders removed by a mass cancel, risk limit
  rejections) and fails if
  any count is zero. A change to
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
engine. The replica must produce exactly the same events (so the same `trade_id` for
every trade), live count and depth.

The replica builds its own hash map, with its own **random seed**. The test
therefore also proves the claim from chapter 5: the seed changes where entries
are stored, never what the engine does. The replica is built by `wal::replay`,
from the capacity the log records, so the test also checks that the header
carries it.

`tests/snapshot.rs` makes the same kind of claim for snapshots (chapter 9.8). It
runs 10,000 generated requests, restores a second engine from a snapshot of the
first, and feeds both the next 10,000 requests: every call must produce exactly
the same events, and the two books must end identical. It runs once with a tight
arena and once with a roomy one.

## 10.4 Proving "zero allocation"

The claim *"`process_order`, `reduce_order` and `cancel_order` never allocate"*
is easy to write
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
     5,000 reductions by one lot
     5,000 cancellations
     one cancel_all of everything still resting
     one taker sweeping all 500 makers of the second engine,      through a counting closure
  4. after = ALLOC_COUNT
  5. assert_eq!(after, before) and that 500 trades were seen   <- not one allocation in between
```

Details worth copying when you write your own:

- **Warm up and pre-allocate first**, so one-time setup is not counted.
- **Exercise every path** you want to prove allocation-free: resting, matching,
  reducing, cancelling, mass cancelling and a sweep far deeper than any fixed
  buffer all take different code.
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
[criterion](https://docs.rs/criterion) library. Four benchmarks, each working
on a fresh engine:

| Benchmark | Setup (not timed) | Timed work | Orders per run |
|---|---|---|---|
| `place_10k_resting_orders` | empty engine, capacity 20,000 | 10,000 buys, each at tick `i % 4000`; nothing crosses, so every order rests | 10,000 |
| `place_10k_resting_orders_300k_ticks` | the same, with 300,000 ticks | the same, at ticks `i × 29 % 300,000`, spread over the whole ladder | 10,000 |
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

When the price ladder learned to have any number of levels (chapter 4.6), a run
on the same machine gave:

| Benchmark | Reported time (estimate) | Orders | ≈ per order |
|---|---|---|---|
| `place_10k_resting_orders` | 557 µs | 10,000 | ≈ 56 ns |
| `place_10k_resting_orders_300k_ticks` | 1.01 ms | 10,000 | ≈ 101 ns |
| `cross_spread_matching` | 381 µs | 5,000 | ≈ 76 ns |
| `cancel_orders` | 127 µs | 10,000 | ≈ 13 ns |

Two lessons hide in these numbers. First, criterion compared the generalized
bitset with the fixed two-level one (`cargo bench -- --save-baseline before`,
then `-- --baseline before`) and found **no change** for placing and matching:
with 4096 ticks the loop runs exactly the old two steps. Second, the wide ladder
costs about 45 ns more per order, and probably not because of its two extra
bit-scans: its orders are spread over 14 MB of price levels, so most of them miss
the cache. (`cancel_orders` also looked much faster than before, but that is
more likely the benchmark than the engine: `Engine` shrank from 1,168 to 272
bytes once the bitsets moved to the heap, and criterion's harness moves the engine
between setup and routine.)

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
| **Test** | `ubuntu-latest`, `windows-latest`, `ubuntu-24.04-arm`, Rust 1.98.1 | `cargo fmt --check`; `clippy -D warnings`; `cargo doc` with `-D warnings`; `cargo test` in debug and release; benchmarks compile; the `pipeline`, `walkthrough` and `checkpoint` examples run |
| **MSRV** | `ubuntu-latest`, Rust 1.89 | `cargo check --lib`: the `rust-version` promise in `Cargo.toml` |
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
