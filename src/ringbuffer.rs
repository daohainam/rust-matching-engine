//! A lock-free Single-Producer Single-Consumer ring buffer, inspired by the
//! LMAX Disruptor pattern, used as the input queue feeding the matching
//! engine's single-threaded hot path.
//!
//! Capacity is fixed at construction (power of two) and the backing storage
//! is allocated once up front; `push`/`pop` never allocate. Producer and
//! consumer cursors are cache-line padded to avoid false sharing.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(align(64))]
struct CachePadded<T>(T);

/// A fixed-capacity SPSC ring buffer. `T` must be `Copy` so that slots can
/// be overwritten without running destructors on the hot path.
pub struct SpscRingBuffer<T: Copy> {
    buffer: Box<[UnsafeCell<Option<T>>]>,
    mask: usize,
    head: CachePadded<AtomicUsize>, // consumer cursor
    tail: CachePadded<AtomicUsize>, // producer cursor
}

unsafe impl<T: Copy + Send> Sync for SpscRingBuffer<T> {}
unsafe impl<T: Copy + Send> Send for SpscRingBuffer<T> {}

impl<T: Copy> SpscRingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.next_power_of_two();
        let mut buffer = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            buffer.push(UnsafeCell::new(None));
        }
        Self {
            buffer: buffer.into_boxed_slice(),
            mask: capacity - 1,
            head: CachePadded(AtomicUsize::new(0)),
            tail: CachePadded(AtomicUsize::new(0)),
        }
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Producer-only. Attempts to push `value`; returns `Err(value)` if the
    /// buffer is full. O(1), lock-free, allocation-free.
    pub fn push(&self, value: T) -> Result<(), T> {
        let tail = self.tail.0.load(Ordering::Relaxed);
        let head = self.head.0.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= self.capacity() {
            return Err(value);
        }
        let idx = tail & self.mask;
        // SAFETY: single producer, and this slot has already been consumed
        // (or never used) because tail - head < capacity.
        unsafe {
            *self.buffer[idx].get() = Some(value);
        }
        self.tail.0.store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Consumer-only. Attempts to pop the oldest value; returns `None` if
    /// the buffer is empty. O(1), lock-free, allocation-free.
    pub fn pop(&self) -> Option<T> {
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let idx = head & self.mask;
        // SAFETY: single consumer, and this slot was published by the
        // producer because head != tail.
        let value = unsafe { (*self.buffer[idx].get()).take() };
        self.head.0.store(head.wrapping_add(1), Ordering::Release);
        value
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head.0.load(Ordering::Acquire) == self.tail.0.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        let tail = self.tail.0.load(Ordering::Acquire);
        let head = self.head.0.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn push_pop_single_thread() {
        let rb: SpscRingBuffer<u64> = SpscRingBuffer::new(4);
        assert!(rb.is_empty());
        assert!(rb.push(1).is_ok());
        assert!(rb.push(2).is_ok());
        assert!(rb.push(3).is_ok());
        assert!(rb.push(4).is_ok());
        assert!(rb.push(5).is_err()); // full
        assert_eq!(rb.pop(), Some(1));
        assert_eq!(rb.pop(), Some(2));
        assert!(rb.push(6).is_ok());
        assert_eq!(rb.pop(), Some(3));
        assert_eq!(rb.pop(), Some(4));
        assert_eq!(rb.pop(), Some(6));
        assert_eq!(rb.pop(), None);
    }

    #[test]
    fn spsc_across_threads_preserves_order() {
        use std::sync::Arc;
        let rb = Arc::new(SpscRingBuffer::<u64>::new(1024));
        let producer = rb.clone();
        let n = 100_000u64;
        let handle = thread::spawn(move || {
            let mut i = 0u64;
            while i < n {
                if producer.push(i).is_ok() {
                    i += 1;
                }
            }
        });
        let mut received = Vec::with_capacity(n as usize);
        while received.len() < n as usize {
            if let Some(v) = rb.pop() {
                received.push(v);
            }
        }
        handle.join().unwrap();
        for (expected, actual) in (0..n).zip(received.iter()) {
            assert_eq!(expected, *actual);
        }
    }
}
