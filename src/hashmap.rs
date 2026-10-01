//! A fixed-capacity, open-addressing flat hash map used to resolve
//! `OrderId -> OrderIndex` without any heap allocation on the hot path.
//!
//! Capacity is always a power of two so that the modulo operation used to
//! compute the initial probe slot can be replaced with a cheap bitwise AND.
//! Collisions are resolved with linear probing. Deletions use backward-shift
//! deletion so that lookups never need tombstones (which would otherwise
//! degrade worst-case probe length over time).
//!
//! Keys are caller-supplied order IDs, so every table mixes a secret
//! per-instance seed into its hash. Without it the SplitMix64 finalizer could
//! simply be inverted to craft IDs that all share one probe chain, turning
//! every operation into a scan of that chain.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

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
/// entry count. [`insert`](Self::insert) always keeps at least one slot empty,
/// which is what lets every probe loop terminate.
pub struct FlatHashMap {
    slots: Box<[Slot]>,
    mask: usize,
    len: usize,
    /// Secret mixed into every hash so slot positions cannot be predicted
    /// (and deliberately collided) from key values alone.
    seed: u64,
}

impl FlatHashMap {
    /// Creates a table with room for at least `capacity_hint` entries.
    ///
    /// The physical slot count is the next power of two at or above twice
    /// `max(capacity_hint, 16)`. Keeping the expected load at or below one
    /// half leaves empty slots that stop unsuccessful linear probes. The
    /// allocation happens once; map operations never resize or allocate.
    /// The hash seed is random, drawn from the standard library's
    /// [`RandomState`].
    ///
    /// # Panics
    /// Panics if the slot count overflows `usize`.
    pub fn with_capacity(capacity_hint: usize) -> Self {
        Self::with_capacity_and_seed(capacity_hint, RandomState::new().hash_one(0u64))
    }

    /// Like [`with_capacity`](Self::with_capacity), but with an explicit hash
    /// seed for reproducible layouts. A seed an adversary can guess gives up
    /// the protection against deliberately colliding keys.
    ///
    /// # Panics
    /// Panics if the slot count overflows `usize`.
    pub fn with_capacity_and_seed(capacity_hint: usize, seed: u64) -> Self {
        // Checked arithmetic: in release builds a plain `* 2` would wrap to
        // a tiny table instead of failing.
        let capacity = capacity_hint
            .max(16)
            .checked_mul(2)
            .and_then(usize::checked_next_power_of_two)
            .expect("hash map capacity overflows usize");
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
            seed,
        }
    }

    #[inline(always)]
    fn hash(&self, key: u64) -> usize {
        // XOR in the secret seed, then let SplitMix64's finalizer avalanche
        // nearby IDs across the machine word; the caller then masks the low
        // bits to select a table slot.
        let mut z = (key ^ self.seed).wrapping_add(0x9E3779B97F4A7C15);
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
    /// not grow.
    ///
    /// # Panics
    /// Panics if `key` is [`RESERVED_KEY`], because that value marks empty
    /// slots and cannot also represent a stored key. Also panics if inserting
    /// a new key would fill the last empty slot, since every probe loop needs
    /// an empty slot to stop at.
    pub fn insert(&mut self, key: u64, value: u32) -> Option<u32> {
        // This is a real (non-debug-only) check rather than `debug_assert!`:
        // in release builds, a caller-supplied `key == EMPTY` would be
        // silently indistinguishable from an unoccupied slot, making the
        // inserted entry permanently unreachable via `get`/`remove` while
        // still consuming a pool slot and inflating `len` without bound on
        // repeated calls. Callers (e.g. `Engine`) must reject this reserved
        // value before it ever reaches the map.
        assert!(key != EMPTY, "u64::MAX is reserved as the empty sentinel");
        let mut idx = self.hash(key) & self.mask;
        loop {
            let slot = &mut self.slots[idx];
            if slot.key == EMPTY {
                // `get`, `insert` and `remove` all stop at the first EMPTY
                // slot, so the last one must never be filled: fail loudly
                // here instead of letting a later lookup spin forever.
                assert!(
                    self.len + 1 < self.mask + 1,
                    "FlatHashMap is full: at least one slot must stay empty"
                );
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
    /// many slots must be examined. A missing key's probe always terminates
    /// because [`insert`](Self::insert) never fills the last empty slot.
    #[inline]
    pub fn get(&self, key: u64) -> Option<u32> {
        let mut idx = self.hash(key) & self.mask;
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
        let mut idx = self.hash(key) & self.mask;
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
            let ideal = self.hash(slot.key) & self.mask;
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
    #[should_panic(expected = "FlatHashMap is full")]
    fn insert_refuses_to_fill_the_last_empty_slot() {
        // 32 slots: 31 keys fit, the 32nd would leave no slot for a probe to
        // stop at, so `get` of a missing key would loop forever.
        let mut map = FlatHashMap::with_capacity(16);
        assert_eq!(map.capacity(), 32);
        for key in 0..32u64 {
            map.insert(key, 0);
        }
    }

    #[test]
    #[should_panic(expected = "capacity overflows usize")]
    fn absurd_capacity_is_rejected_instead_of_wrapping() {
        let _ = FlatHashMap::with_capacity(usize::MAX);
    }

    #[test]
    fn get_and_remove_are_safe_for_reserved_sentinel_key() {
        let mut map = FlatHashMap::with_capacity(16);
        assert_eq!(map.get(RESERVED_KEY), None);
        assert_eq!(map.remove(RESERVED_KEY), None);
    }

    /// Inverts the SplitMix64 finalizer to produce a key whose hash under
    /// `seed` is exactly `target_hash`, as an attacker could for a known seed.
    fn craft_key(target_hash: u64, seed: u64) -> u64 {
        fn undo_xorshift(y: u64, shift: u32) -> u64 {
            (1..)
                .map(|k| k * shift)
                .take_while(|&s| s < 64)
                .fold(y, |x, s| x ^ (y >> s))
        }
        fn inverse(odd: u64) -> u64 {
            // Newton's iteration doubles the correct low bits each round.
            (0..6).fold(odd, |x, _| {
                x.wrapping_mul(2u64.wrapping_sub(odd.wrapping_mul(x)))
            })
        }
        let z = undo_xorshift(target_hash, 31).wrapping_mul(inverse(0x94D049BB133111EB));
        let z = undo_xorshift(z, 27).wrapping_mul(inverse(0xBF58476D1CE4E5B9));
        undo_xorshift(z, 30).wrapping_sub(0x9E3779B97F4A7C15) ^ seed
    }

    #[test]
    fn keys_crafted_for_one_seed_spread_out_under_another() {
        let crafted: Vec<u64> = (1..=1_000u64).map(|i| craft_key(i << 32, 0)).collect();

        // With a known seed, every crafted key targets the same ideal slot...
        let known = FlatHashMap::with_capacity_and_seed(1_000, 0);
        assert!(crafted.iter().all(|&key| known.hash(key) & known.mask == 0));

        // ...but under any other seed they scatter like ordinary keys.
        let seeded = FlatHashMap::with_capacity_and_seed(1_000, 0x5EED_1234_ABCD_9876);
        let mut buckets = vec![0usize; seeded.capacity()];
        for &key in &crafted {
            buckets[seeded.hash(key) & seeded.mask] += 1;
        }
        let worst = buckets.into_iter().max().unwrap();
        assert!(worst < 32, "worst bucket held {worst} crafted keys");
    }

    #[test]
    fn seeded_maps_behave_identically() {
        // The seed changes only where keys live, never what the map returns.
        let mut a = FlatHashMap::with_capacity_and_seed(64, 1);
        let mut b = FlatHashMap::with_capacity_and_seed(64, 2);
        for i in 0..100u64 {
            assert_eq!(a.insert(i * 7, i as u32), b.insert(i * 7, i as u32));
        }
        for i in (0..100u64).step_by(3) {
            assert_eq!(a.remove(i * 7), b.remove(i * 7));
        }
        for i in 0..700u64 {
            assert_eq!(a.get(i), b.get(i));
        }
        assert_eq!(a.len(), b.len());
    }
}
