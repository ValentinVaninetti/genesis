//! `AtomType`: identidad elemental de un átomo.
//!
//! En el universo de Genesis un átomo **no** es un objeto especial: es una
//! entidad con `AtomType`. El tipo solo aporta masa y un nombre simbólico;
//! toda química futura debe ser consecuencia de las leyes, no de esta tabla.

use crate::ecs::{Component, ComponentId};
use serde::{Deserialize, Serialize};

/// Elementos disponibles inicialmente. La tabla es un punto de partida
/// configurable, no una ley.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AtomType {
    #[default]
    Hydrogen = 1,
    Helium = 2,
    Carbon = 6,
    Nitrogen = 7,
    Oxygen = 8,
    Sodium = 11,
}

impl AtomType {
    /// Nombre simbólico.
    pub fn symbol(self) -> &'static str {
        match self {
            AtomType::Hydrogen => "H",
            AtomType::Helium => "He",
            AtomType::Carbon => "C",
            AtomType::Nitrogen => "N",
            AtomType::Oxygen => "O",
            AtomType::Sodium => "Na",
        }
    }

    /// Masa atómica (unidades de masa atómica).
    pub fn mass(self) -> f64 {
        match self {
            AtomType::Hydrogen => 1.008,
            AtomType::Helium => 4.0026,
            AtomType::Carbon => 12.011,
            AtomType::Nitrogen => 14.007,
            AtomType::Oxygen => 15.999,
            AtomType::Sodium => 22.99,
        }
    }
}

impl Component for AtomType {
    const ID: ComponentId = ComponentId(7);
}
