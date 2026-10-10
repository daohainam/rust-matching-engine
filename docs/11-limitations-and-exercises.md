# 11. Known Limitations, Sharp Edges and Exercises

> **New word?** Every technical term in these chapters is explained in the
> [glossary](glossary.md).

This is a teaching project, and it is more useful to learn from its rough edges
than to pretend it has none. Everything below was found by reading the code and
(where noted) confirmed by running it.

The chapter has three parts. **11.1** lists what is still rough. **11.2** is a
set of graded exercises for you to try. **11.3** retraces the bugs that earlier
versions had: each one was fixed in a small commit you can study, and some make
good exercises in their own right.

## 11.1 Known limitations and sharp edges

### Sharp edges that remain

| Where | Issue | How bad | Chapter |
|---|---|---|---|
| `wal.rs` | The durability limits: a failed flush is final, a new file's directory entry is not synced on Unix, a full disk becomes `SIGBUS` on Linux, `create` is not atomic, damage in the middle of a log is treated like a torn tail, and the writer lock stops only other `WalWriter`s, not other programs. | Medium for a real deployment; fine for learning, as long as you know them. | [9.6](09-write-ahead-log.md#96-caveats-read-before-relying-on-the-wal) |
| `wal.rs` | A log written before format version 4 does not record its engine capacity until `open_append` continues it; replaying it by hand with the wrong capacity still gives a different book. | Low: `replay` refuses such a log instead of guessing. | [9.6](09-write-ahead-log.md#96-caveats-read-before-relying-on-the-wal) |
| `events.rs` | An `EventBatch` keeps only 64 events per call. It counts the rest, so this is no longer silent, but you must check `is_truncated()`. | Low: check it, or use an unbounded sink. | [7.4](07-events.md#74-what-happens-when-there-are-too-many-events) |
| `engine.rs` | A sink that panics (with the panic caught) leaves a consistent book but a **partly applied** request. | Low: rebuild from the WAL; a binary built with `panic = "abort"` stops instead (your own profile decides, [10.5](10-testing-and-performance.md#105-build-profiles)). | [7.5](07-events.md#75-when-a-sink-panics) |
| `ringbuffer.rs` | Its memory ordering is checked on ARM hardware and by Miri, but only for the interleavings that happen to occur. | Low | [10.8](10-testing-and-performance.md#108-what-the-tests-do-not-cover) |
| `hashmap.rs` | The seed protects against crafted ids only while it stays secret; `with_capacity_and_seed` with a guessable seed gives that up. | Low: the engine always uses a random seed. | [5.7](05-hash-map.md#57-complexity) |

### Design limits (by choice, not bugs)

| Limit | Detail |
|---|---|
| **Price ticks chosen per engine** | 4096 by default, up to 2^24 with `Engine::with_ticks`; memory grows by about 48 bytes per tick. No tick-size or price offset logic; callers convert. |
| **Fixed order capacity** | Chosen in `Engine::new`. A full arena rejects orders that would need to rest (orders that fully match still succeed). |
| **Four order types** | Limit, immediate-or-cancel, market and post-only. No fill-or-kill, stop, iceberg or day orders. Quantity can be reduced in place, but raising it or changing the price means cancel + new, which loses priority. Market orders are protected only when a price band is set. |
| **Risk limits are engine-wide** | One maximum quantity and one price band for every order. The band is measured from the best opposing price, so it does not apply when that side is empty; no per-account limits, no reference price such as the last trade. |
| **Accounts are labels** | Every order has an account, used for self-trade prevention and mass cancels. There are no per-account limits (open orders, position, credit), and the engine keeps no list of an account's orders, so `cancel_account` walks the book. |
| **One instrument per `Engine`** | Run several engines for several symbols. |
| **Engine is not wired to the WAL or ring buffer** | The caller does it; [`examples/pipeline.rs`](../examples/pipeline.rs) shows how, and `WalRecord::apply_to`, `wal::replay` and `wal::replay_from` do the replaying. The WAL's `sequence` is caller-supplied and independent of the engine's own counter. |
| **WAL format is small** | Version 6 has a checksummed header with the engine capacity and tick count, explicit little-endian fields, per-record checksums and a writer lock, but no fsync policy of its own and no log rotation ([9.6](09-write-ahead-log.md#96-caveats-read-before-relying-on-the-wal)). |
| **Snapshots pause matching** | `Engine::snapshot` walks the whole book on the matching thread, so a checkpoint delays the requests behind it. Take them between requests, not too often (exercise 17). |
| **Failing loudly** | `assert!`/`expect` on invariants panic on a bookkeeping bug, in every build profile. With `panic = "abort"` (set in this repo's release profile, and needed in *your* binary's profile because Cargo ignores a dependency's) that aborts the process. Intentional for a financial component. |
| **`depth_snapshot` is slow-ish** | It allocates a `Vec` and scans ticks linearly (documented in the source). Reporting only. |

### Performance notes (not defects)

- `Order` is **48 bytes**: neither cache-line sized nor a divisor of 64, so many
  records straddle two lines ([chapter 3](03-order-pool.md#33-the-order-record)).
- `Order.sequence` is stored but never read. `Order.in_use` is read only by the
  double-free check in `OrderPool::free`. Both cost bytes, but help debugging and
  auditing.

## 11.2 Exercises

Difficulty: 🟢 warm-up · 🟡 a solid afternoon · 🔴 a real project. Exercises are
grouped by topic; inside each group they go from easier to harder. Hints are
deliberately short, because the chapters contain the ingredients.

Work on a branch (`git switch -c my-exercise`) so `main` stays as the chapters
describe it, and run `cargo test` often. Every exercise ends with a **Done when**
line: a test to write or an output to see, so you know when you have finished.

### Getting comfortable

🟢 **1. `Engine::spread()`.** Return `Option<u32>`: best ask minus best bid, or
`None` if either side is empty. Test it on the worked example from chapter 1.

**Done when:** a test replays steps 1 to 5 of the worked example and gets
`Some(1)` (ask 101, bid 100), and gets `None` both on an empty book and after step
6, when no asks are left.

🟢 **2. Order count per level.** `PriceLevel::count` already exists. Add
`Engine::order_count_at(side, tick) -> u32`. *Hint: mirror `depth_at`.*

**Done when:** after steps 1 to 5 of the worked example,
`order_count_at(Side::Sell, 101)` is 2 and `order_count_at(Side::Buy, 99)` is 1,
and a tick of 4096 or more returns 0 instead of panicking.

🟢 **3. Draw the queues.** [`examples/walkthrough.rs`](../examples/walkthrough.rs)
prints the total quantity per price. Extend it to print each level's queue of
orders, like the diagrams in chapter 1 (`[#2:3] -> [#3:4]`). *Hint: walk
`head -> next_idx` until `NIL`; you will need to expose read access to a level's
orders, for example with an iterator method on `Engine`.*

**Done when:** after step 5, `cargo run --example walkthrough` prints the 101
level as `[#2:3] -> [#3:4]`, and `cargo test` still passes.

🟢 **4. Watch truncation happen.** Rest 40 sells of quantity 1 at one price, sweep
them with one buy of 40 into an `EventBatch`, and print `len()` and `dropped()`. Then repeat with a
`Vec<Event>`. *Compare with the unit tests `bounded_batch_sink_reports_truncation`
and `deep_sweep_reports_every_execution`.*

**Done when:** the batch reports `len() == 64` and `dropped() == 18`, and the
`Vec` holds all 82 events (1 acceptance, 40 trades, 40 maker fills, 1 taker fill).

### Understanding the internals

🟡 **5. Shrink `Order`.** `Order` is 48 bytes, and its fields add up to 42, so no
reordering helps: 48 is the next multiple of 8 (see
[3.3](03-order-pool.md#33-the-order-record)). Something has to go. The obvious
candidate is `sequence`, which nothing reads: remove it (and the engine counter
that only feeds it), and update the size test in `pool.rs` to 40. Then try
`#[repr(C, align(64))]` on the original instead, and benchmark both against the
original. *Which wins, and does it depend on the book size?*

**Done when:** the size test pins `size_of::<Order>()` at 40, every other test
still passes, and you have benchmark numbers for all three layouts.

🟡 **6. Fast `next_set`.** Replace the linear `next_lower_active` /
`next_higher_active` in `depth_snapshot` with bitset methods `prev_set(tick)` /
`next_set(tick)`. *Hint: mask off the bits at and below (or above) `tick` in its
word with a shift, check the word; if empty, go up one level and do the same with
that word's bit, until some level has a bit beyond; then come back down as
`lowest()` does, taking the lowest (or highest) bit at each level.*
Unit-test against the old linear version with random bitsets.

**Done when:** a test compares `next_set` / `prev_set` with the linear versions on
thousands of random bitsets, and `cargo test --test model_check` still passes.

🟡 **7. What does the seed cost?** The hash map mixes a secret seed into every
hash (chapter 5). Write a criterion benchmark of `get` on a full-ish map, then a
copy of `FlatHashMap` without the seed, and compare. *Hint: put the copy in the
benchmark file, not in `src/`, so the engine keeps its protection.*

**Done when:** you have both numbers from the same machine, and can say whether
the difference is bigger than the run-to-run noise criterion reports.

### New features

🟡 **8. Fill-or-kill orders.** Add `OrderType::FillOrKill`: the order trades its
whole quantity at once, or nothing at all. "Nothing" must leave the book
untouched, so the engine has to know *before the first trade* whether enough
quantity rests at prices the order accepts. Then decide how a killed order is
reported: rejected before acceptance (like a post-only order), or accepted and
then expired (like an IOC order)? *Hint: walk the opposing levels best first,
adding up `total_qty`, and stop as soon as the sum reaches `qty` or the next price
no longer crosses; `depth_snapshot` shows how to walk the levels. Exercise 6 makes
that walk fast. Remember that the WAL's order-type byte needs a new value.*

**Done when:** tests show a fill-or-kill order that fills completely when enough
quantity is there, and one that is killed with no trades and unchanged depth when
it is not, and `model_check` passes with fill-or-kill orders in the generator.

🟡 **9. A band around the last trade.** The price band is measured from the best
opposing price, so it vanishes when that side is empty, and it moves whenever a
single order changes the best price. Exchanges usually measure it from a
**reference price**, such as the last trade. Add that option. *Hint: the engine
needs one more piece of state, the last trade price, which `match_incoming` knows
whenever it emits a `Trade`. State means it must go into the snapshot, and the
checkpoint format with it; the WAL needs nothing new, because a replay recomputes
the last trade.*

**Done when:** with a band of 5 around the last trade at 100, a buy at 106 is
rejected even when the ask side is empty, and `tests/snapshot.rs` still passes.

🟡 **10. Amend: change the price or raise the quantity.** `reduce_order` keeps
priority because the order gets smaller. Add `amend_order(id, new_price_tick,
new_qty)` for every other change: it keeps the order's place when only the
quantity goes down at the same price, and otherwise acts as cancel + new order,
at the back of the queue, which may even trade if the new price crosses. *Hint:
delegate the first case to `reduce_order`. For the events, keep the rule from 7.2
that a consumer can follow the book from events alone: is `OrderCancelled` then
`OrderAccepted` enough, or does the amend deserve its own event? The WAL needs a
new record kind.*

**Done when:** tests show that lowering the quantity keeps priority, raising it
loses priority, and moving the price across the spread trades; `model_check`
sends amends too.

🔴 **11. Per-account order lists.** `cancel_account` walks the whole book, so
cancelling one trader's two orders can mean scanning thousands of ticks. Give each
account its own intrusive doubly-linked list through the arena, exactly like a
price level's queue, so `cancel_account` touches only that account's orders.
*Hint: each `Order` needs two more `u32` links (watch what that does to its size,
chapter 3), and the engine needs a fixed-size table from account to list head;
the flat hash map from chapter 5 can hold it. Every place an order enters or
leaves the book must update the list: resting, fills, cancels, reductions to
zero, self-trade prevention. Keep the order of the cancellations as it is, or
change the reference book in `model_check` to match.*

**Done when:** `model_check` still passes, and a benchmark shows `cancel_account`
on an account with a handful of orders costing about the same in a book of 100 or
of 100,000 orders.

### Engineering and verification

🟡 **12. Shrinking failures.** `tests/model_check.rs` finds bugs with long random
request streams, but a failure is reported deep inside a 20,000-step run. Port
its generator to [`proptest`](https://docs.rs/proptest), so that a failure
shrinks to the smallest request sequence that reproduces it.

**Done when:** after you break the engine on purpose (the `<=` to `<` change from
[6.11](06-matching-algorithm.md#611-try-it-yourself)), proptest reports a failing
sequence of only a few requests.

🔴 **13. Where do the 45 nanoseconds go?** On a 300,000-tick ladder, placing an
order costs about 45 ns more than on 4096 ticks (10.6). Chapter 10 guesses that
cache misses on the 14 MB of price levels, not the two extra bit-scans, explain
most of it. Test the guess. *Hint: add a benchmark on the wide ladder with prices
clustered in a narrow band, like the default one, and compare; then use a
profiler that counts cache misses (`perf stat -e cache-misses` on Linux, VTune or
the Windows Performance Analyzer elsewhere). For a third point, shrink
`PriceLevel` (do you need `count`?) and see whether the gap narrows.*

**Done when:** you have numbers for clustered and spread prices on both ladders,
and can say in two sentences how much of the gap is cache misses.

🔴 **14. Shard by symbol.** Run four `Engine`s on four threads, each fed by its own
ring buffer, routing requests by a symbol id. Verify that the books are
independent and throughput scales. *Hint: start from
[`examples/pipeline.rs`](../examples/pipeline.rs). Give the gateway one
`Producer` per shard and pick it with `symbol % 4`; each matching thread keeps
its own `Engine` and its own WAL file. For the independence check, replay each
shard's log separately and compare with a single-threaded run per symbol.*

**Done when:** each shard's book equals a single-threaded replay of that shard's
requests, and a benchmark shows throughput with 1, 2 and 4 shards.

🔴 **15. Model-check the ring buffer with [`loom`](https://docs.rs/loom).** Write
loom tests for `push`/`pop` and for `is_disconnected`. Then weaken one `Acquire`
to `Relaxed`, and watch loom find the bug that x86 hides and that even the ARM CI
job may miss. *Hint: loom needs its own versions of the concurrency types. Under
`#[cfg(loom)]`, import `AtomicUsize`, `AtomicBool`, `Arc` and `UnsafeCell` from
`loom` instead of `std`; loom's `UnsafeCell` is used through `with` and
`with_mut` closures instead of `get()`. Wrap each test body in `loom::model(||
...)`, and run with `RUSTFLAGS="--cfg loom" cargo test --release`. Keep the
tests small (two or three pushes): loom tries every interleaving.*

**Done when:** the loom tests pass with the real orderings, and loom reports a
failure once one `Acquire` is weakened to `Relaxed`.

### Making the WAL production-shaped

🟡 **16. A new log at each checkpoint.** After a checkpoint, the log's older
records are dead weight: recovery skips them (9.8). Start a new log file at each
checkpoint, so old files can be archived or deleted. *Hint: name each log after
the first sequence number it holds, for example `wal-00000000000000005001.log`;
when a checkpoint is written with `next_sequence` N, finalize the current log and
`create` `wal-N`. Recovery reads the newest checkpoint and replays the logs from
there on with `replay_from`. Delete a log only once downstream has every event it
produced (9.5).*

**Done when:** a test that writes three checkpoints, crashes (drops everything)
and recovers reaches the live book while reading only the newest checkpoint and
the logs after it.

🔴 **17. Checkpoints without pausing matching.** `Engine::snapshot` runs on the
matching thread and walks the whole book. Take snapshots on another thread
instead, from a **shadow** engine that receives the same requests (the pipeline
and replay tests show that such a replica stays identical). *Hint: give the
matching thread a second ring buffer (chapter 8) to the shadow thread and push
each logged `WalRecord` into it; the shadow applies them with `apply_to` and, now
and then, writes a checkpoint with the sequence it has reached.*

**Done when:** a benchmark shows the matching thread's latency unchanged while
the shadow writes checkpoints, and recovery from the shadow's checkpoint reaches
the primary's book.

🔴 **18. Atomic `create`.** Write the header to `path.tmp`, sync it, rename it to
`path`, then sync the parent directory (on Unix). For each step, explain what a
crash right before it would leave behind, and why the step is needed. *Hint:
`write_checkpoint` in `src/snapshot.rs` follows exactly these steps for
checkpoint files.*

**Done when:** you can explain each step's purpose in one sentence, and a test
shows that a leftover `path.tmp` from an interrupted `create` does not stop the
next `create`.

🔴 **19. No surprise `SIGBUS`.** Make a full disk an `io::Error` from `create` or
`append` instead of a crash later: allocate the file's blocks up front
(`posix_fallocate` through the `libc` crate, or by writing zeros). Measure how
much slower growth becomes.

**Done when:** on a deliberately tiny filesystem (for example a size-limited
`tmpfs` on Linux), filling the disk makes `create` or `append` return an
`io::Error` instead of killing the process.

🔴 **20. Corruption in the middle.** `open_append` discards everything after the
first invalid record. Design a policy that tells a torn tail (normal after a
crash) from damage in the middle (valid records *after* a bad one), and decide
what each should do. *Hint: look at the sequence numbers.*

**Done when:** a test with a damaged record followed by valid ones gets your
policy's result (for example an error) instead of silently losing the valid
records, while a torn tail still recovers as before.

## 11.3 Retrace the fixes

Earlier versions of this repository had real bugs. Each was fixed in a small,
focused commit, and reading them is one of the best lessons the repository
offers: `git show` on a fix displays the bug, the change and the test that proves
it.

### Solved exercises: try them before reading the fix

These started as exercises and have since been done in the code. To try one
yourself, go back to the version before the fixes, solve it there, and then
compare with the commit named:

```sh
git switch --detach a192ebc                          # the code before the first round of fixes
# ... work on the exercise, run cargo test ...
git log --oneline main --grep "caller-supplied sink" # find the fix
git show <hash>                                      # compare with your solution
git switch main                                      # come back (stash or commit your work first)
```

`a192ebc` has the **old API**, so the chapters, which describe the current code,
will not match it exactly; use them for the ideas. R7 to R14 were solved much
later: start those from the commit just before the named one
(`git switch --detach <hash>~1`).

🟢 **R1. An overflow flag.** Make the old fixed-size event batch record whether
any event was dropped. *Solved by `fix(events)!: stream execution reports through
a caller-supplied sink`, as `dropped()` and `is_truncated()`.*

🟡 **R2. Stop returning a batch from every call.** Let the caller supply where
events go, so nothing is copied and nothing is lost. *Solved in the same commit
as R1; measure the difference with the benchmarks (section 10.6 has one run).*

🟡 **R3. Keyed hash.** Give `FlatHashMap` a per-instance random seed. *Solved by
`fix(hashmap): seed the hash per instance to resist crafted order IDs`. Exercise 7
measures what it costs.*

🔴 **R4. Type-safe SPSC.** Split the ring buffer into a `Producer<T>` and a
`Consumer<T>` that are `Send` but not `Clone`, with `&mut self` methods. *Solved
by `fix(ringbuffer)!: enforce SPSC roles with Producer/Consumer handles`; note how
the `compile_fail` doctests test the guarantee.*

🔴 **R5. Differential testing.** Compare the engine with a slow but obviously
correct reference book on long random request streams. *Solved by
`test: model-based engine check and WAL replay determinism` (section 10.2).
Exercise 12 is the next step.*

🔴 **R6. Harden the WAL.** A header with a magic number and version, a checksum
per record, portable little-endian fields, and an open-for-append constructor.
*Solved by `fix(wal)!: sound, portable, crash-safe log format`. The remaining
limits became R11 and R12 and exercises 18 to 20.*

🟡 **R7. Immediate-or-cancel orders.** An IOC order matches what it can and never
rests. What should the event stream be for the unfilled remainder:
`OrderCancelled`, or `OrderRejected` with a new `RejectReason`? Justify your
choice. *Solved by `feat(engine): add immediate-or-cancel orders`, which chose
neither: its commit message explains why a new `OrderExpired` event fits best.*

🟡 **R8. Market orders.** A buy at tick 4095 crosses every ask, but rests its
remainder there, which is not what a market order means. Build real market orders
on top of IOC. *Solved by `feat(engine): add market orders`; R13 adds the
price protection real exchanges use.*

🟡 **R9. Reduce quantity in place.** Let a trader lower an order's quantity without
losing time priority; reducing it to nothing should behave like a cancel. *Solved
by `feat(engine): reduce a resting order without losing priority`. Note how many
places a new request kind touches: the event, the reject reason, the WAL record
and the test generator. Exercise 10 goes further.*

🔴 **R10. Self-trade prevention.** Give every order an account. When a taker
would trade with its own account's resting order, apply a policy (cancel the
resting order, cancel the incoming one, or both) and emit matching events.
*Solved by `feat(engine): give every order an account` and
`feat(engine): add self-trade prevention`. Look at where the check sits in the
inner loop of `match_incoming`, and how the cancelled maker leaves the book by
exactly the steps of a full fill. Mass cancels, which also needed accounts, came
next in `feat(engine): cancel every order of an account, or the whole book`;
exercise 11 makes them faster.*

🟡 **R11. Record the capacity.** Replay is only deterministic with the same
`Engine` capacity (9.1). Store it in the log header, and make replay refuse a log
written for a different capacity, while keeping older logs readable. *Solved by
`feat(wal)!: record the engine capacity in the log (format version 4)`;
`feat(wal): replay a log into a new engine` then builds the replica from it, so
recovery can no longer pick the wrong capacity.*

🟡 **R12. One writer per file.** Take an exclusive lock with `File::try_lock`, and
test that a second writer gets a clear error instead of corrupting the log.
*Solved by `feat(wal): refuse a second writer with a lock file`. It locks a
companion file, not the log: its commit message explains what Windows would do
otherwise. Checkpoints, the other half of the recovery story, came right after,
in `feat(engine): snapshot and restore the book` and the two commits that follow
it; exercises 16 and 17 build on them.*

🟡 **R13. Price protection for market orders.** A market order sweeps until it is
filled, however far that takes it. Add a price band: a market order's limit
becomes the best opposing price at arrival, plus (for a buy) or minus (for a sell)
`band` ticks. *Solved by `feat(engine): add pre-trade risk limits`, which made the
band a risk limit for every order and gave market orders its edge as their limit;
exercise 9 measures the band from the last trade instead.*

🔴 **R14. Beyond 4096 ticks.** Generalise the two-level bitset to more levels, so
an engine can have more than 4096 ticks. *Solved by `refactor(bitset): size the
price ladder at run time`, which chose any number of levels at run time instead of
a fixed three, and `feat(engine)!: choose the number of price ticks per engine`.
Compare the commit's benchmark notes with chapter 10.6, and see exercise 13.*

### Every fix, with its commit

Find a commit by its subject, then show it:

```sh
git log --oneline --grep "detect double frees"   # then: git show <hash>
```

| Where | Problem | Fixed by (commit subject) | Chapter |
|---|---|---|---|
| `wal.rs` | `finalize` called `set_len` while the file was still mapped, which Windows refuses (OS error 1224). `Drop` swallowed the error, so the log kept its zero padding and the reader returned extra all-zero records. | `fix(wal)!: sound, portable, crash-safe log format` (the map is now dropped before truncating) | [9.4](09-write-ahead-log.md#94-the-writers-lifecycle) |
| `events.rs` | Every call returned a fixed 64-event batch, so a taker that swept 32 or more makers **silently lost** reports. | `fix(events)!: stream execution reports through a caller-supplied sink` | [7.3](07-events.md#73-the-eventsink-trait) |
| `ringbuffer.rs` | The SPSC contract was unchecked: `push` and `pop` took `&self`, so two producers compiled fine and raced (undefined behaviour). `len()` from a third thread could wrap. | `fix(ringbuffer)!: enforce SPSC roles with Producer/Consumer handles` | [8.5](08-ring-buffer.md#85-safety-what-the-type-system-enforces) |
| `hashmap.rs` | The hash had no secret and SplitMix64 is invertible, so a client could craft colliding order ids and force long probe chains. | `fix(hashmap): seed the hash per instance to resist crafted order IDs` | [5.7](05-hash-map.md#57-complexity) |
| `wal.rs` | Records were raw `repr(C)` bytes: native endianness, 4 bytes of uninitialised padding, no header, version or checksum, so a torn record looked valid. | `fix(wal)!: sound, portable, crash-safe log format` | [9.2](09-write-ahead-log.md#92-the-record-format) |
| `wal.rs` | `create` truncated an existing file and there was no way to continue a log, so a naive restart erased the log it should replay. | `fix(wal)!: sound, portable, crash-safe log format` (adds `open_append`) and `feat(wal)!: refuse to overwrite an existing log in create` | [9.5](09-write-ahead-log.md#95-reading-and-replaying) |
| `engine.rs` | `depth_at` and `depth_snapshot` could panic on unusual arguments. | `fix(engine): make depth queries panic-free` | [6.1](06-matching-algorithm.md#61-the-public-api) |
| `wal.rs` | Merge `57c7c34` combined two rewrites of the file and broke the build. | `fix(wal): restore the PR #2 writer lost in merge 57c7c34` | |
| `engine.rs` | The last maker's `OrderFilled` was reported while its drained price was still in the bitset, so a caught sink panic left a stale best price. | `fix(engine): update the price index before reporting a maker fill` | [6.3](06-matching-algorithm.md#63-match_incoming-the-heart-of-the-engine) |
| `hashmap.rs` | `get`, `insert` and `remove` looped forever on a full table, and the size computation could wrap in release builds. | `fix(hashmap): panic instead of hanging when the table would fill`, `fix(hashmap): check the table size computation for overflow` | [5.5](05-hash-map.md#55-sizing-why-the-table-is-never-full) |
| `pool.rs` | A double free was caught only by `debug_assert!`, and `count -= 1` wrapped silently in release builds. | `fix(pool): detect double frees in release builds`, `fix(pool): check the level order count for underflow` | [3.7](03-order-pool.md#37-defensive-checks) |
| `bitset.rs` | The `TICKS` limit was only a comment. | `fix(bitset): enforce the TICKS limits at compile time` | [4.6](04-price-ladder-bitset.md#46-why-4096-ticks-and-how-to-get-more) |
| `wal.rs` | Readers mapped the log a second time, which could race a live writer. | `fix(wal): read logs with plain file I/O` | [9.3](09-write-ahead-log.md#93-memory-mapped-files-in-one-minute) |
| `ringbuffer.rs` | A consumer could not tell a finished producer from a slow one and spun forever. | `feat(ringbuffer): detect a dropped producer or consumer` | [8.5](08-ring-buffer.md#telling-nothing-yet-from-nothing-ever-again) |
| docs | The README and chapters described an old API, and nothing noticed. | `docs(readme): fix the examples for the current API and compile them` (README doctest), `docs(ch08): move the complete program into examples/pipeline.rs` (`tests/docs_sync.rs`) | [10.7](10-testing-and-performance.md#107-continuous-integration) |

## 11.4 Further reading

- **How to Build a Fast Limit Order Book** (WK Selph): the classic write-up of
  the data-structure choices behind a fast book.
- **The LMAX Disruptor** technical paper (Thompson et al.) and Martin Thompson's
  *Mechanical Sympathy* blog: ring buffers, the single-writer principle, false
  sharing, cache effects.
- **Rust Atomics and Locks** (Mara Bos): memory ordering explained properly, the
  background for `ringbuffer.rs`.
- **The Art of Computer Programming, Vol. 3, section 6.4** (Knuth): hashing,
  including the linear-probing analysis quoted in chapter 5.
- **"Are You Sure You Want to Use MMAP in Your Database Management System?"**
  (Crotty, Leis and Pavlo, CIDR 2022): why memory-mapped I/O makes error handling
  and durability hard, with the problems the WAL caveats in 9.6 describe.
- **"Can Applications Recover from fsync Failures?"** (Rebello et al., USENIX ATC
  2020): what really happens after a failed flush, and why retrying is not enough.
- **"How to Build an Exchange"** (Brian Nigito, Jane Street tech talk): how real
  exchanges structure the engine, sequencer and log.

**Back to:** [README](../README.md)
