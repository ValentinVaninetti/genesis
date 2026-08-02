//! Archetypes: grouping of entities with an identical set of components.
//!
//! An `Archetype` stores, in parallel arrays (SoA), a `Vec<T>` column for each
//! component of the set. All rows are aligned: row `i` is the same entity in
//! every column.

use crate::ecs::component::ComponentId;
use crate::ecs::entity::EntityId;
use crate::ecs::Component;
use std::any::{Any, TypeId};

/// Archetype identifier inside the `World`.
pub type ArchetypeId = u32;

/// Generic typed column (trait object).
///
/// The contract is minimal: inspection, bytes in memory, and moving a row
/// from another column (equivalent to `swap_remove` + `push`). All the
/// downcasting is validated against `TypeId`/`ComponentId` in the `World`.
pub trait Column: Any + Send + Sync {
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Number of rows.
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Approximate bytes occupied by the buffer (real capacity).
    fn bytes(&self) -> usize;

    /// Moves the value at row `src_row` of `src` to the end of this column.
    ///
    /// `swap_remove` semantics: row `src_row` is removed from `src` by moving
    /// the last element to its position. All `ColumnImpl`s of the same
    /// archetype must process the same row to keep the alignment.
    fn push_row(&mut self, src: &mut dyn Column, src_row: usize);

    /// Removes row `row` (`swap_remove` semantics).
    fn swap_remove(&mut self, row: usize);

    /// Empties the column keeping its capacity.
    fn clear(&mut self);
}

/// Concrete column implementation: a flat `Vec<T>`.
pub struct ColumnImpl<T> {
    pub(crate) data: Vec<T>,
}

impl<T> Default for ColumnImpl<T> {
    fn default() -> Self {
        Self { data: Vec::new() }
    }
}

impl<T> Column for ColumnImpl<T>
where
    T: Component,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn bytes(&self) -> usize {
        self.data.capacity().saturating_mul(std::mem::size_of::<T>())
    }

    fn push_row(&mut self, src: &mut dyn Column, src_row: usize) {
        let src = src
            .as_any_mut()
            .downcast_mut::<ColumnImpl<T>>()
            .expect("push_row: column type does not match");
        let value = src.data.swap_remove(src_row);
        self.data.push(value);
    }

    fn swap_remove(&mut self, row: usize) {
        self.data.swap_remove(row);
    }

    fn clear(&mut self) {
        self.data.clear();
    }
}

/// A group of entities with the same set of components.
pub struct Archetype {
    pub(crate) id: ArchetypeId,
    /// Component set sorted ascending (uniqueness key).
    pub(crate) components: Vec<ComponentId>,
    /// SoA columns, parallel to `components`.
    pub(crate) columns: Vec<Box<dyn Column>>,
    /// Entities, parallel to the rows.
    pub(crate) entities: Vec<EntityId>,
}

impl Archetype {
    pub(crate) fn new(id: ArchetypeId, components: Vec<ComponentId>, columns: Vec<Box<dyn Column>>) -> Self {
        debug_assert_eq!(components.len(), columns.len());
        Self {
            id,
            components,
            columns,
            entities: Vec::new(),
        }
    }

    pub fn id(&self) -> ArchetypeId {
        self.id
    }

    /// Component ids of this archetype (sorted).
    pub fn component_ids(&self) -> &[ComponentId] {
        &self.components
    }

    /// Number of entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Position of the component in the set (binary search).
    pub fn position_of(&self, id: ComponentId) -> Option<usize> {
        self.components.binary_search(&id).ok()
    }

    pub(crate) fn column_mut(&mut self, id: ComponentId) -> &mut dyn Column {
        let p = self
            .position_of(id)
            .unwrap_or_else(|| panic!("column {:?} missing in archetype {}", id, self.id));
        self.columns[p].as_mut()
    }

    /// Removes row `row` from the entity list (`swap_remove`).
    ///
    /// Returns the entity displaced to `row`, if any. Note: the data columns
    /// must already have been removed coherently by the caller.
    pub(crate) fn remove_row_swap(&mut self, row: usize) -> Option<EntityId> {
        let last = self.entities.len() - 1;
        let displaced = (last != row).then(|| self.entities[last]);
        self.entities.swap_remove(row);
        displaced
    }

    /// Approximate bytes of the archetype (columns + metadata).
    pub fn memory_bytes(&self) -> usize {
        let mut total = self
            .entities
            .capacity()
            .saturating_mul(std::mem::size_of::<EntityId>());
        total += self
            .components
            .capacity()
            .saturating_mul(std::mem::size_of::<ComponentId>());
        total += self.columns.len().saturating_mul(std::mem::size_of::<Box<dyn Column>>());
        for col in &self.columns {
            total += col.bytes();
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::component::Component;

    struct A;
    impl Component for A {
        const ID: ComponentId = ComponentId(1);
    }

    #[allow(dead_code)]
    struct B;
    impl Component for B {
        const ID: ComponentId = ComponentId(2);
    }

    #[test]
    fn push_row_moves_and_removes() {
        let mut a: ColumnImpl<A> = ColumnImpl {
            data: vec![A, A, A],
        };
        let mut b: ColumnImpl<A> = ColumnImpl { data: Vec::new() };

        // move row 1 of a -> b (swap_remove: the last one goes to position 1)
        b.push_row(&mut a, 1);

        assert_eq!(a.len(), 2);
        assert_eq!(b.len(), 1);
        assert_eq!(a.data.len(), 2);
    }
}
