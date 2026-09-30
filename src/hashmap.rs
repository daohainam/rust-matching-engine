//! A fixed-capacity, open-addressing flat hash map used to resolve
//! `OrderId -> OrderIndex` without any heap allocation on the hot path.
//!
//! Capacity is always a power of two so that the modulo operation used to
//! compute the initial probe slot can be replaced with a cheap bitwise AND.
//! Collisions are resolved with linear probing. Deletions use backward-shift
//! deletion so that lookups never need tombstones (which would otherwise
//! degrade worst-case probe length over time).

/// Sentinel key value reserved to mean "unoccupied slot". Callers must
/// never insert a real key equal to this value; [`FlatHashMap::insert`]
/// enforces this with a hard runtime assertion (not just in debug builds),
/// since silently accepting it would corrupt lookups instead of panicking.
pub const RESERVED_KEY: u64 = u64::MAX;
const EMPTY: u64 = RESERVED_KEY;

/// A single slot in the table. `key == EMPTY` means the slot is unoccupied.
#[derive(Clone, Copy)]
struct Slot {
    key: u64,
    value: u32,
}

/// Fixed-capacity open-addressing hash map from `u64` order ID to `u32`
/// order index. All storage is pre-allocated in `new()`; the table never
/// resizes, so callers must choose a hint large enough for the maximum live
/// entry count and keep at least one slot empty for probe termination.
pub struct FlatHashMap {
    slots: Box<[Slot]>,
    mask: usize,
    len: usize,
}

impl FlatHashMap {
    /// Creates a table with room for at least `capacity_hint` entries.
    ///
    /// The physical slot count is the next power of two at or above twice
    /// `max(capacity_hint, 16)`. Keeping the expected load at or below one
    /// half leaves empty slots that stop unsuccessful linear probes. The
    /// allocation happens once; map operations never resize or allocate.
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
        // SplitMix64's finalizer avalanches nearby IDs across the machine
        // word; the caller then masks the low bits to select a table slot.
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

    /// Inserts `key -> value`, returning the replaced value if `key` exists.
    ///
    /// Collisions are resolved by checking consecutive slots until an empty
    /// slot or matching key is found. This is expected O(1) with a suitable
    /// load factor, but can take O(capacity) in the worst case. The table does
    /// not grow, and callers must leave at least one slot empty so a missing
    /// key has a stopping point.
    ///
    /// # Panics
    /// Panics if `key` is [`RESERVED_KEY`], because that value marks empty
    /// slots and cannot also represent a stored key.
    pub fn insert(&mut self, key: u64, value: u32) -> Option<u32> {
        // This is a real (non-debug-only) check rather than `debug_assert!`:
        // in release builds, a caller-supplied `key == EMPTY` would be
        // silently indistinguishable from an unoccupied slot, making the
        // inserted entry permanently unreachable via `get`/`remove` while
        // still consuming a pool slot and inflating `len` without bound on
        // repeated calls. Callers (e.g. `Engine`) must reject this reserved
        // value before it ever reaches the map.
        assert!(key != EMPTY, "u64::MAX is reserved as the empty sentinel");
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

    /// Looks up `key` using linear probing.
    ///
    /// Expected O(1) for a suitably sized table; worst-case O(capacity) when
    /// many slots must be examined. As with insertion, the table must retain
    /// an empty slot so a missing key's probe can terminate.
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
    /// deletion to keep probe chains short without tombstones. The scan and
    /// any shifts take O(capacity) in the worst case.
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
            // A key displaced from its ideal slot must remain reachable from
            // that slot. Move it into the hole only when its probe path crosses
            // the hole; otherwise leave the gap for a later search to stop at.
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

    #[test]
    #[should_panic(expected = "reserved as the empty sentinel")]
    fn insert_rejects_reserved_sentinel_key_in_all_build_profiles() {
        // This uses `assert!` (not `debug_assert!`) specifically so that
        // this invariant holds in release builds too; silently accepting
        // `RESERVED_KEY` would make the entry unreachable via `get` while
        // still incrementing `len`, corrupting the map's bookkeeping.
        let mut map = FlatHashMap::with_capacity(16);
        map.insert(RESERVED_KEY, 0);
    }

    #[test]
    fn get_and_remove_are_safe_for_reserved_sentinel_key() {
        let mut map = FlatHashMap::with_capacity(16);
        assert_eq!(map.get(RESERVED_KEY), None);
        assert_eq!(map.remove(RESERVED_KEY), None);
    }
}
