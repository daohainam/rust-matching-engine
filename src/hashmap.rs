//! A fixed-capacity, open-addressing flat hash map used to resolve
//! `OrderId -> OrderIndex` without any heap allocation on the hot path.
//!
//! Capacity is always a power of two so that the modulo operation used to
//! compute the initial probe slot can be replaced with a cheap bitwise AND.
//! Collisions are resolved with linear probing. Deletions use backward-shift
//! deletion so that lookups never need tombstones (which would otherwise
//! degrade worst-case probe length over time).

const EMPTY: u64 = u64::MAX;

/// A single slot in the table. `key == EMPTY` means the slot is unoccupied.
#[derive(Clone, Copy)]
struct Slot {
    key: u64,
    value: u32,
}

/// Fixed-capacity open-addressing hash map from `u64` order id to `u32`
/// order index. All storage is pre-allocated in `new()`; no further
/// allocations occur.
pub struct FlatHashMap {
    slots: Box<[Slot]>,
    mask: usize,
    len: usize,
}

impl FlatHashMap {
    /// Creates a table capable of holding at least `capacity_hint` entries.
    /// The allocation happens once, up-front; `insert`/`remove`/`get` never
    /// allocate afterwards.
    pub fn with_capacity(capacity_hint: usize) -> Self {
        let capacity = (capacity_hint.max(16) * 2).next_power_of_two();
        let slots = vec![
            Slot {
                key: EMPTY,
                value: 0
            };
            capacity
        ]
        .into_boxed_slice();
        Self {
            slots,
            mask: capacity - 1,
            len: 0,
        }
    }

    #[inline(always)]
    fn hash(key: u64) -> usize {
        // splitmix64 finalizer: cheap, well-distributed avalanche.
        let mut z = key.wrapping_add(0x9E3779B97F4A7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        (z ^ (z >> 31)) as usize
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Inserts `key -> value`. Returns the previous value if `key` was
    /// already present. O(1) amortized.
    ///
    /// # Panics
    /// Panics if the table is full (callers should size the table to the
    /// maximum number of concurrently-live orders up front).
    pub fn insert(&mut self, key: u64, value: u32) -> Option<u32> {
        debug_assert!(key != EMPTY, "u64::MAX is reserved as the empty sentinel");
        let mut idx = Self::hash(key) & self.mask;
        loop {
            let slot = &mut self.slots[idx];
            if slot.key == EMPTY {
                slot.key = key;
                slot.value = value;
                self.len += 1;
                return None;
            }
            if slot.key == key {
                let old = slot.value;
                slot.value = value;
                return Some(old);
            }
            idx = (idx + 1) & self.mask;
        }
    }

    /// Looks up `key`. O(1) worst-case (amortized over well-distributed keys).
    #[inline]
    pub fn get(&self, key: u64) -> Option<u32> {
        let mut idx = Self::hash(key) & self.mask;
        loop {
            let slot = &self.slots[idx];
            if slot.key == EMPTY {
                return None;
            }
            if slot.key == key {
                return Some(slot.value);
            }
            idx = (idx + 1) & self.mask;
        }
    }

    /// Removes `key`, returning its value if present. Uses backward-shift
    /// deletion to keep probe chains short without tombstones.
    pub fn remove(&mut self, key: u64) -> Option<u32> {
        let mut idx = Self::hash(key) & self.mask;
        loop {
            let slot = self.slots[idx];
            if slot.key == EMPTY {
                return None;
            }
            if slot.key == key {
                let removed = slot.value;
                self.slots[idx].key = EMPTY;
                self.len -= 1;
                self.backward_shift(idx);
                return Some(removed);
            }
            idx = (idx + 1) & self.mask;
        }
    }

    fn backward_shift(&mut self, mut hole: usize) {
        let mut probe = (hole + 1) & self.mask;
        loop {
            let slot = self.slots[probe];
            if slot.key == EMPTY {
                return;
            }
            let ideal = Self::hash(slot.key) & self.mask;
            // If the slot's ideal position is not strictly between `hole`
            // (exclusive) and `probe` (inclusive) in probe order, it can be
            // moved back into the hole.
            let displaced = if probe >= hole {
                ideal <= hole || ideal > probe
            } else {
                ideal <= hole && ideal > probe
            };
            if displaced {
                self.slots[hole] = slot;
                self.slots[probe].key = EMPTY;
                hole = probe;
            }
            probe = (probe + 1) & self.mask;
            if probe == hole {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_remove() {
        let mut map = FlatHashMap::with_capacity(16);
        assert_eq!(map.insert(1, 100), None);
        assert_eq!(map.insert(2, 200), None);
        assert_eq!(map.get(1), Some(100));
        assert_eq!(map.get(2), Some(200));
        assert_eq!(map.get(3), None);
        assert_eq!(map.remove(1), Some(100));
        assert_eq!(map.get(1), None);
        assert_eq!(map.get(2), Some(200));
    }

    #[test]
    fn update_existing_key() {
        let mut map = FlatHashMap::with_capacity(16);
        map.insert(42, 1);
        assert_eq!(map.insert(42, 2), Some(1));
        assert_eq!(map.get(42), Some(2));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn survives_many_collisions_and_deletions() {
        let mut map = FlatHashMap::with_capacity(64);
        for i in 0..100u64 {
            map.insert(i, i as u32 * 10);
        }
        for i in 0..100u64 {
            assert_eq!(map.get(i), Some(i as u32 * 10));
        }
        // Remove every other key and verify the rest are still reachable
        // (this exercises backward-shift deletion under collisions).
        for i in (0..100u64).step_by(2) {
            assert_eq!(map.remove(i), Some(i as u32 * 10));
        }
        for i in 0..100u64 {
            if i % 2 == 0 {
                assert_eq!(map.get(i), None);
            } else {
                assert_eq!(map.get(i), Some(i as u32 * 10));
            }
        }
    }
}
