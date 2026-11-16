//! Seeded, portable randomness. Every random draw in Tarafold goes through [`Rng`].

use rand_chacha::ChaCha8Rng;
use rand_core::{Rng as _, SeedableRng};

/// Deterministic random number generator derived from a run seed and a stream path.
pub struct Rng(ChaCha8Rng);

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    /// Generator for `seed` and a stream path such as `[STAGE_SPLIT]` or `[island, epoch]`.
    /// Different paths give independent streams; the same path always gives the same stream.
    pub fn derive(seed: u64, path: &[u64]) -> Self {
        let mut state = seed;
        for &p in path {
            state = splitmix64(&mut state) ^ p.wrapping_mul(0xD1B5_4A32_D192_ED03);
        }
        let mut bytes = [0u8; 32];
        for chunk in bytes.chunks_mut(8) {
            chunk.copy_from_slice(&splitmix64(&mut state).to_le_bytes());
        }
        Self(ChaCha8Rng::from_seed(bytes))
    }

    /// Next raw 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// Uniform in `[0, 1)` with 53 bits of precision.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform()
    }

    /// Uniform integer in `[0, n)`. Returns 0 when `n == 0`.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        // Lemire's method without the rejection step is biased by < 2^-64 * n; acceptable here.
        ((u128::from(self.next_u64()) * n as u128) >> 64) as usize
    }

    /// Standard normal draw (Box–Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = 1.0 - self.uniform(); // (0, 1]
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    /// Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

/// Stream identifiers for top-level stages, so stages never share random streams.
pub mod stream {
    /// Train/validation/test split.
    pub const SPLIT: u64 = 1;
    /// Row subsampling for discovery.
    pub const SUBSAMPLE: u64 = 2;
    /// Discovery search.
    pub const DISCOVER: u64 = 3;
    /// Optimization search.
    pub const OPTIMIZE: u64 = 4;
    /// Synthetic data generation.
    pub const SYNTH: u64 = 5;
    /// Bootstrap resampling.
    pub const BOOTSTRAP: u64 = 6;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_path_same_stream() {
        let a: Vec<u64> = (0..5).map(|_| Rng::derive(42, &[1, 2]).next_u64()).collect();
        assert!(a.windows(2).all(|w| w[0] == w[1]));
        let mut r1 = Rng::derive(42, &[1]);
        let mut r2 = Rng::derive(42, &[2]);
        assert_ne!(r1.next_u64(), r2.next_u64());
    }

    #[test]
    fn uniform_and_normal_moments() {
        let mut r = Rng::derive(7, &[]);
        let n = 200_000;
        let u: Vec<f64> = (0..n).map(|_| r.uniform()).collect();
        let mean = u.iter().sum::<f64>() / n as f64;
        assert!((mean - 0.5).abs() < 0.005);
        let z: Vec<f64> = (0..n).map(|_| r.normal()).collect();
        let m = z.iter().sum::<f64>() / n as f64;
        let v = z.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n as f64;
        assert!(m.abs() < 0.01 && (v - 1.0).abs() < 0.02);
    }
}
