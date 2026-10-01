//! Hierarchical 2-level bitset used as a dense price ladder index.
//!
//! Each bit in the lower level represents whether a given price tick has at
//! least one resting order. Each bit in the summary word represents whether
//! any tick within the corresponding 64-tick group is active. This gives
//! O(1) (via hardware bit-scan intrinsics) lookup of the best (highest or
//! lowest) active tick without ever scanning the full ladder.

/// Number of ticks tracked by a single [`TwoLevelBitset`]. Must be a
/// multiple of 64 (word size) and, for the summary word to cover every
/// group, at most 64 * 64 = 4096. Both limits are checked at compile time.
pub const TICKS: usize = 4096;
const WORDS: usize = TICKS / 64;

// The summary word has one bit per 64-tick word, so it can describe at most
// 64 words. A larger `TICKS` would make `1u64 << word_idx` overflow, so the
// compiler rejects it instead of leaving it to a comment.
const _: () = assert!(
    TICKS.is_multiple_of(64) && TICKS <= 64 * 64,
    "TICKS must be a multiple of 64 and at most 4096"
);

/// A fixed-size, allocation-free two-level bitset over `TICKS` price ticks.
#[derive(Debug, Clone)]
pub struct TwoLevelBitset {
    /// One bit per group of 64 ticks; set iff any tick in that group is set.
    summary: u64,
    /// One bit per tick.
    words: [u64; WORDS],
}

impl Default for TwoLevelBitset {
    fn default() -> Self {
        Self::new()
    }
}

impl TwoLevelBitset {
    /// Creates an empty bitset with no active price ticks.
    pub const fn new() -> Self {
        Self {
            summary: 0,
            words: [0u64; WORDS],
        }
    }

    /// Returns `true` when no price tick is active.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.summary == 0
    }

    /// Marks `tick` as active. `tick` must be less than [`TICKS`].
    ///
    /// The lower-level bit records the tick itself. The summary bit records
    /// that its 64-tick word is non-empty, allowing a later best-price lookup
    /// to skip all empty words.
    #[inline(always)]
    pub fn set(&mut self, tick: usize) {
        debug_assert!(tick < TICKS);
        let word_idx = tick >> 6;
        let bit_idx = tick & 63;
        self.words[word_idx] |= 1u64 << bit_idx;
        self.summary |= 1u64 << word_idx;
    }

    /// Marks `tick` as inactive. `tick` must be less than [`TICKS`].
    ///
    /// The summary bit is cleared only when this was the last active tick in
    /// its word; otherwise it must remain set so the summary has no false
    /// negatives.
    #[inline(always)]
    pub fn clear(&mut self, tick: usize) {
        debug_assert!(tick < TICKS);
        let word_idx = tick >> 6;
        let bit_idx = tick & 63;
        self.words[word_idx] &= !(1u64 << bit_idx);
        if self.words[word_idx] == 0 {
            self.summary &= !(1u64 << word_idx);
        }
    }

    /// Returns whether `tick` is active. `tick` must be less than [`TICKS`].
    #[inline(always)]
    pub fn test(&self, tick: usize) -> bool {
        debug_assert!(tick < TICKS);
        let word_idx = tick >> 6;
        let bit_idx = tick & 63;
        (self.words[word_idx] >> bit_idx) & 1 == 1
    }

    /// Returns the lowest active tick (used for best-ask lookups), using
    /// `trailing_zeros` hardware bit-scan (`tzcnt`) at both levels.
    ///
    /// The first set bit in the summary identifies the lowest non-empty
    /// word; the first set bit in that word identifies the tick within it.
    /// Returns `None` when the summary is empty.
    #[inline(always)]
    pub fn lowest(&self) -> Option<usize> {
        if self.summary == 0 {
            return None;
        }
        let word_idx = self.summary.trailing_zeros() as usize;
        let word = self.words[word_idx];
        debug_assert!(word != 0);
        let bit_idx = word.trailing_zeros() as usize;
        Some(word_idx * 64 + bit_idx)
    }

    /// Returns the highest active tick (used for best-bid lookups), using
    /// `leading_zeros` hardware bit-scan (`lzcnt`) at both levels.
    ///
    /// The highest set bit in the summary identifies the highest non-empty
    /// word; the highest set bit in that word identifies the tick within it.
    /// Returns `None` when the summary is empty.
    #[inline(always)]
    pub fn highest(&self) -> Option<usize> {
        if self.summary == 0 {
            return None;
        }
        let word_idx = 63 - self.summary.leading_zeros() as usize;
        let word = self.words[word_idx];
        debug_assert!(word != 0);
        let bit_idx = 63 - word.leading_zeros() as usize;
        Some(word_idx * 64 + bit_idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bitset_has_no_extrema() {
        let bs = TwoLevelBitset::new();
        assert!(bs.is_empty());
        assert_eq!(bs.lowest(), None);
        assert_eq!(bs.highest(), None);
    }

    #[test]
    fn set_and_query_single_tick() {
        let mut bs = TwoLevelBitset::new();
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
        let mut bs = TwoLevelBitset::new();
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
}
