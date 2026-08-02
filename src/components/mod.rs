//! Catálogo de componentes del universo.
//!
//! Este módulo define la **única** lista fuente de verdad de componentes.
//! La macro `for_each_component!` la reutiliza para generar el registro de
//! tipos y el código de serialización. Para agregar un componente futuro:
//!
//! 1. crear `src/components/<nuevo>.rs` con `impl Component for Nuevo { const ID }`,
//! 2. añadirlo a la macro `for_each_component!` de abajo.
//!
//! ⚠️ Los `ComponentId` son permanentes: jamás reasignar ni reutilizar.

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

/// Lista única de componentes: `(Tipo, nombre_de_campo)`.
///
/// Los ids 5 (`Energy`) y 6 (`Temperature`) se **retiraron** en la etapa de
/// física: la energía y la temperatura son magnitudes *derivadas* (cinética +
/// equipartición), no estado almacenado. Los ids retirados no se reutilizan.
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

/// Registra todos los componentes en la tabla global del ECS.
/// Idempotente; se invoca una vez al arrancar el universo.
pub fn register_all() {
    for_each_component!(register_one);
}

macro_rules! register_one {
    ($(($t:ident, $name:ident)),* $(,)?) => {
        $( crate::ecs::component::register::<$t>(); )*
    };
}
pub(crate) use register_one;
