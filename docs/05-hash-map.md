# 5. The Flat Hash Map: `OrderId → Arena Slot`

Source: [`src/hashmap.rs`](../src/hashmap.rs)

## 5.1 Why do we need it?

Matching never needs the map to *find* an order: it takes orders straight from
the head of a price queue (it only removes each fully filled maker from the map
afterwards). **Cancellation does need it.** A client says *"cancel order
12345"* and the engine must find where order 12345 lives in the arena, without
scanning the book.

```text
   cancel_order(12345)  ->  id_map.remove(12345)  ->  arena slot 987  ->  unlink it
```

The same map also powers **duplicate detection**: before accepting an order the
engine asks `id_map.get(order_id).is_some()` and rejects a reused id.

## 5.2 Why not `std::collections::HashMap`?

It is an excellent general-purpose map, but for this job:

- It **allocates**, and **resizes** (allocate + rehash everything) when it grows.
  Both are latency spikes.
- Its default hasher (SipHash) is designed to resist attackers who craft colliding
  keys; that costs cycles we do not want to spend per order. (This module gets
  most of that protection far more cheaply, with a secret seed: see 5.7.)
- We know the maximum number of entries up front (the pool's capacity), so we
  can size the table **once** and never resize.

So this module implements the smallest possible table that fits our needs.

## 5.3 Open addressing with linear probing

A hash table with *separate chaining* stores a list per bucket. An **open-addressing**
table stores everything **directly in one flat array**; if a slot is taken, it
looks at the next one. This variant is called **linear probing**.

```rust
struct Slot { key: u64, value: u32 }       // 16 bytes (with padding)

pub struct FlatHashMap {
    slots: Box<[Slot]>,    // the whole table
    mask: usize,           // slots.len() - 1
    len: usize,
    seed: u64,             // secret, random per table (see 5.7)
}
```

An unoccupied slot has `key == u64::MAX` (called `EMPTY` / `RESERVED_KEY`). That
means `u64::MAX` can never be a real order id (more in 5.8).

### Insert and lookup by example

> The four keys below are made up and their "ideal slots" are chosen to
> *collide* on purpose. Real hashes scatter keys differently, but the mechanics
> are identical.

Table with 8 slots. Insert `A`, `B`, `C`, `D`; their ideal slots (hash & mask) are
`A→2`, `B→2`, `C→3`, `D→2`.

```text
 slot:      0    1    2    3    4    5    6    7
          [   ][   ][ A ][   ][   ][   ][   ][   ]     insert A: ideal 2 is free       -> slot 2
          [   ][   ][ A ][ B ][   ][   ][   ][   ]     insert B: ideal 2 taken, try 3  -> slot 3
          [   ][   ][ A ][ B ][ C ][   ][   ][   ]     insert C: ideal 3 taken, try 4  -> slot 4
          [   ][   ][ A ][ B ][ C ][ D ][   ][   ]     insert D: 2,3,4 taken, try 5    -> slot 5
```

**Lookup** repeats the same walk: start at the ideal slot and step forward until
you either find the key (**hit**) or reach an `EMPTY` slot (**miss**: the key
cannot be further along, or it would have been placed in that empty slot).

```text
 get(C): start at ideal 3 -> B (not C) -> slot 4 -> C  FOUND    (2 probes)
 get(E) with ideal 2:  A, B, C, D, then slot 6 is EMPTY  -> NOT FOUND
```

The code is a tight loop:

```rust
pub fn get(&self, key: u64) -> Option<u32> {
    let mut idx = self.hash(key) & self.mask;
    loop {
        let slot = &self.slots[idx];
        if slot.key == EMPTY { return None; }          // reached a hole: miss
        if slot.key == key   { return Some(slot.value); } // hit
        idx = (idx + 1) & self.mask;                   // next slot, wrapping around
    }
}
```

> [!WARNING]
> This loop only terminates if the table always contains **at least one
> `EMPTY` slot**. A completely full table would make a lookup for a missing key
> spin forever. The engine's sizing leaves plenty of empty slots (5.5), and
> `insert` enforces the minimum for *any* user of the map: it panics rather than
> fill the last empty slot. A loud panic is far easier to debug than a silent hang.

## 5.4 Two tricks: power-of-two size and a good hash

### The mask trick

To turn a hash into a slot number you would normally write `hash % len`. Division
is slow. If `len` is a **power of two**, then

```text
   hash % len  ==  hash & (len - 1)            e.g. len = 32:  hash & 0b11111
```

is one cheap AND instruction. That is what `mask = capacity - 1` is for.
`FlatHashMap::with_capacity` rounds the size up with `next_power_of_two()`.

### The hash function

Order ids often follow patterns: strides (multiples of 1024), ids with a gateway
number in the high bits... Using `id & mask` directly would pile such keys into
the same few slots. (Plain sequential ids `1, 2, 3, ...` would happen to be
the best case for `id & mask`, but you cannot rely on that.) The function below mixes the bits
so that **every input bit affects every output bit** (an *avalanche*):

```rust
fn hash(&self, key: u64) -> usize {
    let mut z = (key ^ self.seed).wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    (z ^ (z >> 31)) as usize
}
```

Apart from the first XOR with `self.seed`, this is the finalizer of
**SplitMix64**: ten simple operations in total (four xors, one add, three shifts,
two multiplies), no branches, excellent distribution. The low bits, which the
mask keeps, are well mixed too.

> It is *not* cryptographically secure. The secret seed is what stops an
> attacker from predicting where an id lands; 5.7 shows why that matters.

## 5.5 Sizing: why the table is never full

```rust
let capacity = (capacity_hint.max(16) * 2).next_power_of_two();
```

The table has **at least twice as many slots as the maximum number of entries**
(rounded up to a power of two). The load factor, entries ÷ slots, therefore
never exceeds **50%**, and once the engine holds 16 or more orders' worth of
capacity it is above 25% when the book is completely full (it is lower while the
book is only partly full):

| `order_capacity` | `slots` | load when full |
|---|---|---|
| 10,000 | 32,768 | 30.5% |
| 16,384 | 32,768 | 50% |
| 16,385 | 65,536 | 25% |

And the entry count can never exceed `order_capacity`, because the engine inserts
into `id_map` only **after** `pool.allocate` succeeded, and the pool holds at most
`order_capacity` orders. So the table always has plenty of empty slots.

Two guards make the map safe to use on its own as well:

- The size calculation uses checked arithmetic
  (`checked_mul(2)`, then `checked_next_power_of_two()`). An absurd hint panics
  with "hash map capacity overflows usize" instead of wrapping to a tiny table in
  a release build, where overflow checks are off.
- `insert` panics with "FlatHashMap is full" rather than fill the last empty
  slot, so the probe loops from 5.3 always terminate.

Why keep it so sparse? Linear probing degrades quickly as a table fills. The
classic (Knuth) estimates for the average number of probes at load factor `α`:

| Load `α` | Hit (found) | Miss (not found) |
|---|---|---|
| 25% | ≈ 1.2 | ≈ 1.4 |
| 50% | 1.5 | 2.5 |
| 90% | 5.5 | 50.5 |

Half-empty is a good memory/speed trade-off: a cancellation usually costs one or
two probes in one or two adjacent cache lines.

## 5.6 Deleting: the hard part

Insert and lookup were easy. **Removal** is where open addressing gets tricky.

### The naive approach is broken

Take the table from earlier and simply mark `B`'s slot empty:

```text
 slot:      0    1    2    3    4    5    6    7
          [   ][   ][ A ][ B ][ C ][ D ][   ][   ]
   remove B, naively:
          [   ][   ][ A ][   ][ C ][ D ][   ][   ]
                          ^ hole

   get(C): start at ideal 3 -> EMPTY -> "NOT FOUND"      WRONG! C is at slot 4.
```

A lookup stops at the first empty slot, so the hole we created **cuts the probe
chain** and hides `C` and `D`.

### Option 1: tombstones (not used)

Mark the slot `DELETED` instead of `EMPTY`; lookups skip over tombstones. It
works, but tombstones accumulate, probe chains grow, and eventually the whole
table must be rebuilt (a big latency spike). The source comment explicitly calls
this out as the thing to avoid.

### Option 2: backward-shift deletion (used here)

After creating the hole, **walk forward and pull back any entry that would
otherwise become unreachable**, leaving no tombstone at all.

The rule for an entry sitting at `probe`, whose ideal slot is `ideal`:

> It may move into the hole if its probe path (`ideal → ... → probe`) **passes
> through the hole**, i.e. if its ideal slot is *at or before* the hole.

Moving it there is always safe: the new position is still on its own probe path,
and closer to its ideal slot.

**Example:** remove `B` from `[_, _, A, B, C, D, F, _]`, where `F` sits in its own
ideal slot 6.

```text
 start   slot:  0  1  2  3  4  5  6  7
                _  _  A  B  C  D  F  _          hole := 3 (B removed)
                _  _  A  .  C  D  F  _

 probe 4: C, ideal 3 <= hole 3  -> its path crosses the hole -> MOVE to 3, hole := 4
                _  _  A  C  .  D  F  _

 probe 5: D, ideal 2 <= hole 4  -> MOVE to 4, hole := 5
                _  _  A  C  D  .  F  _

 probe 6: F, ideal 6 is NOT <= hole 5 -> its path never touches the hole -> STAY
 probe 7: EMPTY -> stop.

 result         _  _  A  C  D  .  F  _          everyone is still reachable
```

The table is circular, so the "at or before" test has to work when a chain
wraps from the end of the array to the start. That is why the source has two
branches:

```rust
let displaced = if probe >= hole {
    ideal <= hole || ideal > probe        // normal case (or the path wrapped around)
} else {
    ideal <= hole && ideal > probe        // `probe` has wrapped past the end
};
```

(The variable is named `displaced`, but read it as **"can be moved back into the
hole"**.)

**Wrap-around example** (8 slots): `P` (ideal 6) at slot 6, `Q` (ideal 7) at slot
7, `R` (ideal 6) wrapped to slot 0. Remove `P` (hole = 6):

- Probe 7: `Q`, ideal 7; `7 <= 6`? no, `7 > 7`? no → stays.
- Probe 0 (wrapped, `probe < hole`): `R`, ideal 6; `6 <= 6 && 6 > 0` → **moves** to
  slot 6. Hole is now 0.
- Probe 1 is `EMPTY` → stop.

## 5.7 Complexity

| Operation | Expected | Worst case |
|---|---|---|
| `get` | O(1), about 1.5 probes | O(capacity) if all keys collide |
| `insert` | O(1) | O(capacity) |
| `remove` | O(1) plus a short backward shift | O(capacity) |

For ordinary workloads this is "O(1)", and the doc comments are careful to say
"expected O(1)".

### Crafted ids, and the secret seed

The worst case is reachable by anyone who can **choose order ids** and predict
the hash. Without a seed, SplitMix64's finalizer is fully **invertible**: adding a
constant, `z ^ (z >> k)` and multiplying by an odd constant can each be undone.
So an attacker could compute thousands of ids that all hash to slot 0, and every
lookup would then walk one long probe chain: a cheap denial-of-service attack on
latency.

The defence costs one instruction: XOR a **secret, random seed** into the key
before mixing. `FlatHashMap::with_capacity` draws it from the standard library's
`RandomState`, the same randomness that keys `std`'s SipHash. Without the seed an
attacker cannot run the inversion, so they cannot aim ids at a slot.

The test `keys_crafted_for_one_seed_spread_out_under_another` demonstrates both
halves. Its `craft_key` function inverts the finalizer to build 1,000 ids that
all target slot 0 under seed `0`, and checks that they really do. Under any other
seed the same ids scatter like ordinary keys (the test requires the fullest
bucket to hold fewer than 32 of them).

> [!NOTE]
> **Randomness without losing determinism.** The seed only changes *where*
> entries are stored, never what `get`, `insert` or `remove` return. The engine
> therefore stays deterministic: the same requests always produce the same events
> and the same book, whatever the seed. `tests/wal_replay.rs` relies on exactly
> this: it replays a log into a second engine, which draws a different seed, and
> requires identical results. Tests that want a reproducible *layout* use
> `FlatHashMap::with_capacity_and_seed`.

## 5.8 The reserved key

`u64::MAX` marks an empty slot, so it cannot also be a key. Two layers defend
this:

1. **`Engine::process_order`** rejects `order_id == RESERVED_KEY` up front,
   emitting `OrderRejected`. (Without this check, a client submitting id
   `u64::MAX` would reach layer 2 below and abort the process: a denial of
   service. Without *both* layers, the entry would be silently unreachable.)
2. **`FlatHashMap::insert`** has a hard `assert!` (not `debug_assert!`), so even a
   direct misuse fails loudly in release builds.

`get` and `remove` of `u64::MAX` are harmless: they simply report "not found",
like any other miss (the probe runs until it reaches an empty slot). The test `rejects_reserved_order_id_sentinel` covers the engine path,
and `insert_rejects_reserved_sentinel_key_in_all_build_profiles` covers the map.

## 5.9 Recap

- Flat array of `(key, value)` slots, power-of-two sized, at most 50% full, and
  never completely full (`insert` refuses to fill the last empty slot).
- Hash with SplitMix64's finalizer after XORing in a secret per-table seed, pick
  the slot with `& mask`.
- Collisions resolved by stepping to the next slot (linear probing).
- Delete with **backward shift** instead of tombstones, so there is nothing to
  clean up later.
- Sized once, never resizes, never allocates after `with_capacity`.

**Next:** [6. The matching algorithm](06-matching-algorithm.md)
