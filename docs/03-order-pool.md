# 3. The Order Pool: Arena, Free List and FIFO Queues

Source: [`src/pool.rs`](../src/pool.rs)

This module answers two questions:

1. **Where do orders live in memory?** In one big pre-allocated array (an *arena*).
2. **How do we keep orders at the same price in arrival order?** With linked
   lists whose links are array indices (an *intrusive* linked list).

## 3.1 The problem

A price level needs a queue of orders: new orders join at the back, matching
takes from the front, and cancellation removes from *anywhere*. The obvious Rust
solution is `VecDeque<Order>` or `LinkedList<Order>`, but:

- `VecDeque` removes from the middle in O(n) (everything after it shifts).
- `LinkedList` allocates a heap node for every order, and nodes end up scattered
  around memory.
- Both allocate on the hot path.

We want: **O(1) push to back, O(1) pop from front, O(1) remove from the middle,
no allocation.**

## 3.2 Idea 1: an arena

An **arena** is one big array, allocated once. Objects are referred to by their
**index** in the array instead of by pointer.

```text
   orders: Box<[Order]>   (length = capacity, fixed forever)

   index:   0      1      2      3      4      5     ...   capacity-1
          +------+------+------+------+------+------+-----+------+
          |Order |Order |Order |Order |Order | free | ... | free |
          +------+------+------+------+------+------+-----+------+
```

Why indices instead of pointers?

- A `u32` index is **4 bytes**, a pointer is **8**. Two links per order: that
  saves 8 bytes per order, so more orders fit in cache.
- Orders sit **next to each other** in memory, which the CPU prefetcher loves.
- No `unsafe`, no lifetimes: an index is just a number, so Rust's borrow checker
  is not fighting us. (A self-referential web of `&mut Order` references would
  be impossible to express in safe Rust.)

## 3.3 The `Order` record

```rust
#[repr(C)]
pub struct Order {
    pub order_id: u64,        // user's id
    pub remaining_qty: u64,   // not yet executed
    pub sequence: u64,        // arrival counter (metadata)
    pub price_tick: u32,      // which price level it rests at
    pub side: Side,           // Buy or Sell (1 byte)
    pub next_idx: u32,        // next order in the same price queue (towards the tail)
    pub prev_idx: u32,        // previous order in the same price queue (towards the head)
    pub in_use: bool,         // is this slot currently holding a live order?
}
```

`#[repr(C)]` keeps fields in declaration order. The compiler inserts *padding*
so every field is aligned. Measured with `offset_of!`:

```text
 byte  0         8        16        24   28   32        36        40   41     48
       +---------+---------+---------+----+----+--------+--------+----+------+
       |order_id |remaining|sequence |tick|side|next_idx|prev_idx|in_ |      |
       |  u64    | _qty u64|  u64    |u32 | u8 |  u32   |  u32   |use |      |
       +---------+---------+---------+----+----+--------+--------+----+------+
                                            ^^^ 3 bytes padding             ^^^ 7 bytes padding
       total size_of::<Order>() = 48 bytes
```

> [!NOTE]
> The source comment says the order is "sized to fit within a single 64-byte
> cache line". True in the sense that 48 ≤ 64, but 48 does not divide 64, so in a
> tightly packed array many orders **straddle two cache lines** (if the array
> starts on a line boundary, order 1 covers bytes 48 to 95, crossing the
> boundary at 64). Two easy experiments are listed
> in [chapter 10](10-limitations-and-exercises.md): reorder the fields to
> shrink the record to 40 bytes, or align it to 64 bytes so each order owns
> exactly one cache line.

## 3.4 Idea 2: the free list (a stack of free slots)

How do we find an unused slot in O(1)? Keep a **stack of free indices**:

```rust
pub struct OrderPool {
    orders: Box<[Order]>,
    free_stack: Box<[u32]>,   // free_stack[..free_top] are the free slot indices
    free_top: usize,          // number of free slots
}
```

- **allocate** = pop the top index of the stack.
- **free** = push the index back onto the stack.

Both are a couple of instructions. Here is a trace with `capacity = 4`:

```text
 new(4)           free_stack = [3, 2, 1, 0]   free_top = 4     (nothing in use)
                                        ^top

 allocate()  -> 0 free_stack = [3, 2, 1, 0]   free_top = 3     (slot 0 in use)
 allocate()  -> 1 free_stack = [3, 2, 1, 0]   free_top = 2     (slots 0,1 in use)
                                  ^top        (entries above the top are stale, ignored)

 free(0)          free_stack = [3, 2, 0, 0]   free_top = 3     (slot 0 pushed back)
 allocate()  -> 0 free_stack = [3, 2, 0, 0]   free_top = 2     (slot 0 reused first)
```

The stack is filled in reverse (`capacity-1, ..., 1, 0`) so that the first
`allocate` returns slot `0`, the next slot `1`, and so on. (Purely cosmetic, as
the source comment says.)

Because it is a *stack* (last in, first out), the most recently freed slot is
reused first. That slot's memory was touched recently, so it is likely still in
cache: a small free win.

`live_count()` is simply `capacity - free_top`.

### Allocation failure

If `free_top == 0` the arena is full and `allocate` returns `None`. The engine
turns that into an `OrderRejected` event (only for an order's resting
remainder; an order that fully matches needs no slot).

## 3.5 Idea 3: an intrusive doubly-linked list

"Intrusive" means the list links are stored **inside the element itself**
(`next_idx`, `prev_idx` in `Order`) rather than in a separate node object. So
the order *is* the list node: no extra allocation.

Each price tick has a tiny descriptor, `PriceLevel`:

```rust
pub struct PriceLevel {
    pub head: u32,       // oldest order (first to trade), or NIL if empty
    pub tail: u32,       // newest order (last to trade), or NIL if empty
    pub count: u32,      // number of orders at this level
    pub total_qty: u64,  // sum of remaining_qty of all orders (O(1) depth queries)
}
```

The queue for ask tick 101 from the running example (`#2` then `#3`, in arena
slots 1 and 2):

```text
   ask_levels[101]:  head=1  tail=2  count=2  total_qty=7
                       |        |
                       v        v
   slot 1  [ #2 qty 3 | prev=NIL | next=2  ]  <---->  slot 2  [ #3 qty 4 | prev=1 | next=NIL ]
           (head, oldest)                                      (tail, newest)
```

### `NIL`: "no link"

```rust
pub const NIL: u32 = u32::MAX;
```

`NIL` plays the role of a null pointer. We use a sentinel value instead of
`Option<u32>` because `Option<u32>` is **8 bytes** (4 for the value plus a
tag with alignment padding), which would double the size of both links. That is
also why `OrderPool::new` requires `capacity < u32::MAX`: the value `u32::MAX`
is reserved.

### Operation 1: `push_back` (a new order joins the queue)

Adds slot `idx` at the tail. Two cases:

```text
 CASE A: level was empty (tail == NIL)

   before:  head=NIL tail=NIL
   push(X)
   after:   head=X   tail=X        X.prev=NIL  X.next=NIL


 CASE B: level has orders

   before:  head=A  ...  tail=C
            A <-> B <-> C
   push(X)
      1. C.next = X
      2. X.prev = C
      3. X.next = NIL
      4. tail   = X
   after:   A <-> B <-> C <-> X       head=A  tail=X
```

Bookkeeping: `count += 1` and `total_qty += X.remaining_qty`. The addition is a
`checked_add` (see 3.7).

### Operation 2: `remove` (from head, middle or tail)

Because every node knows both neighbours, removal never has to search:

```text
 remove(B) from   A <-> B <-> C           (middle)
    A.next = C ; C.prev = A               =>   A <-> C

 remove(A) from   A <-> B <-> C           (head)
    prev is NIL, so head = B ; B.prev = NIL  =>   B <-> C   head=B

 remove(C) from   A <-> B <-> C           (tail)
    next is NIL, so tail = B ; B.next = NIL  =>   A <-> B   tail=B
```

The code is a direct translation:

```rust
if prev != NIL { pool.get_mut(prev).next_idx = next; } else { self.head = next; }
if next != NIL { pool.get_mut(next).prev_idx = prev; } else { self.tail = prev; }
```

Then the removed node's own links are reset to `NIL`, and `count` and
`total_qty` are decreased (with checked subtraction, see 3.7).

This is exactly why the list is **doubly** linked: to cancel `B` in O(1) we must
be able to reach `A` (to update `A.next`) without walking from the head.

### Operation 3: `reduce_qty` (partial fill)

When a maker is partly filled, it stays at the head. Only the numbers change:

```text
   order.remaining_qty -= delta
   level.total_qty     -= delta
```

The queue order (and therefore the maker's time priority) is untouched. A
partly-filled order **keeps its place** at the front.

## 3.6 Invariants (things that must always be true)

These are the rules the rest of the engine relies on. If one breaks, the book is
corrupt.

| # | Invariant | Maintained by |
|---|---|---|
| 1 | `level.head == NIL` ⇔ `level.tail == NIL` ⇔ `level.count == 0` ⇔ `level.total_qty == 0` | `push_back`, `remove` |
| 2 | `level.total_qty` = sum of `remaining_qty` over every order in the queue. (The "`total_qty == 0` ⇔ empty" half of invariant 1 also relies on the engine never resting a zero-quantity order: `process_order` rejects `qty == 0`; `PriceLevel` itself does not check.) | `push_back` (+), `remove` (−), `reduce_qty` (−) |
| 3 | Walking `head -> next_idx -> ...` visits exactly `count` orders and ends at `tail` | `push_back`, `remove` |
| 4 | `a.next_idx == b` ⇔ `b.prev_idx == a` | `push_back`, `remove` |
| 5 | A slot is on the free stack ⇔ `in_use == false` | `allocate`, `free` |
| 6 | Every live order is in exactly one queue **and** has an `id_map` entry | `Engine` |

## 3.7 Defensive checks

The pool protects itself against three kinds of trouble, all visible in the code:

**Overflow on insert.** `push_back` uses `checked_add` on `total_qty`. If a
hostile or buggy client sends a quantity so large that the level's total would
exceed `u64::MAX`, `push_back` returns `false` *without touching anything*. The
method is `#[must_use]`, and the engine reacts by removing the order's `id_map`
entry, freeing the slot and emitting `OrderRejected`.

**Underflow on remove / reduce.** `remove` decreases both `count` and
`total_qty`, and `reduce_qty` decreases `remaining_qty` and `total_qty`, all with
`.checked_sub(..).expect(..)`. If the books are ever inconsistent, the program
panics immediately with a message naming the broken invariant, instead of
silently wrapping a counter to a gigantic number.

**Double free.** `OrderPool::free` checks the slot's `in_use` flag with a real
`assert!`. Freeing the same slot twice would otherwise push it on the free stack
twice, and two different orders could later receive the same slot, silently
corrupting both of their queues. The engine never does this; the check is there
so that a bug in new code (an exercise, say) fails at the faulty call instead of
much later.

> [!NOTE]
> **Why `assert!` and not `debug_assert!`?** A `debug_assert!` is compiled out
> of release builds, which is exactly where nobody is watching. Release builds
> also stop checking arithmetic overflow, so a bare `count -= 1` would wrap
> silently there. These checks cost one predictable branch each, on data that is
> already in cache. The crate's release profile sets `panic = "abort"`, which
> turns such a panic into a process abort in a binary built with that setting
> (see [chapter 9](09-testing-and-performance.md#95-build-profiles) for the
> caveat about libraries). That is the intent: for a financial system, stopping
> loudly beats continuing with a corrupt book.

## 3.8 Walk-through with the running example

Start from an empty engine with capacity 16 and replay the first three orders of
the worked example. Every call also takes the event sink as its last argument
(`&mut events`); it is left out of the table to keep the rows short.

| Step | Call | `pool` effect | Queue state |
|---|---|---|---|
| 1 | `process_order(1, Sell, 102, 5)` | `allocate` -> slot 0 | `ask[102]`: slot 0 |
| 2 | `process_order(2, Sell, 101, 3)` | `allocate` -> slot 1 | `ask[101]`: slot 1 |
| 3 | `process_order(3, Sell, 101, 4)` | `allocate` -> slot 2; `push_back` links `1 -> 2` | `ask[101]`: slot 1, slot 2 (`total_qty = 7`) |
| 4 | `cancel_order(2)` *(hypothetical)* | `remove(slot 1)`: head becomes slot 2; `free(1)` | `ask[101]`: slot 2 only (`total_qty = 4`) |
| 5 | `process_order(7, Sell, 100, 1)` *(new id; `#4` is taken in chapter 1)* | `allocate` pops the **most recently freed** slot 1 | `ask[100]`: slot 1 |

Step 5 shows LIFO reuse: slot 1 was freed in step 4 and is the first one handed
out again.

## 3.9 Recap

- **Arena**: orders live in one fixed array and are named by `u32` index.
- **Free stack**: O(1) allocate and free, most-recently-freed reused first.
- **Intrusive doubly-linked list**: links live inside the orders, giving O(1)
  push-back, pop-front and remove-from-middle with zero allocation.
- **`PriceLevel`** caches `count` and `total_qty`, so depth queries and
  "is this level empty?" checks are O(1).

**Next:** [4. The price ladder bitset](04-price-ladder-bitset.md)
