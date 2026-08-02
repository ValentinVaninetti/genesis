//! Elastic collision between two particles.
//!
//! Pure function: given the unit normal between two particles and their
//! masses and velocities (before the collision), returns the velocity change
//! of each one. Conserves momentum and energy exactly (coefficient of
//! restitution `e = 1`). Everything else (detection, application) lives
//! outside this module.

use crate::math::Vec3;

/// Elastic impulse between two particles.
///
/// - `n` must be a unit vector pointing from particle 2 towards particle 1.
/// - Returns `(Δv1, Δv2)`. If the particles are separating (`vrel·n ≥ 0`)
///   or the masses are not positive, there is no impulse and it returns zeros.
pub fn elastic_pair(m1: f64, v1: Vec3, m2: f64, v2: Vec3, n: Vec3) -> (Vec3, Vec3) {
    if m1 <= 0.0 || m2 <= 0.0 {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    let vrel = v1 - v2;
    let vn = vrel.dot(n);
    if vn >= 0.0 {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    // e = 1: the scalar impulse j = -2·vn / (1/m1 + 1/m2) is positive here.
    let j = -2.0 * vn / (1.0 / m1 + 1.0 / m2);
    let jv = n * j;
    (jv / m1, jv / m2 * -1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_masses_swap_velocities() {
        // The normal points from particle 2 (right) towards particle 1 (left).
        let n = Vec3::new(-1.0, 0.0, 0.0);
        let v1 = Vec3::new(1.0, 0.0, 0.0);
        let v2 = Vec3::new(-1.0, 0.0, 0.0);
        let (d1, d2) = elastic_pair(1.0, v1, 1.0, v2, n);
        assert_eq!(d1, Vec3::new(-2.0, 0.0, 0.0));
        assert_eq!(d2, Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(v1 + d1, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(v2 + d2, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn conserves_momentum_and_energy() {
        let m1 = 1.0;
        let m2 = 3.0;
        let v1 = Vec3::new(2.0, -1.0, 0.5);
        let v2 = Vec3::new(0.0, 1.0, -0.5);
        // vrel·n = -2 < 0: they approach (the normal points towards particle 1).
        let n = Vec3::new(-1.0, 0.0, 0.0);

        let (d1, d2) = elastic_pair(m1, v1, m2, v2, n);
        let v1p = v1 + d1;
        let v2p = v2 + d2;

        // The impulse actually happens (not the trivial separating case).
        assert_ne!(d1, Vec3::ZERO);

        // Total momentum conserved.
        let p_before = v1 * m1 + v2 * m2;
        let p_after = v1p * m1 + v2p * m2;
        assert!((p_before - p_after).length() < 1e-12);

        // Total kinetic energy conserved.
        let e_before = 0.5 * m1 * v1.length_squared() + 0.5 * m2 * v2.length_squared();
        let e_after = 0.5 * m1 * v1p.length_squared() + 0.5 * m2 * v2p.length_squared();
        assert!((e_before - e_after).abs() < 1e-12);
    }

    #[test]
    fn separating_does_not_impulse() {
        // vrel·n > 0: already separating, no interaction.
        let (d1, d2) = elastic_pair(
            1.0,
            Vec3::new(1.0, 0.0, 0.0),
            1.0,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
        );
        assert_eq!(d1, Vec3::ZERO);
        assert_eq!(d2, Vec3::ZERO);
    }

    #[test]
    fn zero_mass_does_not_impulse() {
        let (d1, d2) = elastic_pair(0.0, Vec3::new(1.0, 0.0, 0.0), 1.0, Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(d1, Vec3::ZERO);
        assert_eq!(d2, Vec3::ZERO);
    }
}
