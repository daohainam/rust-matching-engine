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
| `wal.rs` | The durability limits: a failed flush is final, a new file's directory entry is not synced on Unix, a full disk becomes `SIGBUS` on Linux, `create` is not atomic, damage in the middle of a log is treated like a torn tail, and one writer per file is not enforced. | Medium for a real deployment; fine for learning, as long as you know them. | [9.6](09-write-ahead-log.md#96-caveats-read-before-relying-on-the-wal) |
| `wal.rs` + `engine.rs` | Replay is deterministic only with the **same `Engine` capacity**, and the log does not record it. | Medium: get it wrong and the replica silently differs. | [9.1](09-write-ahead-log.md#91-why-log-at-all) |
| `events.rs` | An `EventBatch` keeps only 64 events per call. It counts the rest, so this is no longer silent, but you must check `is_truncated()`. | Low: check it, or use an unbounded sink. | [7.4](07-events.md#74-what-happens-when-there-are-too-many-events) |
| `engine.rs` | A sink that panics (with the panic caught) leaves a consistent book but a **partly applied** request. | Low: rebuild from the WAL; a binary built with `panic = "abort"` stops instead (your own profile decides, [10.5](10-testing-and-performance.md#105-build-profiles)). | [7.5](07-events.md#75-when-a-sink-panics) |
| `ringbuffer.rs` | Its memory ordering is checked on ARM hardware and by Miri, but only for the interleavings that happen to occur. | Low | [10.8](10-testing-and-performance.md#108-what-the-tests-do-not-cover) |
| `hashmap.rs` | The seed protects against crafted ids only while it stays secret; `with_capacity_and_seed` with a guessable seed gives that up. | Low: the engine always uses a random seed. | [5.7](05-hash-map.md#57-complexity) |

### Design limits (by choice, not bugs)

| Limit | Detail |
|---|---|
| **4096 price ticks** | Fixed by the two-level bitset (`64 × 64`), checked at compile time. No tick-size or price offset logic; callers convert. |
| **Fixed order capacity** | Chosen in `Engine::new`. A full arena rejects orders that would need to rest (orders that fully match still succeed). |
| **Limit orders only** | No market, IOC, FOK, stop, iceberg; no modify (cancel + new loses priority); no self-trade prevention; no accounts. |
| **One instrument per `Engine`** | Run several engines for several symbols. |
| **Engine is not wired to the WAL or ring buffer** | The caller does it; [`examples/pipeline.rs`](../examples/pipeline.rs) shows how. The WAL's `sequence` is caller-supplied and independent of the engine's own counter. |
| **WAL format is small** | Version 1 has a header, explicit little-endian fields and per-record checksums, but no fsync policy of its own, no recorded engine capacity and no file locking ([9.6](09-write-ahead-log.md#96-caveats-read-before-relying-on-the-wal)). |
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
describe it, and run `cargo test` often.

### Getting comfortable

🟢 **1. `Engine::spread()`.** Return `Option<u32>`: best ask minus best bid, or
`None` if either side is empty. Test it on the worked example from chapter 1.

🟢 **2. Order count per level.** `PriceLevel::count` already exists. Add
`Engine::order_count_at(side, tick) -> u32`. *Hint: mirror `depth_at`.*

🟢 **3. Draw the queues.** [`examples/walkthrough.rs`](../examples/walkthrough.rs)
prints the total quantity per price. Extend it to print each level's queue of
orders, like the diagrams in chapter 1 (`[#2:3] -> [#3:4]`). *Hint: walk
`head -> next_idx` until `NIL`; you will need to expose read access to a level's
orders, for example with an iterator method on `Engine`.*

🟢 **4. Watch truncation happen.** Rest 40 sells at one price, sweep them with one
buy into an `EventBatch`, and print `len()` and `dropped()`. Then repeat with a
`Vec<Event>`. *Compare with the unit tests `bounded_batch_sink_reports_truncation`
and `deep_sweep_reports_every_execution`.*

### Understanding the internals

🟡 **5. Shrink `Order`.** Keeping `#[repr(C)]`, reorder the fields (group the
`u32`s, then the `u8`/`bool`s at the end) so `size_of::<Order>()` becomes 40. Add a
test that pins the size. Then remove `repr(C)` instead and check that the compiler
reaches 40 bytes by itself (see [3.3](03-order-pool.md#33-the-order-record)).
Then try `#[repr(C, align(64))]` instead and benchmark both against the original.
*Which wins, and does it depend on the book size?*

🟡 **6. Fast `next_set`.** Replace the linear `next_lower_active` /
`next_higher_active` in `depth_snapshot` with bitset methods `prev_set(tick)` /
`next_set(tick)`. *Hint: mask off the bits at and below (or above) `tick` in its
word with a shift, check the word; if empty, use the summary word the same way.*
Unit-test against the old linear version with random bitsets.

🟡 **7. What does the seed cost?** The hash map mixes a secret seed into every
hash (chapter 5). Write a criterion benchmark of `get` on a full-ish map, then a
copy of `FlatHashMap` without the seed, and compare. *Hint: put the copy in the
benchmark file, not in `src/`, so the engine keeps its protection.*

### New features

🟡 **8. IOC orders (immediate-or-cancel).** Add a time-in-force parameter. An IOC
order matches what it can and never rests. What should the event stream be for
the unfilled remainder: `OrderCancelled`, or `OrderRejected` with a new
`RejectReason`? Justify your choice.
(Remember the rule from 6.9: the book must be consistent at every `emit`.)

🟡 **9. Market orders.** A buy with `price_tick = 4095` crosses every ask, but its
remainder would *rest* at 4095, which is not what a market order means. Build real
market orders on top of IOC (exercise 8).

🟡 **10. Reduce-quantity in place.** Let a trader *lower* an order's quantity
without losing time priority (raising it must still lose priority, as a
cancel + new would). *Hint: adjust `remaining_qty` and `level.total_qty`
together, as `reduce_qty` does. Reducing to zero should behave like a cancel.*

🔴 **11. Self-trade prevention.** Add an `account: u32` to `Order`. When a taker
would trade with its own account's resting order, apply a policy (cancel oldest,
cancel newest, or decrement both) and emit matching events.

### Engineering and verification

🟡 **12. Shrinking failures.** `tests/model_check.rs` finds bugs with long random
request streams, but a failure is reported deep inside a 20,000-step run. Port
its generator to [`proptest`](https://docs.rs/proptest), so that a failure
shrinks to the smallest request sequence that reproduces it.

🔴 **13. Three-level bitset.** Generalise `TwoLevelBitset` to `N` levels of `u64`
(3 levels = 262,144 ticks). Keep `lowest()` and `highest()` loop-free for a fixed
depth. Make `MAX_TICKS` follow, and update the compile-time check on `TICKS`.

🔴 **14. Shard by symbol.** Run four `Engine`s on four threads, each fed by its own
ring buffer, routing requests by a symbol id. Verify that the books are
independent and throughput scales.

🔴 **15. Model-check the ring buffer with [`loom`](https://docs.rs/loom).** Write
loom tests for `push`/`pop` and for `is_disconnected`. Then weaken one `Acquire`
to `Relaxed`, and watch loom find the bug that x86 hides and that even the ARM CI
job may miss.

### Making the WAL production-shaped

🟡 **16. Record the capacity.** Replay is only deterministic with the same `Engine`
capacity (9.1). Store it in the header's reserved bytes (format version 2), and
make replay refuse a log written for a different capacity. Keep version 1 logs
readable.

🟡 **17. One writer per file.** Take an exclusive lock with `File::try_lock` in
`create` and `open_append` (stable since Rust 1.89, so raise `rust-version`), and
test that a second writer gets a clear error instead of corrupting the log.

🔴 **18. Atomic `create`.** Write the header to `path.tmp`, sync it, rename it to
`path`, then sync the parent directory (on Unix). For each step, explain what a
crash right before it would leave behind, and why the step is needed.

🔴 **19. No surprise `SIGBUS`.** Make a full disk an `io::Error` from `create` or
`append` instead of a crash later: allocate the file's blocks up front
(`posix_fallocate` through the `libc` crate, or by writing zeros). Measure how
much slower growth becomes.

🔴 **20. Corruption in the middle.** `open_append` discards everything after the
first invalid record. Design a policy that tells a torn tail (normal after a
crash) from damage in the middle (valid records *after* a bad one), and decide
what each should do. *Hint: look at the sequence numbers.*

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
will not match it exactly; use them for the ideas.

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
limits are exercises 16 to 20.*

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
| `bitset.rs` | The `TICKS` limit was only a comment. | `fix(bitset): enforce the TICKS limits at compile time` | [4.6](04-price-ladder-bitset.md#46-why-4096-ticks) |
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
