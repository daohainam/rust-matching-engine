# Glossary

Every technical term the chapters use, explained in a sentence or two. Terms are
grouped by topic and sorted alphabetically inside each group. The **Ch.** column
says where the term is first explained, or used most.

Jump to: [Trading](#trading) · [Data structures and algorithms](#data-structures-and-algorithms) ·
[Hardware and performance](#hardware-and-performance) · [Rust](#rust) ·
[Concurrency](#concurrency) · [Files and durability](#files-and-durability) ·
[Testing and tools](#testing-and-tools)

## Trading

| Term | Meaning | Ch. |
|---|---|---|
| Account | Who placed an order: a trader, a firm, or whatever unit the caller tracks (`NewOrder::account`). Self-trade prevention and mass cancels use it. | [1](01-order-book-basics.md) |
| Aggressor / aggressor side | The taker of a trade, and its side. A trade is *buyer-initiated* when a buyer crossed the spread. `Trade` reports it as `taker_side`. | [1](01-order-book-basics.md) |
| Ask / offer | A resting order to **sell**. The best ask is the lowest one. | [1](01-order-book-basics.md) |
| Bid | A resting order to **buy**. The best bid is the highest one. | [1](01-order-book-basics.md) |
| Book (order book) | The list of all resting orders, split into bids and asks and sorted by price. | [1](01-order-book-basics.md) |
| Circuit breaker | An exchange rule that pauses trading when prices move too far, too fast. Not implemented here. | [1](01-order-book-basics.md) |
| Counterparty | The other side of a trade: the seller for a buyer, and the buyer for a seller. | [1](01-order-book-basics.md) |
| Crossing the spread | An incoming order whose price reaches the best price on the other side, so it can trade at once. Such an order is *marketable*. | [1](01-order-book-basics.md) |
| Depth | Total quantity resting at one price level, or across several levels. | [1](01-order-book-basics.md) |
| Exchange | A venue where buyers and sellers trade. Its core is the matching engine. | [1](01-order-book-basics.md) |
| Execution report | A message telling a trader what happened to their order. Here, the engine's [events](07-events.md) play this role. | [7](07-events.md) |
| Fill / partial fill | An execution of some or all of an order's quantity. After a *partial* fill, some quantity is left. | [1](01-order-book-basics.md) |
| FIX | *Financial Information eXchange*, a common text protocol traders use to send orders to exchanges. A gateway would parse it. | [2](02-architecture.md) |
| FOK (fill-or-kill) | An order that must fill completely at once, or not at all. Not implemented here (exercise in ch. 11). | [1](01-order-book-basics.md) |
| Gateway | The part of an exchange that receives orders from the network, checks them, and passes them on to the matching engine. | [2](02-architecture.md) |
| Instrument | The thing being traded: one stock, one future, one currency pair. Each instrument has its own book. | [1](01-order-book-basics.md) |
| IOC (immediate-or-cancel) | An order that trades what it can at once; any remainder expires instead of resting (`OrderType::ImmediateOrCancel`, reported with `OrderExpired`). | [1](01-order-book-basics.md) |
| Limit order | An order with a worst acceptable price: "buy at this price or lower", "sell at this price or higher". Its remainder rests in the book. | [1](01-order-book-basics.md) |
| Liquidity | How much can be traded without moving the price much. Resting orders *provide* liquidity; takers *remove* it. | [1](01-order-book-basics.md) |
| Maker | The resting order in a trade. It was in the book first. | [1](01-order-book-basics.md) |
| Market data | The public feed of trades and book changes that an exchange publishes. | [2](02-architecture.md) |
| Mass cancel | Cancelling every resting order of one account (for example when its connection drops: *cancel on disconnect*), or every order in the book (`cancel_account`, `cancel_all`). | [1](01-order-book-basics.md) |
| Market order | An order with no price of its own: it trades at whatever prices the book offers, and its remainder expires. Real exchanges limit how far it may sweep with a price band, and so does this engine when one is set. | [1](01-order-book-basics.md) |
| Matching engine | The component that pairs buy and sell orders and decides the trade price. This whole repository. | [1](01-order-book-basics.md) |
| Pre-trade risk check / fat finger | A check an order must pass before it may trade, against mistakes such as a mistyped quantity or price (a *fat finger*). Here: a maximum order quantity and a price band (`RiskLimits`). | [1](01-order-book-basics.md) |
| Post-only | A limit order that must not trade on arrival: if its price reaches the other side, it is rejected instead. Market makers use it to be sure they only add liquidity. | [1](01-order-book-basics.md) |
| Price level | All orders resting at one price on one side, kept as a FIFO queue. | [1](01-order-book-basics.md) |
| Price band | How far past the best opposing price an order may reach; further is rejected, and a market order stops at the edge. | [1](01-order-book-basics.md) |
| Price-time priority | Matching rule: the better price trades first; at the same price, the order that arrived earlier trades first. | [1](01-order-book-basics.md) |
| Resting order | An order that is waiting in the book because it could not trade (fully) when it arrived. | [1](01-order-book-basics.md) |
| Self-trade prevention | A rule that stops a trader's buy from matching their own sell. The incoming order chooses what happens instead: cancel the resting order, cancel the incoming one, or both (`SelfTradePrevention`). | [1](01-order-book-basics.md) |
| Spread | Best ask minus best bid. | [1](01-order-book-basics.md) |
| Sweep | One taker trading through several price levels in a row. | [6](06-matching-algorithm.md) |
| Taker | The incoming order in a trade. It "takes" liquidity that a maker left in the book. | [1](01-order-book-basics.md) |
| Tick | The smallest price step. This engine works only with integer ticks, `0..=4095` by default and up to 2^24 per engine (`Engine::with_ticks`); converting to real prices is the caller's job. | [1](01-order-book-basics.md) |
| Wash trade | A trade between two orders of the same owner. It moves nothing, but looks like real activity to the market; done on purpose it is illegal manipulation. See self-trade prevention. | [1](01-order-book-basics.md) |

## Data structures and algorithms

| Term | Meaning | Ch. |
|---|---|---|
| Amortised cost | The average cost per operation over a long sequence, even if a few single operations are slower (for example, when the WAL file grows). | [9](09-write-ahead-log.md) |
| Arena | One large array, allocated up front, that holds every object of a kind. Objects are named by their index instead of a pointer. Here: the order pool. | [3](03-order-pool.md) |
| Avalanche | A property of a good hash function: changing one input bit changes about half of the output bits. | [5](05-hash-map.md) |
| Backward-shift deletion | A way to delete from an open-addressing table without tombstones: entries after the hole move back to fill it, if that keeps them reachable. | [5](05-hash-map.md) |
| Big-O (O(1), O(n)) | How the work grows with input size. O(1) means "the same amount of work no matter how big the book is"; O(n) means "proportional to n". | [1](01-order-book-basics.md) |
| Bit-scan | A CPU instruction that finds the lowest or highest set bit in a word. Rust exposes it as `trailing_zeros` / `leading_zeros`. | [4](04-price-ladder-bitset.md) |
| Bitset | An array of bits, one per item, recording yes/no for each. Here: one bit per tick, "is anything resting at this price?", with summary levels on top (a *hierarchical* bitset). | [4](04-price-ladder-bitset.md) |
| Deterministic | Same inputs, same outputs, every time. No clocks, randomness or thread timing can change a result. | [2](02-architecture.md) |
| FIFO | *First in, first out*: a queue where the oldest item leaves first. Each price level is a FIFO queue. | [3](03-order-pool.md) |
| Free list / free stack | A list of the arena slots not currently in use. Here it is a stack, so the most recently freed slot is reused first. | [3](03-order-pool.md) |
| Hash function | A function that turns a key (an order id) into a number used to pick a table slot. | [5](05-hash-map.md) |
| Intrusive linked list | A linked list whose `next` / `prev` links are stored inside the items themselves, so no separate node has to be allocated. | [3](03-order-pool.md) |
| Invariant | A fact that must be true at certain points, for example "a tick's bit is set exactly when its level is not empty". | [6](06-matching-algorithm.md) |
| Linear probing | Collision rule for open addressing: if a slot is taken, try the next one, then the next. | [5](05-hash-map.md) |
| Load factor | How full a hash table is: entries divided by slots. Probing gets slower as it approaches 1. | [5](05-hash-map.md) |
| Open addressing | A hash table that stores entries directly in its slot array, instead of in a linked list per slot. | [5](05-hash-map.md) |
| Probe / probe chain | Checking one slot during a lookup / the run of slots a lookup has to check. | [5](05-hash-map.md) |
| Seed | A secret random number mixed into the hash, so an outsider cannot predict which ids collide. | [5](05-hash-map.md) |
| Sentinel | A special value that means "nothing here", such as `NIL = u32::MAX` for "no next order" or `u64::MAX` for "empty hash slot". | [3](03-order-pool.md) |
| SplitMix64 | A small, fast function that scrambles a 64-bit number well. Used as the hash function and in the WAL checksum. | [5](05-hash-map.md) |
| Tombstone | A marker left in a deleted hash slot so lookups keep probing past it. This map avoids them with backward-shift deletion. | [5](05-hash-map.md) |

## Hardware and performance

| Term | Meaning | Ch. |
|---|---|---|
| Alignment / padding | Most types must start at an address that is a multiple of their size (their *alignment*). The compiler inserts unused *padding* bytes to make that true. | [3](03-order-pool.md) |
| Benchmark | A program that measures how fast some code runs. | [10](10-testing-and-performance.md) |
| Branch prediction | The CPU guesses which way an `if` will go so it can keep working; a wrong guess costs time. Predictable branches are almost free. | [6](06-matching-algorithm.md) |
| Cache / cache line | Small, fast memory inside the CPU. Data moves between memory and cache in fixed blocks called *cache lines*, usually 64 bytes. | [3](03-order-pool.md) |
| False sharing | Two threads writing to different variables that happen to share one cache line, so the line bounces between CPU cores. Fixed by padding (`CachePadded`). | [8](08-ring-buffer.md) |
| Heap allocation | Asking the memory allocator (`malloc`, `Box::new`, a growing `Vec`) for memory at run time. Slow and unpredictable, so the hot path avoids it. | [2](02-architecture.md) |
| Hot path | The code that runs for every order. It must be the fastest part. | [2](02-architecture.md) |
| Latency / tail latency | Time to handle one request / the time of the slowest requests (for example the slowest 1 in 100). Exchanges care most about the tail. | [8](08-ring-buffer.md) |
| Prefetcher | CPU hardware that notices you reading memory in order and loads the next cache lines early. Flat arrays benefit from it. | [3](03-order-pool.md) |
| Throughput | How many requests are handled per second. | [10](10-testing-and-performance.md) |

## Rust

| Term | Meaning | Ch. |
|---|---|---|
| Associated function | A function inside an `impl` block that does not take `self`, called as `Type::function(...)`. | [6](06-matching-algorithm.md) |
| Borrow checker | The part of the compiler that enforces Rust's rule: either one `&mut` reference or any number of `&` references to a value at a time. | [3](03-order-pool.md) |
| `dyn Trait` | A value used through a trait at run time, without the compiler knowing its concrete type. Each call goes through a small table of function pointers. | [6](06-matching-algorithm.md) |
| Generics / monomorphisation | `fn f<S: EventSink>(...)` works for any `S`. The compiler *monomorphises* it: it builds one copy of `f` per concrete type, so calls are direct and can be inlined. | [6](06-matching-algorithm.md) |
| `#[inline]` | A hint to copy a function's body into its callers, removing the call overhead. | [6](06-matching-algorithm.md) |
| `MaybeUninit<T>` | Memory that may not hold a valid `T` yet. Reading it as a `T` before writing it is undefined behaviour. | [8](08-ring-buffer.md) |
| Panic / `panic = "abort"` | A panic stops the current thread because of a bug (for example a failed `assert!`). With `panic = "abort"` in the final binary's profile, it ends the whole process instead. | [3](03-order-pool.md), [10](10-testing-and-performance.md) |
| `#[repr(C)]` | Lays out a struct's fields in declaration order, like C does. Without it, Rust may reorder fields to save space. | [3](03-order-pool.md) |
| `Send` / `Sync` | Marker traits. `Send`: a value may be moved to another thread. `Sync`: a value may be *shared* (`&T`) between threads. | [8](08-ring-buffer.md) |
| `?Sized` | Allows a generic type parameter to be a type whose size is unknown at compile time, such as `dyn EventSink`. | [6](06-matching-algorithm.md) |
| Trait | A set of methods a type promises to provide, like an interface. `EventSink` is a trait. | [7](07-events.md) |
| Undefined behaviour (UB) | Breaking a rule the compiler relies on (for example reading uninitialised memory). The program may then do *anything*; it is always a bug. | [8](08-ring-buffer.md) |
| `unsafe` | A block where the programmer, not the compiler, promises that the rules are followed. Every `unsafe` block here has a `// SAFETY:` comment saying why it is sound. | [8](08-ring-buffer.md) |
| `UnsafeCell<T>` | The only legal way in Rust to change data behind a shared `&` reference. Every safe interior-mutability type is built on it. | [8](08-ring-buffer.md) |

## Concurrency

| Term | Meaning | Ch. |
|---|---|---|
| Atomic (`AtomicUsize`, `AtomicBool`) | A variable that several threads can read and write safely without a lock; each operation happens all at once or not at all. | [8](08-ring-buffer.md) |
| Compare-and-swap (CAS) | An atomic "change the value to B only if it is still A" operation, needed when several threads write the same variable. The SPSC queue does not need it. | [8](08-ring-buffer.md) |
| Data race | Two threads touching the same memory at the same time, at least one writing, without synchronisation. Undefined behaviour in Rust. | [8](08-ring-buffer.md) |
| Disruptor (LMAX) | A well-known design from the LMAX exchange: a pre-allocated ring buffer feeding one busy thread that owns all state. This repo follows its spirit. | [2](02-architecture.md) |
| Lock-free | Threads coordinate with atomics instead of locks (mutexes), so no thread ever waits for another to release a lock. | [8](08-ring-buffer.md) |
| Memory ordering (`Relaxed`, `Release`, `Acquire`) | Rules for when one thread's writes become visible to another. A `Release` store paired with an `Acquire` load guarantees that everything written before the store is visible after the load. `Relaxed` gives no such guarantee. | [8](08-ring-buffer.md) |
| Ring buffer | A fixed-size array used as a queue, whose read and write positions wrap around to the start. | [8](08-ring-buffer.md) |
| Spin loop | Waiting by checking a condition again and again instead of sleeping. Fast to react, but keeps one CPU core busy. | [8](08-ring-buffer.md) |
| SPSC | *Single producer, single consumer*: a queue with exactly one writing thread and one reading thread. That restriction is what makes it simple and fast. | [8](08-ring-buffer.md) |
| Thread | An independent flow of execution inside a program. Here, the gateway and the matching engine run on separate threads. | [2](02-architecture.md) |

## Files and durability

| Term | Meaning | Ch. |
|---|---|---|
| Atomic rename | Replacing a file in one step by writing a new one under a temporary name and renaming it over the old; a crash leaves the old file or the new one, never a mix. Checkpoint files are written this way. | [9](09-write-ahead-log.md#98-checkpoints-replaying-less) |
| Checkpoint | A snapshot saved to a file, together with the log position it reaches, so recovery replays only the requests logged after it. | [9](09-write-ahead-log.md#98-checkpoints-replaying-less) |
| Checksum | A small number computed from some bytes and stored with them. If the bytes change, the checksum (very likely) no longer matches. | [9](09-write-ahead-log.md) |
| CRC / CRC32 | A standard checksum family (used by zip files and Ethernet). This repo uses a SplitMix64-based checksum instead, to avoid a dependency. | [9](09-write-ahead-log.md) |
| Dirty page | A page of a file that was changed in memory but not yet written to the disk. | [9](09-write-ahead-log.md) |
| Durability | The promise that data survives a crash or power loss once it is reported as saved. | [9](09-write-ahead-log.md) |
| Endianness / little-endian | The byte order of a multi-byte number in memory. *Little-endian* stores the lowest byte first. The WAL always uses little-endian, so files work on any machine. | [9](09-write-ahead-log.md) |
| `fsync` / `sync_all` / `msync` | Calls that force the operating system to write cached data to the disk. Until one of them succeeds, a power loss can lose the data. | [9](09-write-ahead-log.md) |
| File lock (advisory / mandatory) | A claim on a file that other programs can check. An *advisory* lock only stops those that ask for it too (Unix); a *mandatory* lock also blocks their reads and writes (Windows). The WAL writer locks a companion `.lock` file so that readers are never blocked. | [9](09-write-ahead-log.md) |
| Header / magic number | The first bytes of a file, describing its format. The *magic number* is a fixed value that identifies the file type. | [9](09-write-ahead-log.md) |
| MAC | *Message authentication code*: a checksum that needs a secret key, so an attacker cannot forge it. The WAL checksum is **not** a MAC. | [9](09-write-ahead-log.md) |
| Memory-mapped file (mmap) | A file made to look like an array in memory. Writing to the array writes to the file, through the page cache. | [9](09-write-ahead-log.md) |
| Page / page cache / page fault | The OS handles memory in *pages* (often 4 KiB). The *page cache* keeps recently used file pages in RAM. A *page fault* happens when a program touches a page that is not in RAM yet. | [9](09-write-ahead-log.md) |
| Replay | Rebuilding state by feeding the logged requests into a fresh engine again. Works because the engine is deterministic. | [9](09-write-ahead-log.md) |
| Snapshot | A copy of an engine's complete state (capacity, counters, resting orders in priority order) that can rebuild an identical engine. Not to be confused with `depth_snapshot`, which only lists price levels. | [9](09-write-ahead-log.md#98-checkpoints-replaying-less) |
| `SIGBUS` | A Unix signal that kills a process that touches a memory-mapped page the system cannot back with real storage (for example, the disk is full, or the file was shrunk by someone else). | [9](09-write-ahead-log.md) |
| Sparse file | A file whose unwritten parts take no disk space; the OS reads them as zeros. | [9](09-write-ahead-log.md) |
| Syscall | A request from a program to the operating system kernel (open a file, write to it, ...). Much slower than a normal function call. | [9](09-write-ahead-log.md) |
| Torn write / torn record | A write that a crash interrupted halfway, leaving a record that is part new and part old (or zero). The checksum detects it. | [9](09-write-ahead-log.md) |
| Write-ahead log (WAL) | A file where every request is recorded **before** it is applied, so the state can be rebuilt after a crash by replaying it. | [9](09-write-ahead-log.md) |

## Testing and tools

| Term | Meaning | Ch. |
|---|---|---|
| CI | *Continuous integration*: a server that builds and tests every change automatically. Here: GitHub Actions, see `.github/workflows/ci.yml`. | [10](10-testing-and-performance.md) |
| Counting allocator | A global allocator that counts every allocation, used in a test to prove the hot path allocates nothing. | [10](10-testing-and-performance.md) |
| Criterion | The Rust benchmarking library used by `cargo bench`. | [10](10-testing-and-performance.md) |
| Differential testing | Running the same random inputs through the real code and a simple *reference implementation*, and requiring identical results. | [10](10-testing-and-performance.md) |
| Doctest | A code example inside documentation that `cargo test` compiles and runs, so it cannot go out of date. | [10](10-testing-and-performance.md) |
| Global allocator | The memory allocator a Rust program uses for all heap memory. A program can replace it with `#[global_allocator]`. | [10](10-testing-and-performance.md) |
| Integration test | A test in `tests/` that uses the crate only through its public API, like a real user would. | [10](10-testing-and-performance.md) |
| loom | A tool that runs a concurrent test under every possible thread interleaving, to find ordering bugs. Suggested as an exercise. | [10](10-testing-and-performance.md) |
| LTO | *Link-time optimisation*: the compiler optimises across crate boundaries when building the final binary. | [10](10-testing-and-performance.md) |
| Miri | An interpreter for Rust that detects undefined behaviour in `unsafe` code. CI runs the unit tests under it. | [10](10-testing-and-performance.md) |
| MSRV | *Minimum supported Rust version*: the oldest compiler the crate promises to build with (here 1.89, for `File::try_lock`). | [10](10-testing-and-performance.md) |
| Unit test | A small test next to the code it tests (`#[cfg(test)] mod tests`), which can see private items. | [10](10-testing-and-performance.md) |

**Back to:** [README](../README.md) · [1. Order book basics](01-order-book-basics.md)
