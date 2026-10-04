//! Hierarchical bitset used as a dense price ladder index.
//!
//! The lowest level has one bit per price tick: set when that tick has at
//! least one resting order. Each level above has one bit per 64-bit word of
//! the level below: set when that word is not zero. The top level is a
//! single word. Finding the best (lowest or highest) active tick therefore
//! takes one hardware bit-scan per level, never a scan of the ladder.
//!
//! How many levels there are depends on the number of ticks, chosen when
//! the bitset is created: up to 64 ticks need one level, up to 4096 (64 ×
//! 64) two, up to 262,144 three, and up to [`MAX_TICKS`] (2^24) four. For
//! 4096 ticks the bitset is exactly the classic two-level design: 64 words
//! of ticks and one summary word.

/// The largest number of ticks a [`TickBitset`] supports: 2^24 = 64^4, four
/// levels. Price ticks are `u32`, but a ladder this long already needs 16
/// million price levels per side.
pub const MAX_TICKS: usize = 1 << 24;
/// The most levels a [`TickBitset`] can have; see [`MAX_TICKS`].
const MAX_LEVELS: usize = 4;

/// An allocation-free (after [`new`](Self::new)) hierarchical bitset over a
/// fixed number of price ticks.
#[derive(Debug, Clone)]
pub struct TickBitset {
    /// Every level's words in one array: level 0 (one bit per tick) first,
    /// then each summary level, ending with the single top word.
    words: Box<[u64]>,
    /// Where each level starts in `words`.
    offsets: [usize; MAX_LEVELS],
    /// How many levels are in use, 1 to [`MAX_LEVELS`].
    levels: usize,
    /// How many ticks this bitset tracks.
    ticks: usize,
}

impl TickBitset {
    /// Creates an empty bitset for ticks `0..ticks`.
    ///
    /// # Panics
    /// Panics if `ticks` is zero or above [`MAX_TICKS`].
    pub fn new(ticks: usize) -> Self {
        assert!(
            (1..=MAX_TICKS).contains(&ticks),
            "a price ladder needs 1 to {MAX_TICKS} ticks, not {ticks}"
        );
        // Each level needs one bit per word of the level below, rounded up
        // to whole words, until a single word covers everything.
        let mut offsets = [0; MAX_LEVELS];
        let mut levels = 0;
        let mut total = 0;
        let mut bits = ticks;
        loop {
            let words = bits.div_ceil(64);
            offsets[levels] = total;
            total += words;
            levels += 1;
            if words == 1 {
                break;
            }
            bits = words;
        }
        Self {
            words: vec![0; total].into_boxed_slice(),
            offsets,
            levels,
            ticks,
        }
    }

    /// Returns how many ticks this bitset tracks.
    #[inline(always)]
    pub fn ticks(&self) -> usize {
        self.ticks
    }

    /// Returns how many levels this bitset uses.
    #[inline(always)]
    pub fn levels(&self) -> usize {
        self.levels
    }

    /// The single word at the top of the hierarchy.
    #[inline(always)]
    fn top(&self) -> u64 {
        self.words[self.offsets[self.levels - 1]]
    }

    /// Returns `true` when no price tick is active.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.top() == 0
    }

    /// Marks `tick` as active. `tick` must be less than [`ticks`](Self::ticks).
    ///
    /// The level-0 bit records the tick itself. Each level above learns that
    /// the word below is no longer empty; the climb stops at the first word
    /// that was not empty already, because the levels above it know.
    #[inline(always)]
    pub fn set(&mut self, tick: usize) {
        debug_assert!(tick < self.ticks);
        let mut index = tick;
        for level in 0..self.levels {
            let slot = self.offsets[level] + (index >> 6);
            let was_empty = self.words[slot] == 0;
            self.words[slot] |= 1u64 << (index & 63);
            if !was_empty {
                return;
            }
            index >>= 6;
        }
    }

    /// Marks `tick` as inactive. `tick` must be less than [`ticks`](Self::ticks).
    ///
    /// A summary bit is cleared only when the word below it has just become
    /// zero; otherwise it must remain set, so a summary never claims a word
    /// is empty when it is not.
    #[inline(always)]
    pub fn clear(&mut self, tick: usize) {
        debug_assert!(tick < self.ticks);
        let mut index = tick;
        for level in 0..self.levels {
            let slot = self.offsets[level] + (index >> 6);
            self.words[slot] &= !(1u64 << (index & 63));
            if self.words[slot] != 0 {
                return;
            }
            index >>= 6;
        }
    }

    /// Returns whether `tick` is active. `tick` must be less than
    /// [`ticks`](Self::ticks).
    #[inline(always)]
    pub fn test(&self, tick: usize) -> bool {
        debug_assert!(tick < self.ticks);
        (self.words[tick >> 6] >> (tick & 63)) & 1 == 1
    }

    /// Returns the lowest active tick (used for best-ask lookups), with one
    /// `trailing_zeros` hardware bit-scan (`tzcnt`) per level.
    ///
    /// The lowest set bit of the top word says which word of the next level
    /// down to look at; its lowest set bit says which word below that, and
    /// so on, until level 0 names the tick. Returns `None` when empty.
    #[inline(always)]
    pub fn lowest(&self) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let mut index = 0;
        for level in (0..self.levels).rev() {
            let word = self.words[self.offsets[level] + index];
            debug_assert!(word != 0);
            index = index * 64 + word.trailing_zeros() as usize;
        }
        Some(index)
    }

    /// Returns the highest active tick (used for best-bid lookups), with one
    /// `leading_zeros` hardware bit-scan (`lzcnt`) per level, following the
    /// highest set bit down from the top word. Returns `None` when empty.
    #[inline(always)]
    pub fn highest(&self) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let mut index = 0;
        for level in (0..self.levels).rev() {
            let word = self.words[self.offsets[level] + index];
            debug_assert!(word != 0);
            index = index * 64 + (63 - word.leading_zeros() as usize);
        }
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn empty_bitset_has_no_extrema() {
        let bs = TickBitset::new(4096);
        assert!(bs.is_empty());
        assert_eq!(bs.lowest(), None);
        assert_eq!(bs.highest(), None);
    }

    #[test]
    fn set_and_query_single_tick() {
        let mut bs = TickBitset::new(4096);
        bs.set(1234);
        assert!(bs.test(1234));
        assert_eq!(bs.lowest(), Some(1234));
        assert_eq!(bs.highest(), Some(1234));
        bs.clear(1234);
        assert!(!bs.test(1234));
        assert!(bs.is_empty());
    }

    #[test]
    fn tracks_lowest_and_highest_across_groups() {
        let mut bs = TickBitset::new(4096);
        bs.set(0);
        bs.set(63);
        bs.set(64);
        bs.set(4095);
        bs.set(2000);
        assert_eq!(bs.lowest(), Some(0));
        assert_eq!(bs.highest(), Some(4095));

        bs.clear(4095);
        assert_eq!(bs.highest(), Some(2000));
        bs.clear(2000);
        assert_eq!(bs.highest(), Some(64));
        bs.clear(64);
        assert_eq!(bs.highest(), Some(63));
        bs.clear(63);
        assert_eq!(bs.highest(), Some(0));
        bs.clear(0);
        assert!(bs.is_empty());
    }

    #[test]
    fn the_number_of_levels_follows_the_number_of_ticks() {
        for (ticks, levels) in [
            (1, 1),
            (64, 1),
            (65, 2),
            (4096, 2),
            (4097, 3),
            (262_144, 3),
            (262_145, 4),
            (MAX_TICKS, 4),
        ] {
            assert_eq!(TickBitset::new(ticks).levels(), levels, "{ticks} ticks");
        }
    }

    #[test]
    #[should_panic(expected = "a price ladder needs")]
    fn a_ladder_too_long_for_four_levels_is_rejected() {
        TickBitset::new(MAX_TICKS + 1);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Thousands of operations: too slow under Miri.
    fn agrees_with_an_ordered_set_at_every_size() {
        // Ticks near both ends and around word and summary boundaries are
        // the interesting ones, so draw from those as well as at random.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for ticks in [1, 63, 64, 100, 4096, 5_000, 262_144, 300_000] {
            let mut bitset = TickBitset::new(ticks);
            let mut reference = BTreeSet::new();
            let edges = [0, 63, 64, 4095, 4096, 262_143, 262_144, ticks - 1];
            for _ in 0..4_000 {
                let tick = if next() % 4 == 0 {
                    edges[(next() % edges.len() as u64) as usize].min(ticks - 1)
                } else {
                    (next() % ticks as u64) as usize
                };
                if next() % 3 == 0 {
                    bitset.clear(tick);
                    reference.remove(&tick);
                } else {
                    bitset.set(tick);
                    reference.insert(tick);
                }
                assert_eq!(bitset.lowest(), reference.first().copied(), "{ticks} ticks");
                assert_eq!(bitset.highest(), reference.last().copied(), "{ticks} ticks");
                assert_eq!(bitset.test(tick), reference.contains(&tick));
            }
        }
    }
}
