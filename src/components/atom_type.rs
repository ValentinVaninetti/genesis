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
    Silicon = 14,
    Phosphorus = 15,
    Sulfur = 16,
    Iron = 26,
}

impl AtomType {
    /// Number of available elements.
    pub const COUNT: usize = 10;

    /// All available elements (configurable starting point, not a law).
    pub const ALL: [Self; Self::COUNT] = [
        Self::Hydrogen,
        Self::Helium,
        Self::Carbon,
        Self::Nitrogen,
        Self::Oxygen,
        Self::Sodium,
        Self::Silicon,
        Self::Phosphorus,
        Self::Sulfur,
        Self::Iron,
    ];

    /// Parses an element by symbol or full name (case-insensitive).
    pub fn by_name(name: &str) -> Option<Self> {
        match name.trim() {
            "H" | "Hydrogen" | "hydrogen" => Some(Self::Hydrogen),
            "He" | "Helium" | "helium" => Some(Self::Helium),
            "C" | "Carbon" | "carbon" => Some(Self::Carbon),
            "N" | "Nitrogen" | "nitrogen" => Some(Self::Nitrogen),
            "O" | "Oxygen" | "oxygen" => Some(Self::Oxygen),
            "Na" | "Sodium" | "sodium" => Some(Self::Sodium),
            "Si" | "Silicon" | "silicon" => Some(Self::Silicon),
            "P" | "Phosphorus" | "phosphorus" => Some(Self::Phosphorus),
            "S" | "Sulfur" | "sulfur" => Some(Self::Sulfur),
            "Fe" | "Iron" | "iron" => Some(Self::Iron),
            _ => None,
        }
    }

    /// Symbolic name.
    pub fn symbol(self) -> &'static str {
        match self {
            AtomType::Hydrogen => "H",
            AtomType::Helium => "He",
            AtomType::Carbon => "C",
            AtomType::Nitrogen => "N",
            AtomType::Oxygen => "O",
            AtomType::Sodium => "Na",
            AtomType::Silicon => "Si",
            AtomType::Phosphorus => "P",
            AtomType::Sulfur => "S",
            AtomType::Iron => "Fe",
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
            AtomType::Silicon => 28.085,
            AtomType::Phosphorus => 30.974,
            AtomType::Sulfur => 32.06,
            AtomType::Iron => 55.845,
        }
    }
}

impl Component for AtomType {
    const ID: ComponentId = ComponentId(7);
}
