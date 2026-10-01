//! A lock-free Single-Producer Single-Consumer ring buffer, inspired by the
//! LMAX Disruptor pattern, used as the input queue feeding the matching
//! engine's single-threaded hot path.
//!
//! [`spsc_channel`] returns one [`Producer`] and one [`Consumer`]. Neither
//! handle is `Clone`, and `push`/`pop` take `&mut self`, so the type system
//! guarantees a single writer and a single reader; each handle can be moved
//! to its own thread.
//!
//! Capacity is fixed at construction (rounded up to a power of two) and the
//! backing storage is allocated once up front; `push`/`pop` never allocate.
//! The shared cursors live on separate cache lines, and each handle caches
//! the other side's cursor so the fast path touches only its own line.
//!
//! Dropping a handle disconnects the other side.
//! [`Consumer::is_disconnected`] tells "nothing yet" from "nothing ever
//! again", and [`Producer::is_disconnected`] reports a consumer that is gone.
//! So when the producer thread finishes, or dies with a panic, the consumer
//! can stop instead of spinning forever.
//!
//! ```
//! use matching_engine::ringbuffer::spsc_channel;
//!
//! let (mut producer, mut consumer) = spsc_channel::<u64>(1024);
//! let feeder = std::thread::spawn(move || {
//!     for i in 0..10 {
//!         while producer.push(i).is_err() {
//!             std::hint::spin_loop();
//!         }
//!     }
//!     // `producer` is dropped here, which disconnects the consumer.
//! });
//! let mut received = Vec::new();
//! loop {
//!     match consumer.pop() {
//!         Some(value) => received.push(value),
//!         None if consumer.is_disconnected() => break, // nothing will ever arrive
//!         None => std::hint::spin_loop(),
//!     }
//! }
//! feeder.join().unwrap();
//! assert_eq!(received, (0..10).collect::<Vec<_>>());
//! ```
//!
//! A second producer cannot be created by cloning a handle...
//!
//! ```compile_fail,E0599
//! use matching_engine::ringbuffer::spsc_channel;
//!
//! let (producer, _consumer) = spsc_channel::<u64>(4);
//! let second_producer = producer.clone();
//! ```
//!
//! ...and a shared reference is not enough to push, so wrapping a handle in
//! an `Arc` does not create one either.
//!
//! ```compile_fail,E0596
//! use matching_engine::ringbuffer::spsc_channel;
//!
//! let (producer, _consumer) = spsc_channel::<u64>(4);
//! let shared = &producer;
//! shared.push(1).unwrap();
//! ```

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Aligns a value to 128 bytes: x86 prefetchers pull cache lines in adjacent
/// pairs, so 64-byte padding alone can still let the two cursors interfere.
#[repr(align(128))]
struct CachePadded<T>(T);

/// Storage and cursors shared by one [`Producer`] and one [`Consumer`].
struct Shared<T> {
    /// Written by the producer and read by the consumer; the cursors decide
    /// which side owns each slot at any moment.
    buffer: Box<[UnsafeCell<MaybeUninit<T>>]>,
    mask: usize,
    /// Consumer cursor: the number of values popped so far.
    head: CachePadded<AtomicUsize>,
    /// Producer cursor: the number of values pushed so far.
    tail: CachePadded<AtomicUsize>,
    /// Set (with `Release`) when the [`Producer`] is dropped. Written once and
    /// read only by `is_disconnected`, so it needs no cache line of its own.
    producer_dropped: AtomicBool,
    /// Set (with `Release`) when the [`Consumer`] is dropped.
    consumer_dropped: AtomicBool,
}

// SAFETY: slots are only accessed through the unique `Producer` (writes to
// free slots) and the unique `Consumer` (reads of published slots), and each
// slot changes hands through a Release store / Acquire load of a cursor.
// Values cross threads, so `T` must be `Send`.
unsafe impl<T: Send> Send for Shared<T> {}
unsafe impl<T: Send> Sync for Shared<T> {}

/// Creates a ring buffer and returns its producer and consumer ends.
///
/// `capacity` is rounded up to the next power of two, which lets each
/// monotonically increasing cursor map to a physical slot with a bitmask.
///
/// # Panics
/// Panics if `capacity` is zero or its next power of two overflows `usize`.
pub fn spsc_channel<T: Copy + Send>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    assert!(capacity > 0, "ring buffer capacity must be positive");
    let capacity = capacity
        .checked_next_power_of_two()
        .expect("ring buffer capacity overflows usize");
    let shared = Arc::new(Shared {
        buffer: (0..capacity)
            .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
            .collect(),
        mask: capacity - 1,
        head: CachePadded(AtomicUsize::new(0)),
        tail: CachePadded(AtomicUsize::new(0)),
        producer_dropped: AtomicBool::new(false),
        consumer_dropped: AtomicBool::new(false),
    });
    let producer = Producer {
        shared: Arc::clone(&shared),
        tail: 0,
        cached_head: 0,
    };
    let consumer = Consumer {
        shared,
        head: 0,
        cached_tail: 0,
    };
    (producer, consumer)
}

/// The writing end of an SPSC ring buffer. There is exactly one per buffer.
pub struct Producer<T> {
    shared: Arc<Shared<T>>,
    /// This handle's copy of the producer cursor; only it advances the cursor.
    tail: usize,
    /// Last observed consumer cursor, refreshed only when the ring looks full.
    cached_head: usize,
}

impl<T: Copy> Producer<T> {
    /// Returns the fixed number of slots.
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.shared.mask + 1
    }

    /// Attempts to append `value`; returns `Err(value)` if the ring is full.
    ///
    /// The consumer's cursor is re-read (with `Acquire`, pairing with the
    /// consumer's `Release`) only when the cached copy says the ring is full.
    /// After writing the slot, a `Release` store publishes the new tail.
    /// O(1), lock-free, and allocation-free.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        let capacity = self.capacity();
        if self.tail.wrapping_sub(self.cached_head) >= capacity {
            self.cached_head = self.shared.head.0.load(Ordering::Acquire);
            if self.tail.wrapping_sub(self.cached_head) >= capacity {
                return Err(value);
            }
        }
        let slot = &self.shared.buffer[self.tail & self.shared.mask];
        // SAFETY: `tail - head < capacity`, and the Acquire load that produced
        // `cached_head` synchronized with the consumer's Release store, so the
        // consumer has finished reading this slot (or it was never used). It
        // will not read it again until the Release store below publishes it,
        // and this unique producer is the only writer.
        unsafe { slot.get().write(MaybeUninit::new(value)) };
        self.tail = self.tail.wrapping_add(1);
        self.shared.tail.0.store(self.tail, Ordering::Release);
        Ok(())
    }

    /// Returns the number of queued values.
    ///
    /// The consumer may drain concurrently, so the true count can only be
    /// lower by the time the caller looks at it.
    #[inline]
    pub fn len(&self) -> usize {
        // `tail` is exact here and the consumer never passes it, so this
        // cannot underflow.
        self.tail
            .wrapping_sub(self.shared.head.0.load(Ordering::Acquire))
    }

    /// Returns whether the consumer has drained every pushed value.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` once the [`Consumer`] has been dropped: values pushed
    /// from now on will never be read.
    pub fn is_disconnected(&self) -> bool {
        self.shared.consumer_dropped.load(Ordering::Acquire)
    }
}

impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        // Release: every value pushed before this point is visible to a
        // consumer that reads the flag with Acquire.
        self.shared.producer_dropped.store(true, Ordering::Release);
    }
}

/// The reading end of an SPSC ring buffer. There is exactly one per buffer.
pub struct Consumer<T> {
    shared: Arc<Shared<T>>,
    /// This handle's copy of the consumer cursor; only it advances the cursor.
    head: usize,
    /// Last observed producer cursor, refreshed only when the ring looks empty.
    cached_tail: usize,
}

impl<T: Copy> Consumer<T> {
    /// Returns the fixed number of slots.
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.shared.mask + 1
    }

    /// Removes the oldest value, or returns `None` if the ring is empty.
    ///
    /// The producer's cursor is re-read (with `Acquire`, pairing with the
    /// producer's `Release`) only when the cached copy says the ring is empty.
    /// After reading the slot, a `Release` store to the head lets the
    /// producer reuse it. O(1), lock-free, and allocation-free.
    pub fn pop(&mut self) -> Option<T> {
        if self.head == self.cached_tail {
            self.cached_tail = self.shared.tail.0.load(Ordering::Acquire);
            if self.head == self.cached_tail {
                return None;
            }
        }
        let slot = &self.shared.buffer[self.head & self.shared.mask];
        // SAFETY: `head < tail`, and the Acquire load that produced
        // `cached_tail` synchronized with the producer's Release store that
        // published this slot, so it holds an initialized value. The producer
        // will not overwrite it until the Release store below frees it.
        let value = unsafe { slot.get().read().assume_init() };
        self.head = self.head.wrapping_add(1);
        self.shared.head.0.store(self.head, Ordering::Release);
        Some(value)
    }

    /// Returns the number of queued values.
    ///
    /// The producer may push concurrently, so the true count can only be
    /// higher by the time the caller looks at it.
    #[inline]
    pub fn len(&self) -> usize {
        // `head` is exact here and the producer's tail never falls behind
        // it, so this cannot underflow.
        self.shared
            .tail
            .0
            .load(Ordering::Acquire)
            .wrapping_sub(self.head)
    }

    /// Returns whether no values are currently queued.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` once the [`Producer`] has been dropped **and** every
    /// value it pushed has been popped: nothing will ever arrive again.
    pub fn is_disconnected(&self) -> bool {
        // Read the flag first. Its Acquire pairs with the Release in
        // `Drop for Producer`, so the tail loaded next is the final one.
        self.shared.producer_dropped.load(Ordering::Acquire)
            && self.shared.tail.0.load(Ordering::Acquire) == self.head
    }
}

impl<T> Drop for Consumer<T> {
    fn drop(&mut self) {
        self.shared.consumer_dropped.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn push_pop_single_thread() {
        let (mut tx, mut rx) = spsc_channel::<u64>(4);
        assert!(rx.is_empty());
        assert_eq!(tx.capacity(), 4);
        assert!(tx.push(1).is_ok());
        assert!(tx.push(2).is_ok());
        assert!(tx.push(3).is_ok());
        assert!(tx.push(4).is_ok());
        assert_eq!(tx.push(5), Err(5)); // full
        assert_eq!(tx.len(), 4);
        assert_eq!(rx.len(), 4);
        assert_eq!(rx.pop(), Some(1));
        assert_eq!(rx.pop(), Some(2));
        assert!(tx.push(6).is_ok());
        assert_eq!(rx.pop(), Some(3));
        assert_eq!(rx.pop(), Some(4));
        assert_eq!(rx.pop(), Some(6));
        assert_eq!(rx.pop(), None);
        assert!(tx.is_empty());
    }

    #[test]
    fn capacity_rounds_up_to_a_power_of_two() {
        let (tx, rx) = spsc_channel::<u8>(5);
        assert_eq!(tx.capacity(), 8);
        assert_eq!(rx.capacity(), 8);
    }

    #[test]
    #[should_panic(expected = "capacity must be positive")]
    fn zero_capacity_is_rejected() {
        let _ = spsc_channel::<u8>(0);
    }

    #[test]
    fn spsc_across_threads_preserves_order() {
        let n: u64 = if cfg!(miri) { 1_000 } else { 100_000 };
        let (mut tx, mut rx) = spsc_channel::<u64>(if cfg!(miri) { 16 } else { 1024 });
        let producer = thread::spawn(move || {
            let mut i = 0u64;
            while i < n {
                if tx.push(i).is_ok() {
                    i += 1;
                } else {
                    std::hint::spin_loop();
                }
            }
        });
        let mut expected = 0u64;
        while expected < n {
            match rx.pop() {
                Some(value) => {
                    assert_eq!(value, expected);
                    expected += 1;
                }
                None => {
                    // Fail fast instead of hanging if the producer died.
                    assert!(!rx.is_disconnected(), "producer stopped at {expected}");
                    std::hint::spin_loop();
                }
            }
        }
        producer.join().unwrap();
        assert_eq!(rx.pop(), None);
        assert!(rx.is_disconnected());
    }

    #[test]
    fn consumer_reports_disconnect_only_after_draining() {
        let (mut tx, mut rx) = spsc_channel::<u8>(4);
        tx.push(1).unwrap();
        assert!(!rx.is_disconnected());
        drop(tx);
        assert!(!rx.is_disconnected(), "a value is still queued");
        assert_eq!(rx.pop(), Some(1));
        assert!(rx.is_disconnected());
    }

    #[test]
    fn producer_sees_a_dropped_consumer() {
        let (tx, rx) = spsc_channel::<u8>(4);
        assert!(!tx.is_disconnected());
        drop(rx);
        assert!(tx.is_disconnected());
    }
}
