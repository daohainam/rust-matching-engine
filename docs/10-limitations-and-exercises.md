# 10. Known Limitations, Sharp Edges and Exercises

This is a teaching project, and it is more useful to learn from its rough edges
than to pretend it has none. Everything below was found by reading the code and
(where noted) confirmed by running it.

## 10.1 Known limitations and sharp edges

### Bugs and correctness risks

| # | Where | Issue | How bad | Chapter |
|---|---|---|---|---|
| 1 | `wal.rs` | **Fixed.** `WalWriter::finalize` used to call `set_len` while the file was still memory-mapped, which Windows refuses (OS error 1224); `Drop` swallowed the error, so the file kept its zero padding and `read_all` returned extra all-zero records (`wal::tests::append_and_replay_roundtrip` failed with `left: 16, right: 10`). `finalize` now flushes, unmaps and only then truncates. The number is kept so other references stay valid. | None now. After a crash (when `finalize` never ran) the file still ends in zero padding, so replay should keep stopping at `kind == 0`. | [8](08-ringbuffer-and-wal.md#811-caveats-read-before-relying-on-the-wal) |
| 2 | `events.rs` | **Event batches silently truncate at 64 events**, i.e. when one taker consumes 32 or more makers. The book is right; the event stream is incomplete, with no flag saying so. Confirmed. | Medium: a consumer reconciling trades from events would lose fills. | [7](07-events.md#74-what-happens-when-there-are-too-many-events) |
| 3 | `ringbuffer.rs` | **The SPSC contract is unchecked.** `push` and `pop` both take `&self`; two producers cause a data race (undefined behaviour). | Medium: misuse compiles fine. | [8](08-ringbuffer-and-wal.md#85-safety-and-what-the-type-system-does-not-enforce) |
| 4 | `ringbuffer.rs` | `len()` loads `tail` then `head`. Called from a third (monitoring) thread while both cursors move, it can compute `tail - head` with a `head` newer than `tail` and wrap to a huge number. (Analysis only; not reproduced.) | Low: metrics only. | [8](08-ringbuffer-and-wal.md#85-safety-and-what-the-type-system-does-not-enforce) |
| 5 | `hashmap.rs` | The hash is **unkeyed** (no secret seed) and SplitMix64's finalizer is invertible, so a client who chooses order ids can craft colliding ids and force long probe chains. | Low-Medium: a latency-attack vector if ids are untrusted. | [5](05-hash-map.md#57-complexity) |
| 6 | `hashmap.rs` | `get` (for a missing key), `insert` (of a new key) and `remove` (of an absent key) all loop forever if the table has **no empty slot**. `Engine` guarantees this cannot happen (load ≤ 50%); a direct user of `FlatHashMap` that over-fills it would hang. | Low: internal invariant. | [5](05-hash-map.md#53-open-addressing-with-linear-probing) |
| 7 | `pool.rs` | Double `free` is caught only by `debug_assert!`. In release it would corrupt the free stack. | Low: `Engine` never does it. | [3](03-order-pool.md#37-defensive-checks) |
| 8 | `wal.rs` | **`WalRecord` has 4 bytes of implicit padding** (bytes 20 to 23, after `price_tick`) that are never initialised, yet `to_bytes` copies the whole struct with `transmute_copy`. Reading uninitialised padding is technically undefined behaviour, the "sound" comment there is wrong, and the log bytes are not deterministic. (Analysis only; not run under Miri.) | Low in practice, but it breaks byte-for-byte comparison and any checksum over a record. | [8](08-ringbuffer-and-wal.md#811-caveats-read-before-relying-on-the-wal) |
| 9 | `wal.rs` | **`WalWriter::create` truncates an existing file** and there is no open-for-append API, so a naive restart after a crash overwrites the very log you wanted to replay. Read it with `read_all` *before* calling `create`. Also, `finalize` closes the writer for good: `append` and `flush` after it return an `io::Error`, and there is no way to reopen the log for appending. | Medium: easy to lose data by accident. | [8](08-ringbuffer-and-wal.md#811-caveats-read-before-relying-on-the-wal) |

### Design limits (by choice, not bugs)

| Limit | Detail |
|---|---|
| **4096 price ticks** | Fixed by the two-level bitset (`64 × 64`). No tick-size or price offset logic; callers convert. |
| **Fixed order capacity** | Chosen in `Engine::new`. A full arena rejects orders that would need to rest (orders that fully match still succeed). |
| **Fixed 64 events per call** | See bug #2. |
| **Limit orders only** | No market, IOC, FOK, stop, iceberg; no modify (cancel + new loses priority); no self-trade prevention; no accounts. |
| **One instrument per `Engine`** | Run several engines for several symbols. |
| **Engine is not wired to the WAL or ring buffer** | The caller must do it (chapter 8, Part C). The WAL's `sequence` is caller-supplied and independent of the engine's own counter. |
| **WAL format is minimal** | Native endianness, no header/version, no checksum, no fsync policy. |
| **Failing loudly** | `assert!`/`expect` on invariants panic on a bookkeeping bug. With `panic = "abort"` (set in this repo's release profile, and needed in *your* binary's profile because Cargo ignores a dependency's) that aborts the process. Intentional for a financial component. |
| **`depth_snapshot` is slow-ish** | It allocates a `Vec` and scans ticks linearly (documented in the source). Reporting only. |

### Performance notes (not defects)

- `Order` is **48 bytes**: neither cache-line sized nor a divisor of 64, so many
  records straddle two lines ([chapter 3](03-order-pool.md#33-the-order-record)).
- Each call builds and returns a **2 KiB `EventBatch`** by value
  ([chapter 7](07-events.md#73-eventbatch-a-list-that-never-allocates)).
- `Order.sequence` is stored but never read, and `Order.in_use` is read only by a
  `debug_assert!`. They cost bytes but are useful for debugging and auditing.

## 10.2 Exercises

Difficulty: 🟢 warm-up · 🟡 a solid afternoon · 🔴 a real project.
Hints are deliberately short; the chapters contain the ingredients.

### Getting comfortable

🟢 **1. `Engine::spread()`.** Return `Option<u32>`: best ask minus best bid, or
`None` if either side is empty. Test it on the worked example from chapter 1.

🟢 **2. Order count per level.** `PriceLevel::count` already exists. Add
`Engine::order_count_at(side, tick) -> u32`. *Hint: mirror `depth_at`.*

🟢 **3. Draw the book.** Write a function that prints the ladder like the ASCII
diagrams in chapter 1 using `depth_snapshot`. Bonus: show each level's queue by
walking `head -> next_idx`. (You will need to expose read access to the pool.)

🟢 **4. Reproduce the event truncation.** Write a test that rests 40 sells at one
price, sweeps them with one buy, and asserts that (a) the book is empty and (b)
`events.len() == 64`. Then raise `MAX_EVENTS_PER_CALL` and watch the test change.

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

🟡 **7. Reusable `EventBatch`.** Add `process_order_into(&mut self, ..., out: &mut
EventBatch)` that clears and reuses the caller's batch, and make `process_order`
a thin wrapper. Benchmark the difference using the existing criterion benches.

🟡 **8. An `overflowed` flag.** Make `EventBatch` record whether any event was
dropped and expose `overflowed()`. Update the truncation test from exercise 4.

### New features

🟡 **9. IOC orders (immediate-or-cancel).** Add a time-in-force parameter. An IOC
order matches what it can and never rests. What should the event stream be for
the unfilled remainder: `OrderCancelled` or `OrderRejected`? Justify your choice.

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

🔴 **13. Differential testing.** Write a slow but obviously-correct reference book
(`BTreeMap<u32, VecDeque<(id, qty)>>` per side). Generate long random sequences of
`process_order` and `cancel_order` (a simple xorshift is enough, or use
`proptest`), run both implementations, and assert they emit **identical events**.
This is the single best way to gain confidence in an optimised data structure.
*Hint:* random streams will sometimes sweep 32 or more makers, and the real engine
truncates events at 64 (limitation #2), which would make the comparison fail for a
reason that is not a matching bug. Keep the generated quantities small relative to
resting sizes, or raise `MAX_EVENTS_PER_CALL` for the test, and compare the final
book (`depth_snapshot`) as well as the events.

🔴 **14. Three-level bitset.** Generalise `TwoLevelBitset` to `N` levels of `u64`
(3 levels = 262,144 ticks). Keep `lowest()` and `highest()` loop-free for a fixed
depth. Make `MAX_TICKS` follow.

🔴 **15. Type-safe SPSC.** Split `SpscRingBuffer` into a `Producer<T>` and a
`Consumer<T>` returned by `channel(capacity)`. Both should be `Send` but not
`Clone`, and `push`/`pop` should take `&mut self`. The data race from limitation #3
then becomes a compile error.

🔴 **16. Harden the WAL.** (a) *Already done:* `finalize` holds the map in an
`Option<MmapMut>` and drops it before `set_len`, which fixes the old Windows
truncation failure; read `wal.rs` to see it, and keep its tests green. (b) Add a
file header with a magic number and version. (c) Add a CRC32 to each record and
make `read_all` stop at the first record that fails it. (d) Write the records
field by field in little-endian so logs are portable (this also fixes limitation
#8, the uninitialised padding bytes, which a CRC over raw bytes would otherwise
pick up). (e) Fix limitation #9: add an open-for-append constructor that scans
for the last valid record instead of truncating. Keep the existing test green on
Windows *and* Linux.

🔴 **17. Keyed hash.** Give `FlatHashMap` a per-instance random seed mixed into
`hash`. Measure the cost. Is the extra protection (limitation #5) worth it for a
gateway you trust? (The seed changes only where entries land in the table, never
an observable result, so the engine stays deterministic.)

🔴 **18. Shard by symbol.** Run four `Engine`s on four threads, each fed by its own
ring buffer, routing requests by a symbol id. Verify that the books are
independent and throughput scales.

## 10.3 Further reading

- **How to Build a Fast Limit Order Book** (WK Selph): the classic write-up of
  the data-structure choices behind a fast book.
- **The LMAX Disruptor** technical paper (Thompson et al.) and Martin Thompson's
  *Mechanical Sympathy* blog: ring buffers, the single-writer principle, false
  sharing, cache effects.
- **Rust Atomics and Locks** (Mara Bos): memory ordering explained properly, the
  background for `ringbuffer.rs`.
- **The Art of Computer Programming, Vol. 3, section 6.4** (Knuth): hashing,
  including the linear-probing analysis quoted in chapter 5.
- **"How to Build an Exchange"** (Brian Nigito, Jane Street tech talk): how real
  exchanges structure the engine, sequencer and log.

**Back to:** [README](../README.md)
