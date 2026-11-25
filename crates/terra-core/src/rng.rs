//! Deterministic pseudo-random numbers.
//!
//! Terra never uses operating-system entropy in simulation paths. Every random
//! decision descends from the world seed through an explicitly derived stream,
//! so adding a new consumer of randomness cannot silently change an unrelated
//! subsystem.
//!
//! The generator is xoshiro256** seeded through SplitMix64. Both are public
//! domain algorithms described by Blackman and Vigna; this is an independent
//! implementation.

/// A counter-based stream identifier. Distinct subsystems use distinct
/// constants so their sequences are independent even with the same world seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stream(pub u64);

impl Stream {
    /// Tectonic plate seeding and motion.
    pub const PLATES: Stream = Stream(0x706c_6174_6573_0001);
    /// Base terrain noise.
    pub const TERRAIN: Stream = Stream(0x7465_7272_6169_6e02);
    /// Erosion and sediment transport.
    pub const EROSION: Stream = Stream(0x6572_6f73_696f_6e08);
    /// Climate noise (per-cell variation).
    pub const CLIMATE: Stream = Stream(0x636c_696d_6174_6503);
    /// Biome and ecosystem variation.
    pub const ECOLOGY: Stream = Stream(0x6563_6f6c_6f67_7904);
    /// Civilization names, traits and decisions.
    pub const HISTORY: Stream = Stream(0x6869_7374_6f72_7905);
    /// Natural catastrophes.
    pub const EVENTS: Stream = Stream(0x6576_656e_7473_0006);
    /// Studio-side cosmetic choices that must not affect the world.
    pub const COSMETIC: Stream = Stream(0x636f_736d_6574_6907);
}

/// SplitMix64, used only to expand a seed into generator state.
#[inline]
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// xoshiro256** generator with a small, copy-friendly state.
#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// Creates a generator from a world seed and an explicit stream.
    pub fn new(seed: u64, stream: Stream) -> Self {
        let mut sm = seed ^ stream.0.rotate_left(17) ^ 0x7465_7272_6100_0000;
        let s = [
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
        ];
        // All-zero state is invalid for xoshiro; SplitMix64 cannot produce it
        // for four consecutive draws in practice, but guard anyway.
        let s = if s == [0, 0, 0, 0] { [1, 2, 3, 4] } else { s };
        Self { s }
    }

    /// Creates a generator from four raw words. Useful for tests and for
    /// deriving state snapshots.
    pub fn from_state(s: [u64; 4]) -> Self {
        Self { s }
    }

    /// Returns the internal state, for checkpointing.
    pub fn state(&self) -> [u64; 4] {
        self.s
    }

    /// Derives a child generator. The child is independent of the parent's
    /// future draws, which keeps subsystem streams isolated.
    pub fn fork(&self, stream: Stream) -> Self {
        let mut sm = self.s[0] ^ self.s[3] ^ stream.0 ^ 0x666f_726b_0000_0001;
        Self {
            s: [
                splitmix64(&mut sm),
                splitmix64(&mut sm),
                splitmix64(&mut sm),
                splitmix64(&mut sm),
            ],
        }
    }

    /// Next 64-bit value.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Next 32-bit value.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform float in `[0, 1)` with 53 bits of precision.
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Uniform float in `[lo, hi)`. Returns `lo` if the range is degenerate.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        if !(lo < hi) {
            return lo;
        }
        lo + (hi - lo) * self.next_f64()
    }

    /// Uniform integer in `[0, n)`. Returns 0 when `n == 0`.
    pub fn below(&mut self, n: u64) -> u64 {
        if n <= 1 {
            return 0;
        }
        // Lemire's multiply-shift rejection method, without division bias.
        let mut x = self.next_u64();
        let mut m = u128::from(x) * u128::from(n);
        let mut low = m as u64;
        if low < n {
            let threshold = n.wrapping_neg() % n;
            while low < threshold {
                x = self.next_u64();
                m = u128::from(x) * u128::from(n);
                low = m as u64;
            }
        }
        (m >> 64) as u64
    }

    /// Uniform integer in `[lo, hi]`. If `hi < lo` the values are swapped.
    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        let (a, b) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let span = (b as i128 - a as i128 + 1) as u128;
        if span == 0 {
            return a;
        }
        let n = if span > u128::from(u64::MAX) {
            u64::MAX
        } else {
            span as u64
        };
        a.saturating_add(self.below(n) as i64)
    }

    /// True with probability `p` (clamped to `[0, 1]`).
    pub fn chance(&mut self, p: f64) -> bool {
        if p <= 0.0 {
            return false;
        }
        if p >= 1.0 {
            return true;
        }
        self.next_f64() < p
    }

    /// Fisher-Yates shuffle. Deterministic for a given stream.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }

    /// Picks one element, or `None` for an empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.below(items.len() as u64) as usize])
        }
    }

    /// Index with a weight proportional to `weights`. Zero/negative/non-finite
    /// weights are ignored. Returns `None` when no positive weight exists.
    pub fn weighted_index(&mut self, weights: &[f64]) -> Option<usize> {
        let mut total = 0.0f64;
        for &w in weights {
            if w.is_finite() && w > 0.0 {
                total += w;
            }
        }
        if !(total > 0.0) {
            return None;
        }
        let mut target = self.next_f64() * total;
        let mut last_positive = None;
        for (i, &w) in weights.iter().enumerate() {
            if !(w.is_finite() && w > 0.0) {
                continue;
            }
            last_positive = Some(i);
            target -= w;
            if target <= 0.0 {
                return Some(i);
            }
        }
        last_positive
    }

    /// Approximately normal sample with mean 0 and standard deviation 1
    /// (Irwin-Hall with 12 uniforms, which is bounded and fast).
    pub fn normal(&mut self) -> f64 {
        let mut sum = 0.0;
        for _ in 0..12 {
            sum += self.next_f64();
        }
        sum - 6.0
    }

    /// Gamma-ish positive noise with unit mean, shaped by `k` (larger is
    /// tighter). Implemented as a product of uniforms to stay branch-free and
    /// bounded. `k` is clamped to a sane range.
    pub fn unit_noise(&mut self, k: u32) -> f64 {
        let k = k.clamp(1, 16);
        let mut p = 1.0;
        for _ in 0..k {
            p *= self.next_f64().max(1e-9);
        }
        // -ln(prod U) is Gamma(k, 1); scale to unit mean.
        let g = -p.ln();
        (g / f64::from(k)).clamp(1e-4, 20.0)
    }
}

/// A small deterministic hash used where a seed must combine with an index,
/// e.g. per-cell noise that should not consume a sequential stream.
#[inline]
pub fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x
}

/// Combines a seed, a stream and a cell index into a well-mixed word.
#[inline]
pub fn mix3(seed: u64, stream: u64, index: u64) -> u64 {
    mix64(seed ^ mix64(stream ^ mix64(index.wrapping_add(0x9e37_79b9_7f4a_7c15))))
}

/// Maps a mixed word to `[0, 1)`.
#[inline]
pub fn unit_from_hash(h: u64) -> f64 {
    (h >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42, Stream::TERRAIN);
        let mut b = Rng::new(42, Stream::TERRAIN);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn streams_are_independent() {
        let mut a = Rng::new(42, Stream::TERRAIN);
        let mut b = Rng::new(42, Stream::CLIMATE);
        let mut equal = 0;
        for _ in 0..64 {
            if a.next_u64() == b.next_u64() {
                equal += 1;
            }
        }
        assert!(equal < 3);
    }

    #[test]
    fn seeds_differ() {
        let mut a = Rng::new(1, Stream::PLATES);
        let mut b = Rng::new(2, Stream::PLATES);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn floats_are_in_unit_interval() {
        let mut r = Rng::new(7, Stream::ECOLOGY);
        for _ in 0..10_000 {
            let f = r.next_f64();
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn range_respects_bounds() {
        let mut r = Rng::new(9, Stream::EVENTS);
        for _ in 0..1000 {
            let f = r.range(-3.5, 8.25);
            assert!((-3.5..8.25).contains(&f));
        }
        assert_eq!(r.range(2.0, 2.0), 2.0);
        assert_eq!(r.range(3.0, 1.0), 3.0);
    }

    #[test]
    fn below_is_bounded_and_covers() {
        let mut r = Rng::new(11, Stream::HISTORY);
        let mut seen = [false; 7];
        for _ in 0..5000 {
            let v = r.below(7) as usize;
            assert!(v < 7);
            seen[v] = true;
        }
        assert!(seen.iter().all(|&s| s));
        assert_eq!(r.below(0), 0);
        assert_eq!(r.below(1), 0);
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut r = Rng::new(13, Stream::HISTORY);
        let mut v: Vec<u32> = (0..64).collect();
        r.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..64).collect::<Vec<_>>());
        assert_ne!(v, (0..64).collect::<Vec<_>>());
    }

    #[test]
    fn weighted_index_ignores_bad_weights() {
        let mut r = Rng::new(17, Stream::ECOLOGY);
        assert_eq!(r.weighted_index(&[]), None);
        assert_eq!(r.weighted_index(&[0.0, -1.0, f64::NAN]), None);
        let mut counts = [0u32; 3];
        for _ in 0..3000 {
            let i = r.weighted_index(&[1.0, 2.0, 0.0]).unwrap();
            counts[i] += 1;
        }
        assert_eq!(counts[2], 0);
        assert!(counts[1] > counts[0]);
    }

    #[test]
    fn fork_does_not_replay_parent() {
        let mut parent = Rng::new(21, Stream::PLATES);
        let mut child = parent.fork(Stream::TERRAIN);
        assert_ne!(parent.next_u64(), child.next_u64());
        assert_eq!(parent.state(), parent.clone().state());
    }

    #[test]
    fn normal_is_centered() {
        let mut r = Rng::new(23, Stream::CLIMATE);
        let n = 20_000;
        let mut sum = 0.0;
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        for _ in 0..n {
            let v = r.normal();
            sum += v;
            min = min.min(v);
            max = max.max(v);
        }
        let mean = sum / f64::from(n);
        assert!(mean.abs() < 0.1, "mean {mean}");
        assert!(min >= -6.0 && max <= 6.0);
    }

    #[test]
    fn unit_noise_stays_positive() {
        let mut r = Rng::new(29, Stream::ECOLOGY);
        for _ in 0..1000 {
            let v = r.unit_noise(4);
            assert!(v.is_finite() && v > 0.0);
        }
    }

    #[test]
    fn splitmix_changes_state() {
        let mut s = 0;
        let a = splitmix64(&mut s);
        let b = splitmix64(&mut s);
        assert_ne!(a, b);
        assert_ne!(s, 0);
    }

    #[test]
    fn mix_helpers_are_stable() {
        assert_eq!(mix64(0), 0);
        assert_eq!(mix3(1, 2, 3), mix3(1, 2, 3));
        assert_ne!(mix3(1, 2, 3), mix3(1, 2, 4));
        let u = unit_from_hash(mix3(5, 5, 5));
        assert!((0.0..1.0).contains(&u));
    }
}
