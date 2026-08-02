//! Entity identifier with generation.

use serde::{Deserialize, Serialize};

/// Index inside the `World` tables.
pub type EntityIndex = u32;
/// Generation counter for recycling indices without collisions.
pub type EntityGeneration = u32;

/// Unique and stable identifier of an entity.
///
/// Composed of an index and a generation. When an entity is destroyed, its
/// generation is incremented: any `EntityId` pointing at the index with an
/// older generation is invalidated in a *detectable* way (accesses return
/// `None` instead of corrupt data).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct EntityId {
    index: EntityIndex,
    generation: EntityGeneration,
}

impl EntityId {
    pub(crate) const fn new(index: EntityIndex, generation: EntityGeneration) -> Self {
        Self { index, generation }
    }

    /// Index of the entity inside the `World`.
    pub const fn index(self) -> EntityIndex {
        self.index
    }

    /// Generation of the entity.
    pub const fn generation(self) -> EntityGeneration {
        self.generation
    }

    /// `true` if this id is valid for the current generation.
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
    fn identity_by_pairs() {
        let a = EntityId::new(3, 0);
        let b = EntityId::new(3, 1);
        assert_ne!(a, b);
        assert_eq!(a.index(), 3);
        assert_eq!(a.generation(), 0);
        assert_eq!(a.index(), b.index());
    }
}
