//! Deterministic RNG (xorshift64*) for noise, grain and property tests.
//!
//! A tiny, seeded, dependency-free generator keeps every noisy operation and
//! every property test reproducible (ADR-0004). Not cryptographic.

/// xorshift64* generator (Vigna 2014).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    /// Seeds from an arbitrary u64 (zero is remapped to a non-zero state).
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    /// Next raw 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    /// Next raw 32-bit value (high half of the 64-bit output).
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Next value in `[0, 255]`.
    pub fn next_u8(&mut self) -> u8 {
        (self.next_u64() >> 56) as u8
    }

    /// Next uniform float in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        // 24 random mantissa bits -> [0,1).
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Next uniform float in `[min, max)`.
    pub fn range_f32(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }

    /// Next integer in `[0, n)`; returns 0 when `n == 0`.
    ///
    /// Lemire's bounded generation with a rejection threshold.
    pub fn range_u32(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // 2^32 mod n, computed without 64-bit division.
        let threshold = n.wrapping_neg() % n;
        loop {
            let x = self.next_u32();
            let m = (x as u64).wrapping_mul(n as u64);
            if (m as u32) >= threshold {
                return (m >> 32) as u32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_for_seed() {
        let mut a = XorShift64::new(42);
        let mut b = XorShift64::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(XorShift64::new(1).next_u64(), XorShift64::new(2).next_u64());
    }

    #[test]
    fn zero_seed_is_valid() {
        let mut a = XorShift64::new(0);
        let mut b = XorShift64::new(0);
        assert_eq!(a.next_u64(), b.next_u64());
        assert_ne!(a.next_u64(), 0);
    }

    #[test]
    fn ranges_are_in_bounds() {
        let mut rng = XorShift64::new(12345);
        for _ in 0..10_000 {
            let f = rng.next_f32();
            assert!((0.0..1.0).contains(&f));
            let r = rng.range_f32(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&r));
            let u = rng.range_u32(7);
            assert!(u < 7);
        }
        assert_eq!(rng.range_u32(0), 0);
        assert_eq!(rng.range_u32(1), 0);
    }
}
