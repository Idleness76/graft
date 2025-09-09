//! Deterministic RNG handle for tests.
//! Provides a deterministic random number generator, wrapping rand for seeded generation.
//! Can be exposed read-only in NodeContext.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Deterministic RNG, wrapping rand::StdRng.
#[derive(Debug)]
pub struct DeterministicRng {
    rng: StdRng,
}

impl DeterministicRng {
    /// Create with a fixed seed for determinism.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Generate a random u64.
    pub fn random_u64(&mut self) -> u64 {
        self.rng.random()
    }

    /// Generate a random string of lowercase letters.
    pub fn random_string(&mut self, len: usize) -> String {
        (0..len)
            .map(|_| (self.rng.random_range(97..123)) as u8 as char)
            .collect()
    }
}
