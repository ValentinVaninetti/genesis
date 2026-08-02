//! Physics of the universe.
//!
//! The **physical laws** live here: elastic collisions and their spatial
//! detection ([`grid`], [`collision`]). Each law is exposed as a `System` in
//! `crate::systems` or as a pure function that the systems call.
//!
//! Project rule: no law in here may depend on "chemistry" or "biology". Only
//! fundamental interactions between entities.

pub mod collision;
pub mod forces;
pub mod grid;

pub use collision::elastic_pair;
pub use forces::LJ_CUTOFF_FACTOR;
pub use grid::min_image;

/// Fundamental reference constants (adjustable from config).
pub mod constants {
    pub const GRAVITY: f64 = 6.674e-11;
    pub const BOLTZMANN: f64 = 1.380649e-23;
    pub const ELEMENTARY_CHARGE: f64 = 1.602176634e-19;
}
