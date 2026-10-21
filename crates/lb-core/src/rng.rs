//! Small deterministic RNG (SplitMix64). Planning must be reproducible so the
//! same inputs always produce the same rundown preview.

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// Derive a seed from a base seed and arbitrary string parts.
    pub fn derive(seed: u64, parts: &[&str]) -> Self {
        let mut h = seed ^ 0xcbf2_9ce4_8422_2325;
        for p in parts {
            for b in p.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            h ^= 0xff;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        Rng::new(h)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }

    /// Pick an index with probability proportional to `weights`.
    pub fn weighted(&mut self, weights: &[f64]) -> Option<usize> {
        let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
        if total <= 0.0 {
            return None;
        }
        let mut x = self.next_f64() * total;
        for (i, w) in weights.iter().enumerate() {
            if *w <= 0.0 {
                continue;
            }
            if x < *w {
                return Some(i);
            }
            x -= w;
        }
        weights.iter().rposition(|w| *w > 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic() {
        let mut a = Rng::derive(1, &["x"]);
        let mut b = Rng::derive(1, &["x"]);
        assert_eq!(a.next_u64(), b.next_u64());
        let mut c = Rng::derive(1, &["y"]);
        assert_ne!(Rng::derive(1, &["x"]).next_u64(), c.next_u64());
    }
    #[test]
    fn weighted_respects_zero() {
        let mut r = Rng::new(5);
        for _ in 0..100 {
            assert_eq!(r.weighted(&[0.0, 1.0, 0.0]), Some(1));
        }
        assert_eq!(r.weighted(&[0.0]), None);
    }
}
