//! Colisión elástica entre dos partículas.
//!
//! Función pura: dada la normal unitaria entre dos partículas y sus masas y
//! velocidades (antes de la colisión), devuelve el cambio de velocidad de cada
//! una. Conserva momento y energía exactamente (coeficiente de restitución
//! `e = 1`). Todo el resto (detección, aplicación) vive fuera de este módulo.

use crate::math::Vec3;

/// Impulso elástico entre dos partículas.
///
/// - `n` debe ser un versor que apunta de la partícula 2 hacia la 1.
/// - Devuelve `(Δv1, Δv2)`. Si las partículas se están alejando (`vrel·n ≥ 0`)
///   o las masas no son positivas, no hay impulso y devuelve ceros.
pub fn elastic_pair(m1: f64, v1: Vec3, m2: f64, v2: Vec3, n: Vec3) -> (Vec3, Vec3) {
    if m1 <= 0.0 || m2 <= 0.0 {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    let vrel = v1 - v2;
    let vn = vrel.dot(n);
    if vn >= 0.0 {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    // e = 1: el impulso escalar j = -2·vn / (1/m1 + 1/m2) es positivo aquí.
    let j = -2.0 * vn / (1.0 / m1 + 1.0 / m2);
    let jv = n * j;
    (jv / m1, jv / m2 * -1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masas_iguales_intercambian_velocidades() {
        // La normal apunta de la partícula 2 (derecha) hacia la 1 (izquierda).
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
    fn conserva_momento_y_energia() {
        let m1 = 1.0;
        let m2 = 3.0;
        let v1 = Vec3::new(2.0, -1.0, 0.5);
        let v2 = Vec3::new(0.0, 1.0, -0.5);
        // vrel·n = -2 < 0: se acercan (la normal apunta hacia la partícula 1).
        let n = Vec3::new(-1.0, 0.0, 0.0);

        let (d1, d2) = elastic_pair(m1, v1, m2, v2, n);
        let v1p = v1 + d1;
        let v2p = v2 + d2;

        // El impulso realmente ocurre (no es el caso trivial de separación).
        assert_ne!(d1, Vec3::ZERO);

        // Momento total conservado.
        let p_before = v1 * m1 + v2 * m2;
        let p_after = v1p * m1 + v2p * m2;
        assert!((p_before - p_after).length() < 1e-12);

        // Energía cinética total conservada.
        let e_before = 0.5 * m1 * v1.length_squared() + 0.5 * m2 * v2.length_squared();
        let e_after = 0.5 * m1 * v1p.length_squared() + 0.5 * m2 * v2p.length_squared();
        assert!((e_before - e_after).abs() < 1e-12);
    }

    #[test]
    fn alejandose_no_impulsa() {
        // vrel·n > 0: ya se separan, no hay interacción.
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
    fn masa_cero_no_impulsa() {
        let (d1, d2) = elastic_pair(0.0, Vec3::new(1.0, 0.0, 0.0), 1.0, Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(d1, Vec3::ZERO);
        assert_eq!(d2, Vec3::ZERO);
    }
}
