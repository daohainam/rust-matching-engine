# 4. The Price Ladder: A Two-Level Bitset

> **New word?** Every technical term in these chapters is explained in the
> [glossary](glossary.md).

Source: [`src/bitset.rs`](../src/bitset.rs)

This module answers one question, over and over, in O(1):

> **"What is the best price that currently has at least one order?"**

- For bids, the best price is the **highest** non-empty tick.
- For asks, the best price is the **lowest** non-empty tick.

## 4.1 The problem

The matching loop needs the best opposing price at every step. Some options:

| Approach | Cost to find the best price | Notes |
|---|---|---|
| Scan `levels[0..4096]` for the first non-empty one | up to 4096 steps | Simple, but far too slow and unpredictable. |
| Sorted tree (`BTreeMap`) | O(log n) | Pointer chasing; allocates on insert/remove. |
| Cached "best" value, recompute when it empties | O(1) usually, O(n) sometimes | The occasional rescan is exactly the latency spike we avoid. |
| **Bitset with bit-scan** | **O(1), always** | One bit per tick; the CPU finds a set bit in one instruction. |

## 4.2 Idea 1: one bit per price tick

Keep a bit for every tick: `1` = "at least one order rests here", `0` = empty.
4096 ticks need 4096 bits = 64 words of 64 bits.

```text
 words[0]  : ticks    0 ..  63      0000...0000
 words[1]  : ticks   64 .. 127      0000...0000
   ...
 words[63] : ticks 4032 .. 4095     0000...0000
```

To find the lowest set bit we can use a CPU instruction: **count trailing
zeros**. To find the highest set bit: **count leading zeros**. Both are a single
instruction on modern CPUs, but only on *one* 64-bit word. Scanning 64 words
still means up to 64 steps. Idea 2 removes that.

### Bit-scan in 30 seconds

Using 8-bit numbers to keep it short:

```text
   value = 0b0010_1000
               ^  ^
               |  +-- lowest set bit  = bit 3
               +----- highest set bit = bit 5

   trailing_zeros(value) = 3            -> index of the LOWEST set bit
   leading_zeros(value)  = 2            -> index of the HIGHEST set bit = (8 - 1) - 2 = 5
```

For a `u64` the highest set bit is `63 - leading_zeros`. In Rust these are the
methods `u64::trailing_zeros` and `u64::leading_zeros`. The compiler lowers them
to one or two instructions (for example `tzcnt`/`bsf` and `lzcnt`/`bsr` on x86-64,
`clz` or `rbit` + `clz` on ARM). The exact instruction depends on the compile target; building
with `-C target-cpu=native` lets the compiler use the newest ones.

## 4.3 Idea 2: a summary word on top

Add a **second level**: one 64-bit `summary` word where **bit `g` means "word `g`
is not all zeros"**.

```text
                         summary (u64): one bit per word
        bit:  63 62 ...      31 ...            3  2  1  0
             +--+--+-----+----+-----+---------+--+--+--+--+
             | 0| 0| ... | 1  | ... |         | 0| 0| 1| 0|
             +--+--+-----+-|--+-----+---------+--+--+-|+--+
                           |                          |
            +--------------+                          +-----+
            v                                               v
   words[31]  ticks 1984..2047                     words[1]  ticks 64..127
   +-----------------------------+                 +----------------------------+
   | ... 1 at bit 16 ...         |                 | ... 1 at bits 36 and 37 ...|
   +-----------------------------+                 +----------------------------+
        = tick 2000                                     = ticks 100 and 101
```

In code:

```rust
pub struct TwoLevelBitset {
    summary: u64,           // level 2: bit g set  <=>  words[g] != 0
    words: [u64; 64],       // level 1: bit b of words[g] set <=> tick g*64+b is active
}
```

Size: `8 + 64 × 8 = 520 bytes`. The whole structure spans only 9 to 10 cache lines, and in
practice only the summary plus one or two words are touched per operation.

### Converting a tick to a (word, bit) position

With `64 = 2^6`, division and modulo become a shift and a mask:

```rust
let word_idx = tick >> 6;    // tick / 64
let bit_idx  = tick & 63;    // tick % 64
```

Example, `tick = 1234`: `1234 >> 6 = 19` (since 19 × 64 = 1216) and
`1234 & 63 = 18`. So tick 1234 is **bit 18 of word 19**. Check: 19 × 64 + 18 = 1234. ✓

## 4.4 The operations

> Code excerpts in this chapter are lightly condensed (for example, the source
> copies `words[word_idx]` into a local variable first, and writes the array
> length as the constant `WORDS`, which is 64). The behaviour is identical to
> [`src/bitset.rs`](../src/bitset.rs).

### `set(tick)`: mark a tick active

```rust
self.words[word_idx] |= 1u64 << bit_idx;   // level 1: set the tick's bit
self.summary         |= 1u64 << word_idx;  // level 2: mark its word non-empty
```

Setting an already-set bit changes nothing, so `set` is idempotent. (The engine
calls it every time an order rests, even if the level already had orders.)

### `lowest()`: best ask

```rust
let word_idx = self.summary.trailing_zeros() as usize;   // step 1: first non-empty word
let bit_idx  = self.words[word_idx].trailing_zeros() as usize; // step 2: first set bit in it
Some(word_idx * 64 + bit_idx)
```

### `highest()`: best bid

```rust
let word_idx = 63 - self.summary.leading_zeros() as usize;       // last non-empty word
let bit_idx  = 63 - self.words[word_idx].leading_zeros() as usize; // last set bit in it
Some(word_idx * 64 + bit_idx)
```

Both return `None` immediately if `summary == 0`. **Two bit-scans, no loops**,
regardless of whether the book has 2 levels or 4000.

### `clear(tick)`: mark a tick empty

```rust
self.words[word_idx] &= !(1u64 << bit_idx);      // clear the tick's bit
if self.words[word_idx] == 0 {                   // ...only if the word is now all zeros...
    self.summary &= !(1u64 << word_idx);         // ...clear the summary bit too
}
```

The `if` matters. Word 1 may hold ticks 100 **and** 101. Clearing 100 must **not**
clear the summary bit, or the summary would claim word 1 is empty while tick 101
is still active (a "false negative") and the engine would never see the order at
101.

### `test(tick)`: is a tick active?

Just reads one bit. Used by `depth_snapshot` to walk levels (see 4.7).

## 4.5 Worked example

Activate ticks **100, 101 and 2000**.

```text
 tick 100  -> word 1  (100 >> 6 = 1),  bit 36 (100 - 64)
 tick 101  -> word 1,                  bit 37
 tick 2000 -> word 31 (2000 >> 6 = 31, since 31*64 = 1984), bit 16 (2000 - 1984)

 words[1]  = bits {36, 37}
 words[31] = bit  {16}
 summary   = bits {1, 31}
```

**`lowest()`**

1. `summary.trailing_zeros()` = **1** → the first non-empty word is `words[1]`.
2. `words[1].trailing_zeros()` = **36**.
3. Result: `1 × 64 + 36 = 100`. ✓

**`highest()`**

1. `summary.leading_zeros()` = 32, so the word index is `63 − 32` = **31**.
2. `words[31].leading_zeros()` = 47, so the bit index is `63 − 47` = **16**.
3. Result: `31 × 64 + 16 = 2000`. ✓

**`clear(100)`:** `words[1]` becomes `{37}`, which is non-zero, so the summary is
unchanged. `lowest()` now returns 101.

**`clear(101)`:** `words[1]` becomes `0`, so summary bit 1 is cleared too. The
summary is now `{31}`, and `lowest()` returns 2000.

## 4.6 Why 4096 ticks?

`4096 = 64 × 64`. One summary word has exactly 64 bits, one per word, so two
levels cover 64 × 64 ticks. That is also the limit on `TICKS`: a multiple of 64,
and at most 64 × 64 = 4096. A larger value would make the `1u64 << word_idx`
shift in `set` overflow the summary word.

The source does not just document this limit, it makes the compiler check it:

```rust
const _: () = assert!(
    TICKS.is_multiple_of(64) && TICKS <= 64 * 64,
    "TICKS must be a multiple of 64 and at most 4096"
);
```

`const _` evaluates the expression while compiling, so an invalid `TICKS` never
produces a program at all. Try it: set `TICKS` to 4160 and `cargo build` stops
with `error[E0080]: evaluation panicked: TICKS must be a multiple of 64 and at
most 4096`. Turning a comment into a compile-time check is cheap insurance for
exactly the kind of change exercise 13 asks for.

Need more ticks? Add another level: three levels of `u64` cover `64³ = 262,144`
ticks, with `highest()` / `lowest()` still taking only 3 bit-scans. (That is one
of the exercises in [chapter 11](11-limitations-and-exercises.md).)

## 4.7 How the engine uses it

The `Engine` owns **two** bitsets, one per side:

| Bitset | Bit set when... | Best price is... | Query |
|---|---|---|---|
| `bid_bitset` | a bid rests at that tick | the **highest** set tick | `highest()` (`Engine::best_bid`) |
| `ask_bitset` | an ask rests at that tick | the **lowest** set tick | `lowest()` (`Engine::best_ask`) |

The bit is maintained at exactly three places:

1. **Order rests** (`process_order`, after `push_back` succeeds): `set(tick)`.
2. **Level drained by matching** (`match_incoming`, when `level.is_empty()`):
   `clear(tick)`.
3. **Last order at a level cancelled** (`cancel_order`): `clear(tick)`.

**Invariant:** bit `t` is set ⇔ `levels[t]` is non-empty. The bitset is an
*index* over the level array: a fast way to find non-empty entries.

> [!NOTE]
> `Engine::depth_snapshot` needs the *next* non-empty level after the best one.
> The current code does that by calling `test(t)` on each tick in turn
> (`next_lower_active` / `next_higher_active`), a linear scan, even though the
> bitset could answer faster by masking off the bits at or below `t` (for asks,
> which scan upward) or at or above `t` (for bids, which scan downward) and
> bit-scanning what is left. That is an easy, instructive optimisation (see
> exercises). It is not on the order-processing hot path.

## 4.8 Recap

- One bit per tick, grouped in 64-bit words.
- A summary word records which words are non-empty.
- Best price = two hardware bit-scans: **O(1)**, no loops, no allocation.
- `clear` only clears the summary bit when the whole word becomes empty.

## 4.9 Try it yourself

Run the bitset's unit tests:

```sh
cargo test bitset::
```

Then write a small test of your own at the end of the `tests` module in
[`src/bitset.rs`](../src/bitset.rs): set ticks `5`, `70` and `4000`, predict what
`lowest()` and `highest()` return, and which bits of `summary` are set, then
check with `assert_eq!`. Clear tick `5` and predict again.

**Next:** [5. The flat hash map](05-hash-map.md)
