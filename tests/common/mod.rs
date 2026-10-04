//! Shared helpers for the randomized integration tests: a seeded PRNG, a
//! reproducible generator of engine requests that mixes ordinary flow with
//! edge inputs, and a self-deleting temp file path.

// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]

use std::ops::Range;
use std::path::PathBuf;

use matching_engine::engine::MAX_TICKS;
use matching_engine::{OrderType, Side};

/// xorshift64*: tiny, deterministic, and plenty for driving test inputs.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // The state must never be zero; spreading the seed keeps nearby
        // seeds from producing correlated streams.
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..n`; modulo bias is irrelevant for test inputs.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn percent(&mut self, p: u64) -> bool {
        self.below(100) < p
    }
}

/// One engine request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Place {
        order_id: u64,
        side: Side,
        order_type: OrderType,
        price_tick: u32,
        qty: u64,
    },
    Cancel {
        order_id: u64,
    },
}

/// Most orders land in this narrow band so they cross each other often.
pub const BAND: Range<u32> = 1_000..1_016;
/// Far from the band, oversized orders rest without crossing ordinary flow,
/// so a second one at the same tick overflows the level's total quantity.
pub const FAR_BID_TICK: u32 = 10;
pub const FAR_ASK_TICK: u32 = 4_000;

/// Every tick the generator can rest an order at, for depth comparisons.
pub fn watched_ticks() -> impl Iterator<Item = u32> {
    BAND.chain([0, FAR_BID_TICK, FAR_ASK_TICK, MAX_TICKS as u32 - 1])
}

/// How many recently issued IDs cancels and reuse attempts choose from.
const RECENT_WINDOW: usize = 256;

/// Produces a reproducible request stream: mostly limit orders in [`BAND`],
/// some other order types, some cancels, and occasional edge inputs (zero quantity, out-of-range
/// ticks, the reserved ID, reused IDs, deep sweeps, oversized quantities).
pub struct OpGenerator {
    rng: Rng,
    next_id: u64,
    recent: [u64; RECENT_WINDOW],
    issued: usize,
}

impl OpGenerator {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            next_id: 1,
            recent: [0; RECENT_WINDOW],
            issued: 0,
        }
    }

    pub fn next_op(&mut self) -> Op {
        if self.issued > 0 && self.rng.percent(30) {
            let order_id = match self.rng.below(100) {
                0..=4 => u64::MAX,                              // reserved sentinel
                5..=14 => u64::MAX - 1 - self.rng.below(1_000), // never issued
                _ => self.recent_id(),                          // live, filled, or gone
            };
            return Op::Cancel { order_id };
        }

        let side = if self.rng.percent(50) {
            Side::Buy
        } else {
            Side::Sell
        };
        let mut order_id = self.fresh_id();
        let mut price_tick = BAND.start + self.rng.below(u64::from(BAND.end - BAND.start)) as u32;
        let mut qty = 1 + self.rng.below(50);
        match self.rng.below(100) {
            0 => qty = 0,
            1 => price_tick = MAX_TICKS as u32 + self.rng.below(8) as u32,
            2 => order_id = u64::MAX,
            // A duplicate while the earlier order still rests, a reuse after.
            3 | 4 => order_id = self.recent_id(),
            5 => {
                qty = u64::MAX - self.rng.below(4);
                price_tick = match side {
                    Side::Buy => FAR_BID_TICK,
                    Side::Sell => FAR_ASK_TICK,
                };
            }
            6 | 7 => {
                // Sweep every level on the other side, often filling more
                // makers than a bounded batch could report.
                qty = 1 + self.rng.below(2_000);
                price_tick = match side {
                    Side::Buy => MAX_TICKS as u32 - 1,
                    Side::Sell => 0,
                };
            }
            _ => {}
        }
        let order_type = match self.rng.below(100) {
            0..=9 => OrderType::ImmediateOrCancel,
            10..=12 => OrderType::Market,
            _ => OrderType::Limit,
        };
        Op::Place {
            order_id,
            side,
            order_type,
            price_tick,
            qty,
        }
    }

    fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.recent[self.issued % RECENT_WINDOW] = id;
        self.issued += 1;
        id
    }

    fn recent_id(&mut self) -> u64 {
        let known = self.issued.min(RECENT_WINDOW);
        self.recent[self.rng.below(known as u64) as usize]
    }
}

/// A per-process temp file path that is removed when dropped.
pub struct TempFile(pub PathBuf);

impl TempFile {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{name}_{}.bin", std::process::id()));
        // A file left behind by an aborted earlier run would make
        // `WalWriter::create` fail, so start from a clean slate.
        let _ = std::fs::remove_file(&path);
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
