//! `AtomType`: elemental identity of an atom.
//!
//! In the Genesis universe an atom is **not** a special object: it is an
//! entity with `AtomType`. The type only provides mass and a symbolic name;
//! any future chemistry must be a consequence of the laws, not of this table.

use crate::ecs::{Component, ComponentId};
use serde::{Deserialize, Serialize};

/// Elements initially available. The table is a configurable starting point,
/// not a law.
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
    /// Symbolic name.
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

    /// Atomic mass (atomic mass units).
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
