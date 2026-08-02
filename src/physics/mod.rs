//! Física del universo.
//!
//! Aquí viven las **leyes físicas**: colisiones elásticas y su detección
//! espacial ([`grid`], [`collision`]). Cada ley se expone como un `System` en
//! `crate::systems` o como una función pura a la que los sistemas llaman.
//!
//! Regla del proyecto: ninguna ley aquí dentro puede depender de "química" o
//! "biología". Solo interacciones fundamentales entre entidades.

pub mod collision;
pub mod forces;
pub mod grid;

pub use collision::elastic_pair;
pub use forces::LJ_CUTOFF_FACTOR;
pub use grid::min_image;

/// Constantes fundamentales de referencia (ajustables desde config).
pub mod constants {
    pub const GRAVITY: f64 = 6.674e-11;
    pub const BOLTZMANN: f64 = 1.380649e-23;
    pub const ELEMENTARY_CHARGE: f64 = 1.602176634e-19;
}
