//! RNG module of the universe.
//!
//! Every random number of the simulation must go through this single point.
//! The RNG is *seedable* and *serializable*: it is part of the universe
//! state, so a saved simulation can resume with identical deterministic
//! results.

use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use serde::{Deserialize, Serialize};

/// Randomness source of the simulation.
///
/// `Xoshiro256PlusPlus` is fast and — key for persistence — serializable
/// (feature `serde1`), so a saved universe resumes exactly the same random
/// sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rng {
    inner: Xoshiro256PlusPlus,
}

impl Rng {
    /// Creates an RNG from an integer seed.
    pub fn new(seed: u64) -> Self {
        Self {
            inner: Xoshiro256PlusPlus::seed_from_u64(seed),
        }
    }

    /// A uniform float in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        rand::Rng::gen_range(&mut self.inner, 0.0..1.0)
    }

    /// A uniform float in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// A uniform integer in `[lo, hi]` (inclusive).
    pub fn int(&mut self, lo: u64, hi: u64) -> u64 {
        rand::Rng::gen_range(&mut self.inner, lo..=hi)
    }

    /// A sample of the standard normal distribution (Box-Muller).
    ///
    /// Used for the Maxwell-Boltzmann seeding: if each velocity component is
    /// `N(0, σ)` with `σ = √(k·T/m)`, the speed `|v|` follows the
    /// Maxwell-Boltzmann distribution.
    pub fn gaussian(&mut self) -> f64 {
        let u1 = self.unit().max(f64::MIN_POSITIVE);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    /// A vector uniformly distributed inside a box.
    pub fn in_box(&mut self, half_extents: crate::math::Vec3) -> crate::math::Vec3 {
        crate::math::Vec3::new(
            self.range(-half_extents.x, half_extents.x),
            self.range(-half_extents.y, half_extents.y),
            self.range(-half_extents.z, half_extents.z),
        )
    }

    /// Direct access to the underlying generator for advanced uses.
    pub fn as_rand(&mut self) -> &mut dyn rand::RngCore {
        &mut self.inner
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new(0x_5EED_0A11_CAFE_F00D)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_is_correct() {
        let mut r = Rng::new(42);
        for _ in 0..1000 {
            let v = r.unit();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn deterministic_with_same_seed() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.unit(), b.unit());
        }
    }
}
