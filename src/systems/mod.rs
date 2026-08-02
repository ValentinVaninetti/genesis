//! Sistemas del universo.
//!
//! Cada ley es un `System` independiente, activable/desactivable desde la
//! configuración. Aquí conviven las leyes físicas (colisiones, fuerzas LJ,
//! integración de Verlet) con la observación (estadísticas).

pub mod aggregate;
pub mod boundary;
pub mod collisions;
pub mod forces;
pub mod integrate;
pub mod movement;
pub mod structure;

pub use aggregate::StatsSystem;
pub use boundary::BoundarySystem;
pub use collisions::CollisionSystem;
pub use forces::ForceSystem;
pub use integrate::{PositionDrift, VelocityHalfKick};
pub use movement::MovementSystem;
pub use structure::StructureSystem;
