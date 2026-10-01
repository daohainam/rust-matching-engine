# 10. Known Limitations, Sharp Edges and Exercises

This is a teaching project, and it is more useful to learn from its rough edges
than to pretend it has none. Everything below was found by reading the code and
(where noted) confirmed by running it.

## 10.1 What has been fixed, and how

Earlier versions had the problems below. Each fix is a small, focused commit, and
reading them is one of the best lessons this repository offers: `git show` on a
fix displays the bug, the change and the test that proves it. Find a commit by its
subject:

```sh
git log --oneline --grep "detect double frees"   # then: git show <hash>
```

| Where | Problem | Fixed by (commit subject) | Chapter |
|---|---|---|---|
| `wal.rs` | `finalize` called `set_len` while the file was still mapped, which Windows refuses (OS error 1224). `Drop` swallowed the error, so the log kept its zero padding and the reader returned extra all-zero records. | `fix(wal)!: sound, portable, crash-safe log format` (the map is now dropped before truncating) | [8.9](08-ringbuffer-and-wal.md#89-the-writers-lifecycle) |
| `events.rs` | Every call returned a fixed 64-event batch, so a taker that swept 32 or more makers **silently lost** reports. | `fix(events)!: stream execution reports through a caller-supplied sink` | [7.3](07-events.md#73-the-eventsink-trait) |
| `ringbuffer.rs` | The SPSC contract was unchecked: `push` and `pop` took `&self`, so two producers compiled fine and raced (undefined behaviour). `len()` from a third thread could wrap. | `fix(ringbuffer)!: enforce SPSC roles with Producer/Consumer handles` | [8.5](08-ringbuffer-and-wal.md#85-safety-what-the-type-system-enforces) |
| `hashmap.rs` | The hash had no secret and SplitMix64 is invertible, so a client could craft colliding order ids and force long probe chains. | `fix(hashmap): seed the hash per instance to resist crafted order IDs` | [5.7](05-hash-map.md#57-complexity) |
| `wal.rs` | Records were raw `repr(C)` bytes: native endianness, 4 bytes of uninitialised padding, no header, version or checksum, so a torn record looked valid. | `fix(wal)!: sound, portable, crash-safe log format` | [8.7](08-ringbuffer-and-wal.md#87-the-record-format) |
| `wal.rs` | `create` truncated an existing file and there was no way to continue a log, so a naive restart erased the log it should replay. | `fix(wal)!: sound, portable, crash-safe log format` (adds `open_append`) and `feat(wal)!: refuse to overwrite an existing log in create` | [8.10](08-ringbuffer-and-wal.md#810-reading-and-replaying) |
| `engine.rs` | `depth_at` and `depth_snapshot` could panic on unusual arguments. | `fix(engine): make depth queries panic-free` | [6.1](06-matching-algorithm.md#61-the-public-api) |
| `wal.rs` | Merge `57c7c34` combined two rewrites of the file and broke the build. | `fix(wal): restore the PR #2 writer lost in merge 57c7c34` | |
| `engine.rs` | The last maker's `OrderFilled` was reported while its drained price was still in the bitset, so a caught sink panic left a stale best price. | `fix(engine): update the price index before reporting a maker fill` | [6.3](06-matching-algorithm.md#63-match_incoming-the-heart-of-the-engine) |
| `hashmap.rs` | `get`, `insert` and `remove` looped forever on a full table, and the size computation could wrap in release builds. | `fix(hashmap): panic instead of hanging when the table would fill`, `fix(hashmap): check the table size computation for overflow` | [5.5](05-hash-map.md#55-sizing-why-the-table-is-never-full) |
| `pool.rs` | A double free was caught only by `debug_assert!`, and `count -= 1` wrapped silently in release builds. | `fix(pool): detect double frees in release builds`, `fix(pool): check the level order count for underflow` | [3.7](03-order-pool.md#37-defensive-checks) |
| `bitset.rs` | The `TICKS` limit was only a comment. | `fix(bitset): enforce the TICKS limits at compile time` | [4.6](04-price-ladder-bitset.md#46-why-4096-ticks) |
| `wal.rs` | Readers mapped the log a second time, which could race a live writer. | `fix(wal): read logs with plain file I/O` | [8.8](08-ringbuffer-and-wal.md#88-memory-mapped-files-in-one-minute) |
| `ringbuffer.rs` | A consumer could not tell a finished producer from a slow one and spun forever. | `feat(ringbuffer): detect a dropped producer or consumer` | [8.5](08-ringbuffer-and-wal.md#telling-nothing-yet-from-nothing-ever-again) |
| docs | The README and chapters described an old API, and nothing noticed. | `docs(readme): fix the examples for the current API and compile them` (README doctest), `docs(ch08): move the complete program into examples/pipeline.rs` (`tests/docs_sync.rs`) | [9.7](09-testing-and-performance.md#97-continuous-integration) |

To see the code *before* most of these fixes, check out `a192ebc`, the commit that
the first round of fixes started from.

## 10.2 Known limitations and sharp edges

### Sharp edges that remain

| Where | Issue | How bad | Chapter |
|---|---|---|---|
| `wal.rs` | The durability limits: a failed flush is final, a new file's directory entry is not synced on Unix, a full disk becomes `SIGBUS` on Linux, `create` is not atomic, damage in the middle of a log is treated like a torn tail, and one writer per file is not enforced. | Medium for a real deployment; fine for learning, as long as you know them. | [8.11](08-ringbuffer-and-wal.md#811-caveats-read-before-relying-on-the-wal) |
| `wal.rs` + `engine.rs` | Replay is deterministic only with the **same `Engine` capacity**, and the log does not record it. | Medium: get it wrong and the replica silently differs. | [8.6](08-ringbuffer-and-wal.md#86-why-log-at-all) |
| `events.rs` | An `EventBatch` keeps only 64 events per call. It counts the rest, so this is no longer silent, but you must check `is_truncated()`. | Low: check it, or use an unbounded sink. | [7.4](07-events.md#74-what-happens-when-there-are-too-many-events) |
| `engine.rs` | A sink that panics (with the panic caught) leaves a consistent book but a **partly applied** request. | Low: rebuild from the WAL; a release build aborts anyway. | [7.5](07-events.md#75-when-a-sink-panics) |
| `ringbuffer.rs` | Its memory ordering is checked on ARM hardware and by Miri, but only for the interleavings that happen to occur. | Low | [9.8](09-testing-and-performance.md#98-what-the-tests-do-not-cover) |
| `hashmap.rs` | The seed protects against crafted ids only while it stays secret; `with_capacity_and_seed` with a guessable seed gives that up. | Low: the engine always uses a random seed. | [5.7](05-hash-map.md#57-complexity) |

### Design limits (by choice, not bugs)

| Limit | Detail |
|---|---|
| **4096 price ticks** | Fixed by the two-level bitset (`64 × 64`), checked at compile time. No tick-size or price offset logic; callers convert. |
| **Fixed order capacity** | Chosen in `Engine::new`. A full arena rejects orders that would need to rest (orders that fully match still succeed). |
| **Limit orders only** | No market, IOC, FOK, stop, iceberg; no modify (cancel + new loses priority); no self-trade prevention; no accounts. |
| **One instrument per `Engine`** | Run several engines for several symbols. |
| **Engine is not wired to the WAL or ring buffer** | The caller does it; [`examples/pipeline.rs`](../examples/pipeline.rs) shows how. The WAL's `sequence` is caller-supplied and independent of the engine's own counter. |
| **WAL format is small** | Version 1 has a header, explicit little-endian fields and per-record checksums, but no fsync policy of its own, no recorded engine capacity and no file locking (8.11). |
| **Failing loudly** | `assert!`/`expect` on invariants panic on a bookkeeping bug, in every build profile. With `panic = "abort"` (set in this repo's release profile, and needed in *your* binary's profile because Cargo ignores a dependency's) that aborts the process. Intentional for a financial component. |
| **`depth_snapshot` is slow-ish** | It allocates a `Vec` and scans ticks linearly (documented in the source). Reporting only. |

### Performance notes (not defects)

- `Order` is **48 bytes**: neither cache-line sized nor a divisor of 64, so many
  records straddle two lines ([chapter 3](03-order-pool.md#33-the-order-record)).
- `Order.sequence` is stored but never read. `Order.in_use` is read only by the
  double-free check in `OrderPool::free`. Both cost bytes, but help debugging and
  auditing.

## 10.3 Exercises

Difficulty: 🟢 warm-up · 🟡 a solid afternoon · 🔴 a real project.
Hints are deliberately short; the chapters contain the ingredients. Exercises
marked **✅ solved** have been done in the code since they were written. Try them
on `a192ebc` first (`git switch --detach a192ebc`), then compare your solution
with the commit named.

### Getting comfortable

🟢 **1. `Engine::spread()`.** Return `Option<u32>`: best ask minus best bid, or
`None` if either side is empty. Test it on the worked example from chapter 1.

🟢 **2. Order count per level.** `PriceLevel::count` already exists. Add
`Engine::order_count_at(side, tick) -> u32`. *Hint: mirror `depth_at`.*

🟢 **3. Draw the book.** Write a function that prints the ladder like the ASCII
diagrams in chapter 1 using `depth_snapshot`. Bonus: show each level's queue by
walking `head -> next_idx`. (You will need to expose read access to the pool.)

🟢 **4. Watch truncation happen.** Rest 40 sells at one price, sweep them with one
buy into an `EventBatch`, and print `len()` and `dropped()`. Then repeat with a
`Vec<Event>`. *Compare with the unit tests `bounded_batch_sink_reports_truncation`
and `deep_sweep_reports_every_execution`.*

### Understanding the internals

🟡 **5. Shrink `Order`.** Reorder the fields (group the `u32`s, then the `u8`/`bool`s
at the end) so `size_of::<Order>()` becomes 40. Add a test that pins the size.
Then try `#[repr(C, align(64))]` instead and benchmark both against the original.
*Which wins, and does it depend on the book size?*

🟡 **6. Fast `next_set`.** Replace the linear `next_lower_active` /
`next_higher_active` in `depth_snapshot` with bitset methods `prev_set(tick)` /
`next_set(tick)`. *Hint: mask off the bits at and below (or above) `tick` in its
word with a shift, check the word; if empty, use the summary word the same way.*
Unit-test against the old linear version with random bitsets.

🟡 **7. ✅ solved: stop returning a batch from every call.** Let the caller supply
where events go, so nothing is copied and nothing is lost. *Solved by
`fix(events)!: stream execution reports through a caller-supplied sink`; measure
the difference with the benchmarks (chapter 9.6 has one run).*

🟡 **8. ✅ solved: an overflow flag.** Make `EventBatch` record whether any event
was dropped. *Solved in the same commit as exercise 7: `dropped()` and
`is_truncated()`.*

### New features

🟡 **9. IOC orders (immediate-or-cancel).** Add a time-in-force parameter. An IOC
order matches what it can and never rests. What should the event stream be for
the unfilled remainder: `OrderCancelled` or `OrderRejected`? Justify your choice.
(Remember the rule from 6.9: the book must be consistent at every `emit`.)

🟡 **10. Market orders.** A buy with `price_tick = 4095` crosses every ask, but its
remainder would *rest* at 4095, which is not what a market order means. Build real
market orders on top of IOC (exercise 9).

🟡 **11. Reduce-quantity in place.** Let a trader *lower* an order's quantity
without losing time priority (raising it must still lose priority, as a
cancel + new would). *Hint: adjust `remaining_qty` and `level.total_qty`
together, as `reduce_qty` does. Reducing to zero should behave like a cancel.*

🔴 **12. Self-trade prevention.** Add an `account: u32` to `Order`. When a taker
would trade with its own account's resting order, apply a policy (cancel oldest,
cancel newest, or decrement both) and emit matching events.

### Engineering and verification

🔴 **13. ✅ solved: differential testing.** Compare the engine with a slow but
obviously correct reference book on long random request streams. *Solved by
`test: model-based engine check and WAL replay determinism` (chapter 9.2). Next
step: port the generator to [`proptest`](https://docs.rs/proptest), so that a
failure shrinks to the smallest request sequence that reproduces it.*

🔴 **14. Three-level bitset.** Generalise `TwoLevelBitset` to `N` levels of `u64`
(3 levels = 262,144 ticks). Keep `lowest()` and `highest()` loop-free for a fixed
depth. Make `MAX_TICKS` follow, and update the compile-time check on `TICKS`.

🔴 **15. ✅ solved: type-safe SPSC.** Split the ring buffer into a `Producer<T>` and
a `Consumer<T>` that are `Send` but not `Clone`, with `&mut self` methods. *Solved
by `fix(ringbuffer)!: enforce SPSC roles with Producer/Consumer handles`; note how
the `compile_fail` doctests test the guarantee.*

🔴 **16. ✅ solved: harden the WAL.** A header with a magic number and version,
a checksum per record, portable little-endian fields, and an open-for-append
constructor. *Solved by `fix(wal)!: sound, portable, crash-safe log format`. The
remaining limits are exercises 19 to 23.*

🔴 **17. ✅ solved: keyed hash.** Give `FlatHashMap` a per-instance random seed.
*Solved by `fix(hashmap): seed the hash per instance to resist crafted order IDs`.
Still worth doing: measure what the seed costs per lookup.*

🔴 **18. Shard by symbol.** Run four `Engine`s on four threads, each fed by its own
ring buffer, routing requests by a symbol id. Verify that the books are
independent and throughput scales.

### Making the WAL production-shaped

🟡 **19. Record the capacity.** Replay is only deterministic with the same `Engine`
capacity (8.6). Store it in the header's reserved bytes (format version 2), and
make replay refuse a log written for a different capacity. Keep version 1 logs
readable.

🟡 **20. One writer per file.** Take an exclusive lock with `File::try_lock` in
`create` and `open_append` (stable since Rust 1.89, so raise `rust-version`), and
test that a second writer gets a clear error instead of corrupting the log.

🔴 **21. Atomic `create`.** Write the header to `path.tmp`, sync it, rename it to
`path`, then sync the parent directory (on Unix). For each step, explain what a
crash right before it would leave behind, and why the step is needed.

🔴 **22. No surprise `SIGBUS`.** Make a full disk an `io::Error` from `create` or
`append` instead of a crash later: allocate the file's blocks up front
(`posix_fallocate` through the `libc` crate, or by writing zeros). Measure how
much slower growth becomes.

🔴 **23. Corruption in the middle.** `open_append` discards everything after the
first invalid record. Design a policy that tells a torn tail (normal after a
crash) from damage in the middle (valid records *after* a bad one), and decide
what each should do. *Hint: look at the sequence numbers.*

🔴 **24. Model-check the ring buffer with [`loom`](https://docs.rs/loom).** Write
loom tests for `push`/`pop` and for `is_disconnected`. Then weaken one `Acquire`
to `Relaxed`, and watch loom find the bug that x86 hides and that even the ARM CI
job may miss.

## 10.4 Further reading

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
  and durability hard, with the problems the WAL caveats in 8.11 describe.
- **"Can Applications Recover from fsync Failures?"** (Rebello et al., USENIX ATC
  2020): what really happens after a failed flush, and why retrying is not enough.
- **"How to Build an Exchange"** (Brian Nigito, Jane Street tech talk): how real
  exchanges structure the engine, sequencer and log.

**Back to:** [README](../README.md)
