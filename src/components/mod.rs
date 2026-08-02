//! Component catalog of the universe.
//!
//! This module defines the **only** source-of-truth list of components. The
//! `for_each_component!` macro reuses it to generate the type registry and
//! the serialization code. To add a future component:
//!
//! 1. create `src/components/<new>.rs` with `impl Component for New { const ID }`,
//! 2. add it to the `for_each_component!` macro below.
//!
//! ⚠️ The `ComponentId`s are permanent: never reassign or reuse them.

pub mod acceleration;
pub mod atom_type;
pub mod bonds;
pub mod charge;
pub mod mass;
pub mod position;
pub mod velocity;

pub use acceleration::Acceleration;
pub use atom_type::AtomType;
pub use bonds::Bonds;
pub use charge::Charge;
pub use mass::Mass;
pub use position::Position;
pub use velocity::Velocity;

/// Single list of components: `(Type, field_name)`.
///
/// Ids 5 (`Energy`) and 6 (`Temperature`) were **retired** in the physics
/// stage: energy and temperature are *derived* magnitudes (kinetic +
/// equipartition), not stored state. Retired ids are not reused.
macro_rules! for_each_component {
    ($apply:ident) => {
        $apply! {
            (Position, position),
            (Velocity, velocity),
            (Mass, mass),
            (Charge, charge),
            (AtomType, atom_type),
            (Bonds, bonds),
            (Acceleration, acceleration),
        }
    };
}
pub(crate) use for_each_component;

/// Registers all the components in the global ECS table.
/// Idempotent; invoked once when the universe starts.
pub fn register_all() {
    for_each_component!(register_one);
}

macro_rules! register_one {
    ($(($t:ident, $name:ident)),* $(,)?) => {
        $( crate::ecs::component::register::<$t>(); )*
    };
}
pub(crate) use register_one;
