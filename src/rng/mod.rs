//! Módulo RNG del universo.
//!
//! Todo número aleatorio de la simulación debe pasar por este único punto.
//! El RNG es *seedable* y *serializable*: parte del estado del universo, de
//! modo que una simulación guardada puede reanudarse con resultados
//! deterministas idénticos.

use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use serde::{Deserialize, Serialize};

/// Fuente de aleatoriedad de la simulación.
///
/// `Xoshiro256PlusPlus` es rápido y —clave para la persistencia— serializable
/// (feature `serde1`), de modo que un universo guardado retoma exactamente la
/// misma secuencia aleatoria.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rng {
    inner: Xoshiro256PlusPlus,
}

impl Rng {
    /// Crea un RNG a partir de una semilla entera.
    pub fn new(seed: u64) -> Self {
        Self {
            inner: Xoshiro256PlusPlus::seed_from_u64(seed),
        }
    }

    /// Un flotante uniforme en `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        rand::Rng::gen_range(&mut self.inner, 0.0..1.0)
    }

    /// Un flotante uniforme en `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// Un entero uniforme en `[lo, hi]` (inclusivo).
    pub fn int(&mut self, lo: u64, hi: u64) -> u64 {
        rand::Rng::gen_range(&mut self.inner, lo..=hi)
    }

    /// Una muestra de la distribución normal estándar (Box-Muller).
    ///
    /// Se usa para el seeding de Maxwell-Boltzmann: si cada componente de la
    /// velocidad es `N(0, σ)` con `σ = √(k·T/m)`, la rapidez `|v|` sigue la
    /// distribución de Maxwell-Boltzmann.
    pub fn gaussian(&mut self) -> f64 {
        let u1 = self.unit().max(f64::MIN_POSITIVE);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    /// Un vector uniformemente distribuido dentro de una caja.
    pub fn in_box(&mut self, half_extents: crate::math::Vec3) -> crate::math::Vec3 {
        crate::math::Vec3::new(
            self.range(-half_extents.x, half_extents.x),
            self.range(-half_extents.y, half_extents.y),
            self.range(-half_extents.z, half_extents.z),
        )
    }

    /// Acceso directo al generador subyacente para usos avanzados.
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
    fn rango_correcto() {
        let mut r = Rng::new(42);
        for _ in 0..1000 {
            let v = r.unit();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn determinista_con_misma_semilla() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.unit(), b.unit());
        }
    }
}
