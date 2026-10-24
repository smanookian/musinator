//! Small deterministic RNG (SplitMix64). Same seed, same library, forever.

#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x6d75_7369_6e61_746f)
    }

    /// A child generator; consuming one value keeps the parent stream stable.
    pub fn fork(&mut self) -> Rng {
        Rng(self.next_u64())
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [-1, 1).
    pub fn bipolar(&mut self) -> f32 {
        (self.f64() * 2.0 - 1.0) as f32
    }

    /// Uniform in [0, n).
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        (((self.next_u64() >> 32) * n as u64) >> 32) as usize
    }

    /// Uniform in [lo, hi] (inclusive).
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + self.below((hi - lo + 1) as usize) as u32
    }

    pub fn range_f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// Index picked by weight.
    pub fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u32 = weights.iter().sum();
        let mut x = self.below(total as usize) as u32;
        for (i, &w) in weights.iter().enumerate() {
            if x < w {
                return i;
            }
            x -= w;
        }
        weights.len() - 1
    }
}
