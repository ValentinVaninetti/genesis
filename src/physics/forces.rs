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
    /// Net charge in units of elementary charge (electrostatics law). Zero by
    /// default: charge is a law constant of the species, not a programmed bond.
    pub charge: f64,
}

/// Element table: `σ` in simulation units, `ε` in kelvin, `charge` in
/// elementary charges.
/// Values of the order of the real ones for simple gases (relative to each
/// other). Charges follow ionization trends: metals donate, oxygen accepts,
/// the noble/neutral gases carry none.
const ELEMENTS: [LjElement; AtomType::COUNT] = [
    LjElement { sigma: 1.6, epsilon_k: 12.0, charge: 0.0 },   // H
    LjElement { sigma: 1.4, epsilon_k: 8.0, charge: 0.0 },    // He
    LjElement { sigma: 1.9, epsilon_k: 80.0, charge: 0.0 },   // C
    LjElement { sigma: 1.7, epsilon_k: 40.0, charge: 0.0 },   // N
    LjElement { sigma: 1.65, epsilon_k: 55.0, charge: -1.0 }, // O
    LjElement { sigma: 2.0, epsilon_k: 110.0, charge: 1.0 },  // Na
    LjElement { sigma: 2.3, epsilon_k: 200.0, charge: 0.5 },  // Si
    LjElement { sigma: 2.2, epsilon_k: 150.0, charge: 0.0 },  // P
    LjElement { sigma: 2.1, epsilon_k: 130.0, charge: 0.0 },  // S
    LjElement { sigma: 1.9, epsilon_k: 350.0, charge: 1.0 },  // Fe
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
        AtomType::Silicon => 6,
        AtomType::Phosphorus => 7,
        AtomType::Sulfur => 8,
        AtomType::Iron => 9,
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

/// Net charge of an element (elementary charges).
pub fn charge(t: AtomType) -> f64 {
    ELEMENTS[element_index(t)].charge
}

/// Lorentz–Berthelot mixing of σ for a pair (same rule as `LjTable::new`).
pub fn mix_sigma(a: AtomType, b: AtomType) -> f64 {
    0.5 * (sigma(a) + sigma(b))
}

/// Lorentz–Berthelot mixing of ε for a pair, in energy units.
pub fn mix_epsilon(thermal_constant: f64, a: AtomType, b: AtomType) -> f64 {
    thermal_constant * (epsilon(a) * epsilon(b)).sqrt()
}

/// Second derivative of the LJ potential at its minimum `r_m = σ·2^(1/6)`:
/// the effective spring constant of the well. One line from the analytic
/// form: `V''(r_m) = 72·2^(−1/3)·ε/σ²`.
pub fn well_curvature(epsilon: f64, sigma: f64) -> f64 {
    72.0 * 2.0f64.powf(-1.0 / 3.0) * epsilon / (sigma * sigma)
}

/// Oscillation period of a pair at the bottom of the well, in simulation time
/// units: `T = 2π·√(μ/k)`, with `μ` the reduced mass and `k` the well
/// curvature. The same units as the dynamics (mass in amu, ε in energy).
pub fn vib_period(epsilon: f64, sigma: f64, reduced_mass: f64) -> f64 {
    2.0 * std::f64::consts::PI * (reduced_mass / well_curvature(epsilon, sigma)).sqrt()
}

/// Parameters of a pair of types (`ε` already in energy units).
#[derive(Debug, Clone, Copy)]
pub struct LjPair {
    pub sigma: f64,
    pub epsilon: f64,
}

/// Pair table (COUNT×COUNT, symmetric) with the system cutoff.
pub struct LjTable {
    pair: [[LjPair; AtomType::COUNT]; AtomType::COUNT],
    rc: f64,
    r_on: f64,
}

impl LjTable {
    /// Builds the table with the mixing rules and prepares the smooth switch
    /// between `r_on` and `rc`.
    pub fn new(thermal_constant: f64, cutoff_factor: f64) -> Self {
        let n = AtomType::COUNT;
        let mut max_sigma = 0.0f64;
        let mut pair = [[LjPair { sigma: 1.0, epsilon: 0.0 }; AtomType::COUNT]; AtomType::COUNT];
        for i in 0..n {
            for j in 0..n {
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
        let (m, v) = lj_raw(p, r);
        let (sw, dsw) = smooth_cutoff(r, self.r_on, self.rc);
        let (f_mag, v_sw) = switched(m, v, sw, dsw);
        (normal * f_mag, v_sw)
    }
}

/// Smoothing factor `[0,1]` and its derivative with respect to `r`.
///
/// Quintic polynomial (standard smooth function of molecular dynamics): it is
/// 1 up to `r_on` and falls to 0 at `rc` with zero first and second
/// derivatives at both ends. Any pair term truncated at `rc` uses it, so the
/// total force stays continuous and the energy conserved.
#[inline]
pub fn smooth_cutoff(r: f64, r_on: f64, rc: f64) -> (f64, f64) {
    if r <= r_on {
        return (1.0, 0.0);
    }
    let u = (r - r_on) / (rc - r_on);
    let u2 = u * u;
    let u3 = u2 * u;
    let u4 = u3 * u;
    let u5 = u4 * u;
    let sw = 1.0 - 10.0 * u3 + 15.0 * u4 - 6.0 * u5;
    let dsdu = -30.0 * u2 * (1.0 - u) * (1.0 - u);
    (sw, dsdu / (rc - r_on))
}

/// Applies the smooth truncation to a raw pair term: `F = f_raw·sw − V·(dsw)`
/// (chain rule of `−d(V·sw)/dr`) and `V = v_raw·sw`.
#[inline]
pub fn switched(f_raw: f64, v_raw: f64, sw: f64, dsw: f64) -> (f64, f64) {
    (f_raw * sw - v_raw * dsw, v_raw * sw)
}

/// Raw Coulomb term for two charges (magnitude of the force along `normal`,
/// and the potential): `F = k_e·q_a·q_b/r²`, `V = k_e·q_a·q_b/r`. Opposite
/// charges attract (negative force along `normal`), like charges repel.
#[inline]
pub fn coulomb_raw(k_e: f64, qa: f64, qb: f64, r: f64) -> (f64, f64) {
    let q = k_e * qa * qb;
    (q / (r * r), q / r)
}

/// Raw gravitational term for two masses: `F = −G·m_a·m_b/r²`,
/// `V = −G·m_a·m_b/r`. Always attractive.
#[inline]
pub fn gravity_raw(g: f64, ma: f64, mb: f64, r: f64) -> (f64, f64) {
    let m = g * ma * mb;
    (-m / (r * r), -m / r)
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

    #[test]
    fn well_curvature_matches_numeric_second_derivative() {
        let eps = 0.8;
        let s = 1.9;
        let r_m = s * 2.0f64.powf(1.0 / 6.0);
        let h = 1e-5;
        let v = |r: f64| lj_raw(LjPair { sigma: s, epsilon: eps }, r).1;
        let numeric = (v(r_m + h) + v(r_m - h) - 2.0 * v(r_m)) / (h * h);
        assert!(
            (numeric - well_curvature(eps, s)).abs() / numeric < 1e-4,
            "analytic curvature {} != numeric {numeric}",
            well_curvature(eps, s)
        );
    }

    #[test]
    fn vib_period_is_physically_ordered() {
        // Carbon–Carbon in the default units: a few sim-time units per
        // vibration (~260 ticks at dt = 1/60), i.e. a handful of ticks per
        // oscillation — the scale against which "persistent" should be
        // measured.
        let mu = AtomType::Carbon.mass() / 2.0;
        let eps = mix_epsilon(0.01, AtomType::Carbon, AtomType::Carbon);
        let sig = mix_sigma(AtomType::Carbon, AtomType::Carbon);
        let t_cc = vib_period(eps, sig, mu);
        assert!((1.0..20.0).contains(&t_cc), "period {t_cc:.2} out of range");

        // The periods of very different pairs land in the same order of
        // magnitude (heavy/deep compensates light/shallow), so they are
        // comparable as a criterion; assert the scale holds across species
        // and that pairs genuinely differ.
        let t_hh = vib_period(
            mix_epsilon(0.01, AtomType::Hydrogen, AtomType::Hydrogen),
            mix_sigma(AtomType::Hydrogen, AtomType::Hydrogen),
            AtomType::Hydrogen.mass() / 2.0,
        );
        let t_fefe = vib_period(
            mix_epsilon(0.01, AtomType::Iron, AtomType::Iron),
            mix_sigma(AtomType::Iron, AtomType::Iron),
            AtomType::Iron.mass() / 2.0,
        );
        assert!((1.0..20.0).contains(&t_hh));
        assert!((1.0..20.0).contains(&t_fefe));
        assert!((t_cc - t_hh).abs() > 0.5, "C–C and H–H should differ");
    }

    #[test]
    fn smooth_cutoff_is_continuous_and_vanishes_at_cutoff() {
        let (r_on, rc) = (2.0, 3.0);
        let (sw0, dsw0) = smooth_cutoff(r_on, r_on, rc);
        assert_eq!(sw0, 1.0);
        assert_eq!(dsw0, 0.0);
        let (sw1, dsw1) = smooth_cutoff(rc, r_on, rc);
        assert!(sw1.abs() < 1e-12);
        assert!(dsw1.abs() < 1e-12);
        // Monotone in between.
        let (mid, _) = smooth_cutoff(2.5, r_on, rc);
        assert!(mid > 0.0 && mid < 1.0);
    }

    #[test]
    fn coulomb_signs_and_potential() {
        // Opposite charges attract: force along `normal` is negative.
        let (f, v) = coulomb_raw(1.0, 1.0, -1.0, 2.0);
        assert!(f < 0.0 && v < 0.0);
        assert!((f - (-0.25)).abs() < 1e-12);
        assert!((v - (-0.5)).abs() < 1e-12);
        // Like charges repel.
        let (f2, _) = coulomb_raw(1.0, 1.0, 1.0, 2.0);
        assert!(f2 > 0.0);
        // 1/r² scaling.
        let (f3, _) = coulomb_raw(1.0, 1.0, 1.0, 4.0);
        assert!((f3 - 0.0625).abs() < 1e-12);
    }

    #[test]
    fn gravity_is_always_attractive() {
        let (f, v) = gravity_raw(1.0, 2.0, 3.0, 2.0);
        assert!(f < 0.0 && v < 0.0);
        assert!((f - (-1.5)).abs() < 1e-12);
        assert!((v - (-3.0)).abs() < 1e-12);
    }

    #[test]
    fn element_charges_follow_ionization_trends() {
        assert_eq!(charge(AtomType::Sodium), 1.0);
        assert_eq!(charge(AtomType::Oxygen), -1.0);
        assert_eq!(charge(AtomType::Hydrogen), 0.0);
        assert_eq!(charge(AtomType::Helium), 0.0);
    }
}
