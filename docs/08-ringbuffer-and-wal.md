# 8. Around the Engine: Ring Buffer and Write-Ahead Log

Sources: [`src/ringbuffer.rs`](../src/ringbuffer.rs), [`src/wal.rs`](../src/wal.rs)

The `Engine` is deliberately just a data structure. A real system needs two more
things around it:

1. A way to **feed it requests** from other threads without locks → the
   **ring buffer**.
2. A way to **survive a crash** and reproduce its state → the **write-ahead log**.

Neither is used *by* the engine; they are independent building blocks. The last
section of this chapter shows how to wire all three together.

```text
  ONE gateway thread             matching thread (owns the Engine)              downstream
 +---------------+  SPSC ring   +-----------------------------------+        +------------+
 | parse / check | -----------> | pop -> WAL append -> Engine call  | -----> | publish    |
 | requests      |   buffer     |                                   | events | events     |
 +---------------+              +-----------------------------------+        +------------+
```

(An SPSC queue allows exactly one producer. With several gateway threads you need
either one SPSC queue per gateway, merged by the matching thread, or a
multi-producer queue, which this crate does not provide.)

---

# Part A: The SPSC ring buffer

## 8.1 What problem does it solve?

Requests arrive on network threads, but the `Engine` must be touched by **one**
thread only. We need a queue between them. A `Mutex<VecDeque<_>>` would work, but
locks can block (the thread gets descheduled while holding the lock) and
`VecDeque` may allocate. Both hurt tail latency.

The solution is a **lock-free, fixed-size, single-producer single-consumer
(SPSC) ring buffer**, in the style of the LMAX Disruptor.

- **Single producer**: exactly one thread calls `push`.
- **Single consumer**: exactly one thread calls `pop`.

Restricting ourselves to exactly one of each is what allows such a simple
design, with no compare-and-swap loops and no contention. As 8.5 shows, the API
makes it impossible to break that restriction by accident.

## 8.2 The data structure

A fixed array used as a circle, plus two counters:

```text
   capacity = 8  (power of two),  mask = 7

   slot:    0     1     2     3     4     5     6     7
          +-----+-----+-----+-----+-----+-----+-----+-----+
          |     |     |  C  |  D  |  E  |     |     |     |
          +-----+-----+-----+-----+-----+-----+-----+-----+
                          ^                 ^
                        head = 2          tail = 5
                     (consumer reads)   (producer writes)

   queued items  = tail - head = 3         (C, D, E)
   empty         : head == tail
   full          : tail - head == capacity
```

`spsc_channel` allocates the slots once and returns the two ends of the queue:

```rust
pub fn spsc_channel<T: Copy + Send>(capacity: usize) -> (Producer<T>, Consumer<T>)

/// Storage and cursors shared by the one Producer and the one Consumer.
struct Shared<T> {
    buffer: Box<[UnsafeCell<MaybeUninit<T>>]>,
    mask: usize,
    head: CachePadded<AtomicUsize>,   // consumer cursor: values popped so far
    tail: CachePadded<AtomicUsize>,   // producer cursor: values pushed so far
    producer_dropped: AtomicBool,     // disconnect flags, see 8.5
    consumer_dropped: AtomicBool,
}

pub struct Producer<T> { shared: Arc<Shared<T>>, tail: usize, cached_head: usize }
pub struct Consumer<T> { shared: Arc<Shared<T>>, head: usize, cached_tail: usize }
```

Each handle keeps a private copy of **its own** cursor (only it ever advances
that cursor, so its copy is always exact) and a **cached** copy of the other
side's cursor (8.3 explains why).

The slots are `MaybeUninit<T>`: memory that may or may not hold a value yet.
Slots between `head` and `tail` hold values; the others are uninitialised. No
`Option` tag is needed, because the cursors already say which slots are full.

**The cursors only ever grow** (using wrapping arithmetic on overflow); they are
never reset to zero. The physical slot is `cursor & mask`.

Why grow forever instead of wrapping at `capacity`? With cursors in
`0..capacity` you cannot tell "empty" from "full" (both look like `head == tail`)
without wasting a slot. With ever-increasing cursors the difference `tail − head`
says exactly how many items are queued.

**Wrap-around example** (`capacity = 8`): `head = 6`, `tail = 10`. Four items are
queued in slots `6, 7, 0, 1` (because `6 & 7 = 6`, `7 & 7 = 7`, `8 & 7 = 0`,
`9 & 7 = 1`). The next write goes to slot `10 & 7 = 2`.

Capacity is rounded up to a power of two so `% capacity` becomes `& mask`, the
same trick used by the hash map.

## 8.3 `push` and `pop`

```rust
pub fn push(&mut self, value: T) -> Result<(), T> {
    let capacity = self.capacity();
    if self.tail.wrapping_sub(self.cached_head) >= capacity {
        // Looks full: refresh our copy of the consumer's cursor, then re-check.
        self.cached_head = self.shared.head.0.load(Ordering::Acquire);
        if self.tail.wrapping_sub(self.cached_head) >= capacity {
            return Err(value);                                   // really full
        }
    }
    let slot = &self.shared.buffer[self.tail & self.shared.mask];
    unsafe { slot.get().write(MaybeUninit::new(value)) };        // write the slot
    self.tail = self.tail.wrapping_add(1);
    self.shared.tail.0.store(self.tail, Ordering::Release);      // publish it
    Ok(())
}

pub fn pop(&mut self) -> Option<T> {
    if self.head == self.cached_tail {
        // Looks empty: refresh our copy of the producer's cursor, then re-check.
        self.cached_tail = self.shared.tail.0.load(Ordering::Acquire);
        if self.head == self.cached_tail {
            return None;                                         // really empty
        }
    }
    let slot = &self.shared.buffer[self.head & self.shared.mask];
    let value = unsafe { slot.get().read().assume_init() };      // read the slot
    self.head = self.head.wrapping_add(1);
    self.shared.head.0.store(self.head, Ordering::Release);      // free it
    Some(value)
}
```

`push` returns `Err(value)` when full, handing the value back so the caller can
retry or drop it. Nothing ever blocks or allocates.

**The cached cursors** are a big part of the speed. Reading the other side's
cursor means touching a cache line that the other core keeps writing, which is
expensive. A producer whose `cached_head` still shows free room does not need to
look at `head` at all; it re-reads it only when the ring *looks* full. Likewise
the consumer re-reads `tail` only when the ring looks empty. In the common case
each side touches only its own cache line.

### Why those memory orderings? (plain-language version)

Two threads on different CPU cores do not automatically see each other's memory
writes in the order they happened. **Release/Acquire** pairs fix that:

> Everything a thread wrote **before** a `Release` store is guaranteed to be
> visible to another thread that **`Acquire`-loads the value that store wrote**.

```mermaid
sequenceDiagram
    participant P as Producer
    participant S as slot
    participant T as tail
    participant H as head
    participant C as Consumer
    P->>H: load head (Acquire), only if the ring looks full
    P->>S: write the value
    P->>T: store tail+1 (Release), publish
    C->>T: load tail (Acquire), only if the ring looks empty
    C->>S: read the value, guaranteed to see the write
    C->>H: store head+1 (Release), slot is reusable
```

| Access | Ordering | Reason |
|---|---|---|
| Producer reads **its own** `tail` | none: it is a plain field of `Producer` | Only the producer advances it, so its private copy is always exact. |
| Producer reads `head` (when the ring looks full) | `Acquire` | Pairs with the consumer's `Release` store: the producer must not overwrite a slot until the consumer has **finished reading** it. |
| Producer writes `tail` | `Release` | Publishes the slot contents written just before. |
| Consumer reads `tail` (when the ring looks empty) | `Acquire` | Pairs with the producer's `Release`: makes the slot contents visible. |
| Consumer writes `head` | `Release` | Hands the slot back only after the value was read out. |

## 8.4 False sharing and `CachePadded`

```rust
#[repr(align(128))]
struct CachePadded<T>(T);
```

CPUs move memory in **cache lines** (64 bytes on x86 and most ARM cores). If
`head` and `tail` lived in the same line, every time the producer updated `tail`
it would invalidate the line in the consumer's core, and vice versa, even though
they are different variables. That ping-pong is called **false sharing**.

Why align to 128 bytes rather than 64? Modern x86 cores have an *adjacent-line
prefetcher* that pulls cache lines in pairs, so two variables only 64 bytes apart
can still interfere; 128-byte alignment gives each cursor its own pair. (Some
CPUs, such as Apple's M-series, even use 128-byte cache lines.) The price is a
few hundred bytes per queue, which is nothing.

## 8.5 Safety: what the type system enforces

The slots live in `UnsafeCell`s, because two threads write and read different
slots through the one shared `Arc<Shared<T>>`:

```rust
unsafe impl<T: Send> Send for Shared<T> {}
unsafe impl<T: Send> Sync for Shared<T> {}
```

These `unsafe impl`s are the author promising the compiler that sharing is fine
**provided** there is only one writer and one reader. An earlier version of this
crate could not keep that promise: `push` and `pop` took `&self` on one shared
buffer, so nothing stopped two threads from pushing at once, which is a data
race and undefined behaviour. The current API makes that mistake impossible to
write:

- `spsc_channel` returns **one** `Producer` and **one** `Consumer`. Neither
  implements `Clone`, so you cannot make a second producer.
- `push` and `pop` take **`&mut self`**. Even if you wrap a producer in an `Arc`
  and share it, a shared reference (`&Producer`) is not enough to push, and the
  borrow checker allows only one `&mut` at a time.
- Each handle is `Send` (when `T` is), so you can **move** it to its own thread.

The module docs prove the first two points with `compile_fail` doctests:
examples that must *not* compile.

```rust
// Does not compile:
let (producer, _consumer) = spsc_channel::<u64>(4);
let second_producer = producer.clone();   // error[E0599]: no method named `clone`
```

```rust
// Does not compile either:
let (producer, _consumer) = spsc_channel::<u64>(4);
let shared = &producer;
shared.push(1).unwrap();                  // error[E0596]: cannot borrow as mutable
```

`cargo test` checks that each one still fails with exactly that error code. This
is a powerful pattern: encode a usage rule in the types, then test that breaking
the rule does not compile.

Other details:

- `T: Copy`, so a slot can be overwritten without running a destructor, and
  values still queued when the ring is dropped need no cleanup.
- `len()` and `is_empty()` are **snapshots**. On the producer, `len()` combines
  its exact `tail` with a fresh `head`, so the true count can only be *lower* by
  the time you look (the consumer keeps draining); on the consumer it can only be
  *higher*. Use them for metrics or back-pressure hints, and rely on what `push`
  and `pop` return for correctness.

### Telling "nothing yet" from "nothing ever again"

`pop` returning `None` only means "empty right now". If the producer thread has
finished, or has died with a panic, a consumer that keeps spinning would wait
forever. So each handle raises a flag when it is dropped (which also happens when
a panic unwinds its thread):

```rust
impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        // Release: every value pushed before this point is visible to a
        // consumer that reads the flag with Acquire.
        self.shared.producer_dropped.store(true, Ordering::Release);
    }
}

impl<T: Copy> Consumer<T> {
    pub fn is_disconnected(&self) -> bool {
        // Read the flag first. Its Acquire pairs with the Release in
        // `Drop for Producer`, so the tail loaded next is the final one.
        self.shared.producer_dropped.load(Ordering::Acquire)
            && self.shared.tail.0.load(Ordering::Acquire) == self.head
    }
}
```

`Consumer::is_disconnected()` is `true` only when the producer is gone **and**
every value it pushed has been popped, so acting on it can never lose data. The
order of the two loads matters. Had we read `tail` first, the producer could
push one last value and then drop between our two loads; we would see an empty
ring, then the flag, and wrongly report "disconnected" with a value still queued.
`Producer::is_disconnected()` is the mirror image: it reports that the consumer
was dropped, so anything pushed from now on would never be read.

The idiomatic consumer loop:

```rust
loop {
    match consumer.pop() {
        Some(value) => handle(value),
        None if consumer.is_disconnected() => break, // nothing will ever arrive
        None => std::hint::spin_loop(),              // nothing yet
    }
}
```

The threaded unit test uses the same check to **fail fast**: if its producer
stops early, the consumer's assertion fires instead of the test hanging until
CI's timeout.

---

# Part B: The write-ahead log (WAL)

## 8.6 Why log at all?

If the process crashes, the order book in memory is gone. How do we get it back?

A neat trick: the engine is **deterministic**. It has no clock and no randomness,
and its state is a pure function of the requests it has received (and the
capacity it was built with). So **the same sequence of requests always produces
exactly the same book**. Therefore we do not need to save the book; we only need to save the
**requests**, in order, and replay them into a fresh engine. This is called
*event sourcing* (here, *input* sourcing).

```text
   requests:   new(1) new(2) cancel(1) new(3) ...      ---> append to disk, in order
                                                              |
   crash! book lost                                           v
   restart:    Engine::new()  <----------  read the log, feed each request back in
```

"Write-ahead" means: **log the request first, then apply it.** If we crash right
after logging, replay re-applies the request; if we had applied it first and
crashed before logging, we would lose it.

## 8.7 The record format

Every request is one fixed-size, 40-byte record:

```rust
#[repr(C)]
pub struct WalRecord {
    pub kind: u8,          // 1 = NewOrder, 2 = Cancel   (0 is never valid)
    pub side: u8,          // 0 = Buy, 1 = Sell          (new orders only)
    pub _padding: [u8; 6], // keeps order_id aligned to 8 bytes
    pub order_id: u64,
    pub price_tick: u32,   // new orders only
    pub qty: u64,          // new orders only
    pub sequence: u64,     // caller-supplied sequence number
}
```

Measured byte layout (`offset_of!`):

```text
 byte   0    1    2              8              16        20      24             32             40
        +----+----+--------------+--------------+---------+-------+--------------+--------------+
        |kind|side|  _padding    |   order_id   |  tick   | (pad) |     qty      |   sequence   |
        | u8 | u8 |   6 bytes    |     u64      |   u32   | 4 B   |     u64      |     u64      |
        +----+----+--------------+--------------+---------+-------+--------------+--------------+
        WAL_RECORD_SIZE = 40
```

(40 bytes is the size on typical 64-bit targets. The constant is computed with
`size_of::<WalRecord>()`, so on a 32-bit target where `u64` is 4-byte aligned it
comes out smaller, another reason a log is not portable between machines.)

> [!WARNING]
> **The 4 bytes marked `(pad)` (bytes 20 to 23) are never written by any
> constructor.** Unlike `_padding`, which is an explicit, zeroed field, they are
> *implicit* padding that the compiler inserts to align `qty`. `to_bytes` copies
> the whole struct with `transmute_copy`, so whatever happens to be in that
> padding is written to the log. Two consequences: the log bytes for identical
> requests can differ, and strictly speaking reading uninitialised padding is
> undefined behaviour (the "sound" comment in the source is not quite right). It
> is harmless in practice today, but it would break any checksum over raw record
> bytes (exercise 16). Making the padding an explicit zeroed field fixes it.

Fixed-size records give two big wins: **appending needs no serialization step**
(just copy 40 bytes) and **record *i* is at byte `i × 40`**, so you can seek
straight to it.

Constructors: `WalRecord::new_order(id, side, tick, qty, sequence)` and
`WalRecord::cancel(id, sequence)` (the unused fields are zeroed).

## 8.8 Memory-mapped files in one minute

Normally, writing to a file means a system call (`write`) per record. A
**memory-mapped file** asks the operating system to make the file's bytes appear
as ordinary memory in your process. Writing to that memory is then a plain
`memcpy`; the OS writes the dirty pages to disk later.

```text
   your process memory                 the OS                    disk
  +---------------------+        +---------------+         +-----------+
  | &mut [u8] (the map) | <----> |  page cache   | ------> | orders.wal|
  +---------------------+  memcpy +---------------+  later  +-----------+
```

This crate uses the `memmap2` crate for it. Appending a record is therefore just:

```rust
self.mmap[self.next_offset..self.next_offset + WAL_RECORD_SIZE].copy_from_slice(&bytes);
self.next_offset += WAL_RECORD_SIZE;
```

## 8.9 The writer's lifecycle

```mermaid
flowchart TD
    A["WalWriter::create(path, capacity_records)"] --> B["create or truncate file<br/>set_len(capacity × 40)<br/>map it into memory"]
    B --> C["append(record)"]
    C --> D{"next_offset + 40<br/>> capacity_bytes?"}
    D -- no --> E["copy 40 bytes into the map<br/>next_offset += 40"]
    D -- yes --> F["grow(): double the file size<br/>and map it again"]
    F --> E
    E --> C
    E --> G["flush(): write dirty pages to disk<br/>(blocking)"]
    G --> H["finalize(): flush, unmap, then truncate the file<br/>to next_offset (drop the unused tail)"]
    H --> I["Drop calls finalize() best-effort"]
```

```text
   file right after create(path, 4):    [ zero ][ zero ][ zero ][ zero ]     capacity = 4 records
   after 3 appends:                      [ rec0 ][ rec1 ][ rec2 ][ zero ]     next_offset = 120
   after the 5th append (grow):          [ rec0..rec4 ][ zero ][ zero ][ zero ]   capacity = 8 records
   after finalize():                     [ rec0..rec4 ]                       next_offset = 200
```

- **Pre-sizing** (`create`) means the common case makes no system call on append
  (the first write to each new page of the map can still cause a page fault).
- **Growth** doubles the file and remaps it. That is a syscall-heavy slow path, a
  latency spike, so pick `capacity_records` large enough for your expected volume.
- `flush()` is a **blocking** durability barrier, not a hint. In the `memmap2`
  version this crate uses it calls `msync(MS_SYNC)` on Unix, and
  `FlushViewOfFile` followed by `FlushFileBuffers` on Windows, and returns when
  the OS reports the data written. That is far too slow to call after every
  record; how often to call it (per batch, per second...) is your durability
  policy. (`memmap2` also has a non-blocking `flush_async`, but `WalWriter` does
  not expose it.)
- `finalize()` flushes, **unmaps the file, and then truncates it to the bytes
  actually written**, so the file contains only real records. The order matters:
  Windows refuses to shrink a file that still has a mapped view open (OS error
  1224), so the writer holds its map as an `Option<MmapMut>` and drops it before
  calling `set_len`. `finalize` ends the writer's life: a later `append` or
  `flush` returns an `io::Error` rather than re-mapping the file. Calling
  `finalize` again is harmless.

## 8.10 Reading and replaying

```rust
pub fn read_all<P: AsRef<Path>>(path: P) -> io::Result<Vec<WalRecord>>
```

maps the file, cuts it into 40-byte pieces, and decodes each by copying the bytes
into a `WalRecord`. A partial trailing record is ignored. **It does not validate
anything**: `kind` could be 0 or 99, `side` could be 7.

Because the file is pre-sized and may end with zero padding (see the caveats),
a replay loop should **stop at the first record with `kind == 0`**:

```rust
const NEW_ORDER: u8 = WalRecordKind::NewOrder as u8;
const CANCEL: u8 = WalRecordKind::Cancel as u8;

for rec in read_all(&path)? {
    match rec.kind {
        NEW_ORDER => {
            let side = if rec.side == 0 { Side::Buy } else { Side::Sell };
            engine.process_order(rec.order_id, side, rec.price_tick, rec.qty);
        }
        CANCEL => { engine.cancel_order(rec.order_id); }
        _ => break,   // kind 0 = unwritten padding; anything else = corrupt, stop
    }
}
```

## 8.11 Caveats (read before relying on the WAL)

The module docs call the format "compact and implementation-specific". Concretely:

| Caveat | Consequence |
|---|---|
| Raw `repr(C)` bytes, **native endianness** | A log written on one architecture may not replay on another. |
| **No header, version or checksum** | A torn or corrupted record is indistinguishable from a good one; you cannot detect format changes. |
| File is **pre-sized with zeros** | After a crash (when `finalize` never ran) the file ends in zero records. Replay must stop at `kind == 0`. |
| `flush()` is blocking and you must schedule it | Nothing flushes automatically except `finalize`/`Drop`. If only your *process* crashes, everything already copied into the map survives (the OS owns those pages). A power loss or OS crash loses whatever was not flushed. You decide when a request counts as "durable". |
| **`WalWriter::create` truncates an existing file** | There is no open-for-append API. After a crash, calling `create` on the old path **erases the log you meant to replay**. Replay with `read_all` first, then write a new log. |
| `append` / `flush` after `finalize` fail | `finalize` unmaps the file, so the writer is closed: `append` and `flush` return an `io::Error` ("WAL writer is already finalized") instead of re-mapping. Treat `finalize` as the last call; `len()` and `is_empty()` still work. |
| Implicit padding bytes are uninitialised | See the warning in 8.7. |
| Nothing forces "log first, then apply" | The ordering is the caller's responsibility (the example below does it right). |

> [!NOTE]
> **Why `finalize()` unmaps before truncating.** Windows refuses to shrink a file
> that still has a mapped view open (OS error 1224, "The requested operation
> cannot be performed on a file with a user-mapped section open"). An earlier
> version called `set_len` while the map was alive; `Drop` ignored the resulting
> error (`let _ = self.finalize()`), so on Windows the file silently kept its zero
> padding and `read_all` returned extra all-zero records (for example 16 instead
> of 10 after 10 appends to a WAL created with capacity 4). `finalize` now drops
> the map first, so the file is truncated on every platform and the padding only
> remains after a crash, when `finalize` never ran.
>
> That is why the zero-stopping replay loop in 8.10 stays: it is what makes
> recovery correct after a crash. Call `finalize()` yourself if you need to
> observe a truncation error; `Drop` still swallows it.

---

# Part C: Putting it all together

This complete program ties the three components into one pipeline: a producer
thread, the ring buffer, the WAL, the engine, and finally a replay that rebuilds
an identical book from the log. (It was compiled and run against this crate.)

```rust
use std::sync::Arc;
use std::thread;

use matching_engine::ringbuffer::SpscRingBuffer;
use matching_engine::wal::{read_all, WalRecord, WalRecordKind, WalWriter};
use matching_engine::{Engine, Side};

/// What the gateway thread sends to the matching thread.
#[derive(Clone, Copy)]
enum Command {
    New { id: u64, side: Side, tick: u32, qty: u64 },
    Cancel { id: u64 },
}

fn main() -> std::io::Result<()> {
    const N: u64 = 10_000;
    let queue = Arc::new(SpscRingBuffer::<Command>::new(1024));

    // Producer: pretend to be a network gateway.
    let producer = {
        let queue = Arc::clone(&queue);
        thread::spawn(move || {
            for id in 1..=N {
                let mut cmd = if id % 5 == 0 {
                    Command::Cancel { id: id - 1 }
                } else {
                    let side = if id % 2 == 0 { Side::Buy } else { Side::Sell };
                    Command::New { id, side, tick: 1000 + (id % 7) as u32, qty: 1 + id % 3 }
                };
                // push hands the value back when the queue is full: retry.
                while let Err(back) = queue.push(cmd) {
                    cmd = back;
                    std::hint::spin_loop();
                }
            }
        })
    };

    // Consumer: the single matching thread.
    let path = std::env::temp_dir().join("orders.wal");
    let mut wal = WalWriter::create(&path, 16_384)?;
    let mut engine = Engine::new(50_000);
    for seq in 0..N {
        let cmd = loop {
            match queue.pop() {
                Some(cmd) => break cmd,
                None => std::hint::spin_loop(),
            }
        };
        // 1. Write-ahead: log the request BEFORE applying it.
        wal.append(match cmd {
            Command::New { id, side, tick, qty } => WalRecord::new_order(id, side, tick, qty, seq),
            Command::Cancel { id } => WalRecord::cancel(id, seq),
        })?;
        // 2. Apply it to the book.
        let _events = match cmd {
            Command::New { id, side, tick, qty } => engine.process_order(id, side, tick, qty),
            Command::Cancel { id } => engine.cancel_order(id).1,
        };
        // 3. A real system would publish `_events` to downstream consumers here.
    }
    producer.join().unwrap();
    wal.flush()?;

    // Recovery: rebuild an identical engine from the log alone.
    const NEW_ORDER: u8 = WalRecordKind::NewOrder as u8;
    const CANCEL: u8 = WalRecordKind::Cancel as u8;
    let mut replica = Engine::new(50_000);
    for rec in read_all(&path)? {
        match rec.kind {
            NEW_ORDER => {
                let side = if rec.side == 0 { Side::Buy } else { Side::Sell };
                replica.process_order(rec.order_id, side, rec.price_tick, rec.qty);
            }
            CANCEL => {
                replica.cancel_order(rec.order_id);
            }
            _ => break, // zero padding after the last real record
        }
    }

    assert_eq!(engine.live_order_count(), replica.live_order_count());
    assert_eq!(engine.depth_snapshot(Side::Buy, 100), replica.depth_snapshot(Side::Buy, 100));
    assert_eq!(engine.depth_snapshot(Side::Sell, 100), replica.depth_snapshot(Side::Sell, 100));
    println!("replayed book is identical: {} resting orders", replica.live_order_count());
    Ok(())
}
```

What it demonstrates:

- **Decoupling:** the producer never touches the engine; it only pushes commands.
- **Ordering:** the WAL append happens *before* the engine call.
- **Determinism:** a fresh engine fed the logged requests ends in exactly the same
  state, which is what makes the WAL useful for recovery, testing, and audits.

**Next:** [9. Testing and performance](09-testing-and-performance.md)
