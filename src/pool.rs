//! Pre-allocated order arena with an intrusive doubly-linked FIFO list.
//!
//! Orders live in a flat, fixed-size array (`order_pool[MAX_ORDERS]`).
//! Free slots are tracked with a dense stack of indices (a "free list").
//! Each price level's resting orders form an intrusive doubly-linked list
//! via `next_idx` / `prev_idx` fields stored inline in the order record, so
//! there is no separate allocation per queue node and no pointer chasing
//! outside of the arena itself.

/// Sentinel used in place of `Option<u32>` for arena links, avoiding the
/// extra tag byte / padding that `Option<u32>` would otherwise not need but
/// keeps the code branch-free and cache-friendly.
pub const NIL: u32 = u32::MAX;

/// The side of the book on which an order rests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// A bid: willing to buy at this tick or a lower price.
    Buy,
    /// An ask: willing to sell at this tick or a higher price.
    Sell,
}

/// A single resting (or in-flight) order record. Kept small and `Copy` so
/// it packs densely into the arena; sized to fit within a single 64-byte
/// cache line.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Order {
    /// User-provided identifier, unique among live orders.
    pub order_id: u64,
    /// Quantity not yet executed.
    pub remaining_qty: u64,
    /// Engine-assigned arrival sequence retained as order metadata.
    pub sequence: u64,
    /// Dense integer price level used to index the book's arrays.
    pub price_tick: u32,
    /// Whether this order belongs to the bid or ask side.
    pub side: Side,
    /// Arena index of the next order in this price level's FIFO queue.
    pub next_idx: u32,
    /// Arena index of the previous order in this price level's FIFO queue.
    pub prev_idx: u32,
    /// Whether this arena slot currently contains a live order;
    /// [`OrderPool::free`] checks it to catch double frees.
    pub in_use: bool,
}

impl Order {
    #[inline(always)]
    const fn empty() -> Self {
        Self {
            order_id: 0,
            remaining_qty: 0,
            sequence: 0,
            price_tick: 0,
            side: Side::Buy,
            next_idx: NIL,
            prev_idx: NIL,
            in_use: false,
        }
    }
}

/// Fixed-capacity arena of [`Order`] records with O(1) allocate/free via a
/// dense free-list stack. All storage is allocated once in `new()`.
pub struct OrderPool {
    orders: Box<[Order]>,
    /// Dense stack of free indices; `free_top` is the number of currently
    /// free slots, so `free_stack[..free_top]` holds them.
    free_stack: Box<[u32]>,
    free_top: usize,
}

impl OrderPool {
    /// Allocates the arena and initializes every slot as free.
    ///
    /// `capacity` must be positive and fit in a `u32` index (excluding
    /// [`NIL`], which is reserved to mean "no linked order").
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0 && capacity < NIL as usize, "invalid capacity");
        let orders = vec![Order::empty(); capacity].into_boxed_slice();
        // free_stack[i] = capacity - 1 - i, so popping yields indices
        // 0, 1, 2, ... in order (purely cosmetic; any permutation works).
        let mut free_stack = vec![0u32; capacity].into_boxed_slice();
        for (i, slot) in free_stack.iter_mut().enumerate() {
            *slot = (capacity - 1 - i) as u32;
        }
        Self {
            orders,
            free_stack,
            free_top: capacity,
        }
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.orders.len()
    }

    #[inline(always)]
    pub fn live_count(&self) -> usize {
        self.orders.len() - self.free_top
    }

    /// Allocates a slot and initializes it. O(1) worst case. Returns `None`
    /// if the arena is exhausted.
    #[inline]
    pub fn allocate(
        &mut self,
        order_id: u64,
        side: Side,
        price_tick: u32,
        qty: u64,
        sequence: u64,
    ) -> Option<u32> {
        if self.free_top == 0 {
            return None;
        }
        // Pop one free index, then fully initialize it before exposing it as
        // a live order. The free-list itself is a stack, so this is O(1).
        self.free_top -= 1;
        let idx = self.free_stack[self.free_top];
        let order = &mut self.orders[idx as usize];
        order.order_id = order_id;
        order.remaining_qty = qty;
        order.sequence = sequence;
        order.price_tick = price_tick;
        order.side = side;
        order.next_idx = NIL;
        order.prev_idx = NIL;
        order.in_use = true;
        Some(idx)
    }

    /// Releases a slot back to the free list. O(1) worst case.
    ///
    /// # Panics
    /// Panics if the slot is not in use (a double free).
    #[inline]
    pub fn free(&mut self, idx: u32) {
        // A real check, not `debug_assert!`: in release builds a double free
        // would push the slot twice, and two different orders could later be
        // handed the same slot.
        assert!(
            self.orders[idx as usize].in_use,
            "OrderPool::free: slot {idx} is not in use (double free)"
        );
        self.orders[idx as usize].in_use = false;
        // Push the slot back so the next allocation can reuse it.
        self.free_stack[self.free_top] = idx;
        self.free_top += 1;
    }

    /// Returns the order stored at `idx`; `idx` must be a valid arena index.
    #[inline(always)]
    pub fn get(&self, idx: u32) -> &Order {
        &self.orders[idx as usize]
    }

    /// Returns mutable access to the order stored at `idx`.
    ///
    /// The caller must preserve the pool and price-level bookkeeping
    /// invariants when changing fields used by the book.
    #[inline(always)]
    pub fn get_mut(&mut self, idx: u32) -> &mut Order {
        &mut self.orders[idx as usize]
    }
}

/// Head/tail pointers for the intrusive FIFO queue resting at a single
/// price tick.
#[derive(Debug, Clone, Copy)]
pub struct PriceLevel {
    /// Oldest order at this price, or [`NIL`] when the queue is empty.
    pub head: u32,
    /// Newest order at this price, or [`NIL`] when the queue is empty.
    pub tail: u32,
    /// Number of orders resting at this level; lets callers detect when a
    /// level has drained without walking the list.
    pub count: u32,
    /// Sum of remaining quantity resting at this level (O(1) depth queries).
    pub total_qty: u64,
}

impl PriceLevel {
    /// Creates an empty queue with no resting quantity.
    pub const fn empty() -> Self {
        Self {
            head: NIL,
            tail: NIL,
            count: 0,
            total_qty: 0,
        }
    }

    /// Checks the queue's head sentinel rather than traversing its orders.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head == NIL
    }
}

impl PriceLevel {
    /// Appends `idx` to the tail of this level's FIFO queue. O(1).
    ///
    /// Returns `false` (without modifying the queue or `pool`) if adding
    /// this order's quantity would overflow `total_qty`. This guards
    /// against adversarially large `qty` values accumulating past
    /// `u64::MAX`; callers must free `idx` back to the pool instead of
    /// resting it when this returns `false`.
    #[inline]
    #[must_use = "must reject/free the order if this returns false"]
    pub fn push_back(&mut self, pool: &mut OrderPool, idx: u32) -> bool {
        let qty = pool.get(idx).remaining_qty;
        let Some(new_total) = self.total_qty.checked_add(qty) else {
            return false;
        };
        if self.tail == NIL {
            self.head = idx;
            self.tail = idx;
            pool.get_mut(idx).prev_idx = NIL;
            pool.get_mut(idx).next_idx = NIL;
        } else {
            let old_tail = self.tail;
            pool.get_mut(old_tail).next_idx = idx;
            pool.get_mut(idx).prev_idx = old_tail;
            pool.get_mut(idx).next_idx = NIL;
            self.tail = idx;
        }
        self.count += 1;
        self.total_qty = new_total;
        true
    }

    /// Removes `idx` from anywhere in this level's FIFO queue (head, middle,
    /// or tail). O(1) worst-case since links are intrusive.
    #[inline]
    pub fn remove(&mut self, pool: &mut OrderPool, idx: u32) {
        let qty = pool.get(idx).remaining_qty;
        let (prev, next) = {
            let order = pool.get(idx);
            (order.prev_idx, order.next_idx)
        };
        // Join the removed node's neighbors together, updating an endpoint
        // when the node was the head or tail. No traversal is needed.
        if prev != NIL {
            pool.get_mut(prev).next_idx = next;
        } else {
            self.head = next;
        }
        if next != NIL {
            pool.get_mut(next).prev_idx = prev;
        } else {
            self.tail = prev;
        }
        pool.get_mut(idx).prev_idx = NIL;
        pool.get_mut(idx).next_idx = NIL;
        // Using `checked_sub().expect(..)` rather than a bare `-=` turns a
        // broken bookkeeping invariant into a loud, deterministic panic in
        // every build profile instead of a silently wrapped (and much
        // larger) counter in release builds.
        self.count = self
            .count
            .checked_sub(1)
            .expect("PriceLevel::count underflow: bookkeeping invariant violated");
        self.total_qty = self
            .total_qty
            .checked_sub(qty)
            .expect("PriceLevel::total_qty underflow: bookkeeping invariant violated");
    }

    /// Reduces the resting quantity of the (head) order that was partially
    /// filled by `delta`. Caller is responsible for verifying `idx` belongs
    /// to this level and that `delta` does not exceed the order's remainder.
    #[inline(always)]
    pub fn reduce_qty(&mut self, pool: &mut OrderPool, idx: u32, delta: u64) {
        let order = pool.get_mut(idx);
        order.remaining_qty = order
            .remaining_qty
            .checked_sub(delta)
            .expect("Order::remaining_qty underflow: fill exceeded resting quantity");
        self.total_qty = self
            .total_qty
            .checked_sub(delta)
            .expect("PriceLevel::total_qty underflow: bookkeeping invariant violated");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_and_free_roundtrip() {
        let mut pool = OrderPool::new(4);
        let a = pool.allocate(1, Side::Buy, 10, 100, 0).unwrap();
        let b = pool.allocate(2, Side::Buy, 10, 200, 1).unwrap();
        assert_eq!(pool.live_count(), 2);
        pool.free(a);
        assert_eq!(pool.live_count(), 1);
        let c = pool.allocate(3, Side::Buy, 10, 300, 2).unwrap();
        assert_eq!(pool.live_count(), 2);
        assert_ne!(b, c);
    }

    #[test]
    #[should_panic(expected = "double free")]
    fn double_free_is_detected() {
        let mut pool = OrderPool::new(4);
        let a = pool.allocate(1, Side::Buy, 10, 100, 0).unwrap();
        pool.free(a);
        pool.free(a);
    }

    #[test]
    #[should_panic(expected = "count underflow")]
    fn removing_from_an_empty_level_is_detected() {
        // The order was allocated but never pushed onto this level, so the
        // level has nothing to remove: a bookkeeping bug in the caller.
        let mut pool = OrderPool::new(4);
        let mut level = PriceLevel::empty();
        let a = pool.allocate(1, Side::Buy, 10, 100, 0).unwrap();
        level.remove(&mut pool, a);
    }

    #[test]
    fn exhausts_capacity() {
        let mut pool = OrderPool::new(2);
        assert!(pool.allocate(1, Side::Buy, 0, 1, 0).is_some());
        assert!(pool.allocate(2, Side::Buy, 0, 1, 1).is_some());
        assert!(pool.allocate(3, Side::Buy, 0, 1, 2).is_none());
    }

    #[test]
    fn fifo_queue_push_and_remove_head_mid_tail() {
        let mut pool = OrderPool::new(8);
        let mut level = PriceLevel::empty();
        let a = pool.allocate(1, Side::Buy, 0, 10, 0).unwrap();
        let b = pool.allocate(2, Side::Buy, 0, 10, 1).unwrap();
        let c = pool.allocate(3, Side::Buy, 0, 10, 2).unwrap();
        assert!(level.push_back(&mut pool, a));
        assert!(level.push_back(&mut pool, b));
        assert!(level.push_back(&mut pool, c));
        assert_eq!(level.count, 3);
        assert_eq!(level.total_qty, 30);

        // Remove middle.
        level.remove(&mut pool, b);
        assert_eq!(level.count, 2);
        assert_eq!(pool.get(a).next_idx, c);
        assert_eq!(pool.get(c).prev_idx, a);

        // Remove head.
        level.remove(&mut pool, a);
        assert_eq!(level.head, c);
        assert_eq!(pool.get(c).prev_idx, NIL);

        // Remove tail (only remaining element).
        level.remove(&mut pool, c);
        assert!(level.is_empty());
        assert_eq!(level.total_qty, 0);
    }
}
