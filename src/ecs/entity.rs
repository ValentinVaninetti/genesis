//! Identificador de entidad con generación.

use serde::{Deserialize, Serialize};

/// Índice dentro de las tablas del `World`.
pub type EntityIndex = u32;
/// Contador de generación para reciclar índices sin colisiones.
pub type EntityGeneration = u32;

/// Identificador único y estable de una entidad.
///
/// Compuesto por un índice y una generación. Cuando una entidad se destruye,
/// su generación se incrementa: cualquier `EntityId` que apunte al índice con
/// una generación anterior queda invalidado de forma *detectable* (los accesos
/// devuelven `None` en lugar de datos corruptos).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct EntityId {
    index: EntityIndex,
    generation: EntityGeneration,
}

impl EntityId {
    pub(crate) const fn new(index: EntityIndex, generation: EntityGeneration) -> Self {
        Self { index, generation }
    }

    /// Índice de la entidad dentro del `World`.
    pub const fn index(self) -> EntityIndex {
        self.index
    }

    /// Generación de la entidad.
    pub const fn generation(self) -> EntityGeneration {
        self.generation
    }

    /// `true` si este id está vigente para la generación actual.
    pub fn is_alive(self, world: &crate::ecs::World) -> bool {
        world.is_alive(self)
    }
}

impl Default for EntityId {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}:{}", self.index, self.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identidad_por_pares() {
        let a = EntityId::new(3, 0);
        let b = EntityId::new(3, 1);
        assert_ne!(a, b);
        assert_eq!(a.index(), 3);
        assert_eq!(a.generation(), 0);
        assert_eq!(a.index(), b.index());
    }
}
