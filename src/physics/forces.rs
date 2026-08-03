//! Lennard-Jones potential: the fundamental intermolecular interaction.
//!
//! It is the only "chemistry" of the universe. Each element contributes a `σ`
//! (range) and an `ε` (depth of the potential well, in kelvin). For a pair of
//! different types the **Lorentz–Berthelot** mixing rules are used:
//!
//! ```text
//! σ_ij = (σ_i + σ_j) / 2        ε_ij = √(ε_i · ε_j)
//! ```
//!
//! From this single potential emerge short-range repulsion, Van der Waals
//! attraction and, with it, condensation, aggregates and any structure that
//! appears. Nothing is programmed as a "molecule".
//!
//! ## Units
//!
//! `σ` is in simulation units (the same scale as position and mass); `ε` is
//! expressed in kelvin and converted to simulation energy with the thermal
//! constant of the configuration (`k`). Thus `k·T/ε = T/ε_k`: the critical
//! temperature of an element is ≈ `1.31·ε_k`.

use crate::components::AtomType;
use crate::math::Vec3;

/// Lennard-Jones parameters of an element.
#[derive(Debug, Clone, Copy)]
pub struct LjElement {
    /// Range of the potential (simulation units).
    pub sigma: f64,
    /// Depth of the well in kelvin.
    pub epsilon_k: f64,
}

/// Element table: `σ` in simulation units, `ε` in kelvin.
/// Values of the order of the real ones for simple gases (relative to each
/// other).
const ELEMENTS: [LjElement; 6] = [
    LjElement { sigma: 1.6, epsilon_k: 12.0 }, // H
    LjElement { sigma: 1.4, epsilon_k: 8.0 },  // He
    LjElement { sigma: 1.9, epsilon_k: 80.0 }, // C
    LjElement { sigma: 1.7, epsilon_k: 40.0 }, // N
    LjElement { sigma: 1.65, epsilon_k: 55.0 }, // O
    LjElement { sigma: 2.0, epsilon_k: 110.0 }, // Na
];

/// Cutoff distance in multiples of `σ`: beyond it there is no interaction.
pub const LJ_CUTOFF_FACTOR: f64 = 2.5;

/// Hardened nucleus: below `r_min = NUCLEUS · σ` the potential is evaluated
/// at `r_min` (avoids the `r→0` divergence with finite `dt`).
pub const LJ_NUCLEUS: f64 = 0.5;

fn element_index(t: AtomType) -> usize {
    match t {
        AtomType::Hydrogen => 0,
        AtomType::Helium => 1,
        AtomType::Carbon => 2,
        AtomType::Nitrogen => 3,
        AtomType::Oxygen => 4,
        AtomType::Sodium => 5,
    }
}

/// Range `σ` of an element (simulation units).
pub fn sigma(t: AtomType) -> f64 {
    ELEMENTS[element_index(t)].sigma
}

/// Depth `ε` of an element (kelvin).
pub fn epsilon(t: AtomType) -> f64 {
    ELEMENTS[element_index(t)].epsilon_k
}

/// Parameters of a pair of types (`ε` already in energy units).
#[derive(Debug, Clone, Copy)]
pub struct LjPair {
    pub sigma: f64,
    pub epsilon: f64,
}

/// Pair table (6×6, symmetric) with the system cutoff.
pub struct LjTable {
    pair: [[LjPair; 6]; 6],
    rc: f64,
    r_on: f64,
}

impl LjTable {
    /// Builds the table with the mixing rules and prepares the smooth switch
    /// between `r_on` and `rc`.
    pub fn new(thermal_constant: f64, cutoff_factor: f64) -> Self {
        let mut max_sigma = 0.0f64;
        let mut pair = [[LjPair { sigma: 1.0, epsilon: 0.0 }; 6]; 6];
        for i in 0..6 {
            for j in 0..6 {
                let a = ELEMENTS[i];
                let b = ELEMENTS[j];
                let sigma = 0.5 * (a.sigma + b.sigma);
                let epsilon = thermal_constant * (a.epsilon_k * b.epsilon_k).sqrt();
                max_sigma = max_sigma.max(sigma);
                pair[i][j] = LjPair { sigma, epsilon };
            }
        }
        let rc = cutoff_factor.max(1.0) * max_sigma;
        let r_on = 0.9 * rc;
        Self { pair, rc, r_on }
    }

    /// Cutoff distance of the system.
    pub fn rc(&self) -> f64 {
        self.rc
    }

    /// Parameters of the pair `(a, b)`.
    pub fn pair(&self, a: AtomType, b: AtomType) -> LjPair {
        self.pair[element_index(a)][element_index(b)]
    }

    /// Parameters of the pair by element index.
    pub fn pair_indexed(&self, i: usize, j: usize) -> LjPair {
        self.pair[i][j]
    }

    /// Force (on `a`, along `normal` pointing from `b` towards `a`) and
    /// potential contribution, with the potential truncated and *switched* so
    /// that both energy and force tend smoothly to 0 at `rc`.
    ///
    /// `r` must be `< rc`; below `LJ_NUCLEUS·σ` it is evaluated at that point
    /// (hardened nucleus).
    #[inline]
    pub fn force_switched(&self, p: LjPair, r: f64, normal: Vec3) -> (Vec3, f64) {
        let r = r.max(LJ_NUCLEUS * p.sigma);
        let s = p.sigma / r;
        let s6 = s * s * s * s * s * s;
        let s12 = s6 * s6;

        // LJ potential (not shifted) and base force magnitude along `normal`
        // (= −dV/dr).
        let v = 4.0 * p.epsilon * (s12 - s6);
        let m = (24.0 * p.epsilon / p.sigma) * s * (2.0 * s12 - s6);

        let (sw, dsw) = self.switch(r);
        // F_ef = −d(V·sw)/dr = m·sw − V·(dsw/dr)
        let f_mag = m * sw - v * dsw;
        (normal * f_mag, v * sw)
    }

    /// Smoothing factor `[0,1]` and its derivative with respect to `r`.
    ///
    /// Quintic polynomial (standard smooth function of molecular dynamics):
    /// it is 1 up to `r_on` and falls to 0 at `rc` with zero first and second
    /// derivatives at both ends.
    #[inline]
    fn switch(&self, r: f64) -> (f64, f64) {
        if r <= self.r_on {
            return (1.0, 0.0);
        }
        let u = (r - self.r_on) / (self.rc - self.r_on);
        let u2 = u * u;
        let u3 = u2 * u;
        let u4 = u3 * u;
        let u5 = u4 * u;
        let sw = 1.0 - 10.0 * u3 + 15.0 * u4 - 6.0 * u5;
        let dsdu = -30.0 * u2 * (1.0 - u) * (1.0 - u);
        (sw, dsdu / (self.rc - self.r_on))
    }
}

/// LJ force and potential **without** switch, for tests of the potential
/// shape.
#[inline]
pub fn lj_raw(p: LjPair, r: f64) -> (f64, f64) {
    let s = p.sigma / r;
    let s6 = s * s * s * s * s * s;
    let s12 = s6 * s6;
    let v = 4.0 * p.epsilon * (s12 - s6);
    let m = (24.0 * p.epsilon / p.sigma) * s * (2.0 * s12 - s6);
    (m, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> LjTable {
        LjTable::new(0.01, LJ_CUTOFF_FACTOR)
    }

    #[test]
    fn potential_minimum_at_sigma_root_sixth_of_2() {
        // The minimum of V(r) = 4ε[(σ/r)¹² − (σ/r)⁶] is at r = σ·2^(1/6),
        // with V = −ε.
        let t = table();
        let p = t.pair(AtomType::Hydrogen, AtomType::Hydrogen);
        let r_min = p.sigma * 2.0f64.powf(1.0 / 6.0);
        let (_, v) = lj_raw(p, r_min);
        assert!((v + p.epsilon).abs() < 1e-12 * p.epsilon.max(1.0));
        let (m, _) = lj_raw(p, r_min);
        assert!(m.abs() < 1e-9, "the force at the minimum must vanish");
    }

    #[test]
    fn repulsive_and_attractive_force() {
        // normal = +x (from b towards a). Inside σ the force pushes (repulsion);
        // outside it attracts (m < 0 along +x).
        let t = table();
        let p = t.pair(AtomType::Carbon, AtomType::Carbon);
        let (m_rep, _) = lj_raw(p, 0.8 * p.sigma);
        assert!(m_rep > 0.0, "expected repulsion inside σ");
        let (m_att, _) = lj_raw(p, 1.5 * p.sigma);
        assert!(m_att < 0.0, "expected attraction outside σ");
        // The zero crossing coincides with the minimum of the well.
        let (m0, _) = lj_raw(p, p.sigma * 2.0f64.powf(1.0 / 6.0));
        assert!(m0.abs() < 1e-9);
    }

    #[test]
    fn mixing_rules_are_symmetric() {
        let t = table();
        let h = t.pair(AtomType::Hydrogen, AtomType::Carbon);
        let c = t.pair(AtomType::Carbon, AtomType::Hydrogen);
        assert_eq!(h.sigma, c.sigma);
        assert_eq!(h.epsilon, c.epsilon);
        // ε_ij between the extremes: √(ε_H·ε_C).
        let expected = 0.01 * (12.0f64 * 80.0).sqrt();
        assert!((h.epsilon - expected).abs() < 1e-12);
    }

    #[test]
    fn switch_vanishes_at_cutoff() {
        let t = table();
        let p = t.pair(AtomType::Oxygen, AtomType::Oxygen);
        let r = t.rc();
        assert!(r > p.sigma);
        let (f, v) = t.force_switched(p, r, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(f, Vec3::ZERO);
        assert_eq!(v, 0.0);
        // Inside r_on there is no smoothing.
        let r_in = 0.8 * t.r_on;
        let (f_in, _) = t.force_switched(p, r_in, Vec3::new(1.0, 0.0, 0.0));
        let (m, _) = lj_raw(p, r_in);
        assert!((f_in.x - m).abs() < 1e-12);
    }
}
