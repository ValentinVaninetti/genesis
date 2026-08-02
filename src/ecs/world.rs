//! The `World`: store of archetypes, entities and queries.

use crate::ecs::archetype::{Archetype, ArchetypeId, ColumnImpl};
use crate::ecs::component::{new_column, Component, ComponentId};
use crate::ecs::entity::{EntityGeneration, EntityId};
use rayon::prelude::*;
use std::any::TypeId;
use std::collections::HashMap;

/// Chunk size for parallel iterations inside an archetype.
const PAR_CHUNK: usize = 64;

/// Physical location of an entity inside the `World`.
#[derive(Debug, Clone, Copy)]
pub struct Location {
    pub(crate) archetype: ArchetypeId,
    pub(crate) row: u32,
}

/// Main store of the simulation.
///
/// Organizes entities into archetypes (SoA) and provides:
/// - entity lifecycle (`spawn`, `despawn`, `restore_entity`),
/// - component access (`get`, `get_mut`, `insert`, `remove`),
/// - sequential and parallel queries (`for_each1_*`, `for_each2_mut`, `par_*`),
/// - memory measurement.
#[derive(Default)]
pub struct World {
    pub(crate) archetypes: Vec<Archetype>,
    /// Component set (sorted) -> archetype id.
    archetype_index: HashMap<Vec<ComponentId>, ArchetypeId>,
    /// Per entity index: its current location.
    entities: Vec<Option<Location>>,
    /// Per entity index: current generation.
    generations: Vec<EntityGeneration>,
    /// Free indices to reuse.
    free: Vec<u32>,
    len_alive: usize,
}

impl World {
    /// Creates an empty `World` (with the "no components" archetype already
    /// created).
    pub fn new() -> Self {
        let mut w = Self::default();
        w.archetype_for(&[]);
        w
    }

    // ------------------------------------------------------------------
    // Entities
    // ------------------------------------------------------------------

    /// Creates an entity without components and returns its id.
    pub fn spawn(&mut self) -> EntityId {
        let empty = self.archetype_for(&[]);
        let index = if let Some(i) = self.free.pop() {
            i
        } else {
            self.entities.push(None);
            self.generations.push(0);
            (self.entities.len() - 1) as u32
        };
        let generation = self.generations[index as usize];
        let eid = EntityId::new(index, generation);

        let row = self.archetypes[empty as usize].len() as u32;
        self.archetypes[empty as usize].entities.push(eid);
        self.entities[index as usize] = Some(Location {
            archetype: empty,
            row,
        });
        self.len_alive += 1;
        eid
    }

    /// Creates an entity at a given index/generation.
    ///
    /// Reserved for snapshot deserialization, where ids must be preserved.
    /// Idempotent: if the index is already alive with the same generation, it
    /// returns the existing entity without duplicating it.
    pub fn restore_entity(&mut self, index: u32, generation: EntityGeneration) -> EntityId {
        if (index as usize) < self.entities.len()
            && let Some(_loc) = self.entities[index as usize]
        {
            assert_eq!(
                self.generations[index as usize], generation,
                "restore_entity: index {index} alive with another generation"
            );
            return EntityId::new(index, generation);
        }
        if index as usize >= self.entities.len() {
            self.entities.resize(index as usize + 1, None);
            self.generations.resize(index as usize + 1, 0);
        }
        self.generations[index as usize] = generation;
        self.free.retain(|&i| i != index);

        let empty = self.archetype_for(&[]);
        let eid = EntityId::new(index, generation);
        let row = self.archetypes[empty as usize].len() as u32;
        self.archetypes[empty as usize].entities.push(eid);
        self.entities[index as usize] = Some(Location {
            archetype: empty,
            row,
        });
        self.len_alive += 1;
        eid
    }

    /// Destroys an entity (and all its components). Returns `false` if the id
    /// was stale or did not exist.
    pub fn despawn(&mut self, entity: EntityId) -> bool {
        let Some(loc) = self.locate(entity) else {
            return false;
        };
        let row = loc.row as usize;
        let arch_id = loc.archetype;

        let arch = &mut self.archetypes[arch_id as usize];
        for col in arch.columns.iter_mut() {
            col.swap_remove(row);
        }
        let displaced = arch.remove_row_swap(row);
        if let Some(d) = displaced {
            self.entities[d.index() as usize] = Some(Location {
                archetype: arch_id,
                row: row as u32,
            });
        }

        self.entities[entity.index() as usize] = None;
        self.generations[entity.index() as usize] += 1;
        self.free.push(entity.index());
        self.len_alive -= 1;
        true
    }

    /// Is the entity alive (id valid)?
    pub fn is_alive(&self, entity: EntityId) -> bool {
        self.locate(entity).is_some()
    }

    /// Number of alive entities.
    pub fn len(&self) -> usize {
        self.len_alive
    }

    pub fn is_empty(&self) -> bool {
        self.len_alive == 0
    }

    /// Number of entity index slots (max index + 1).
    ///
    /// Indices are not compacted when entities are destroyed, so a buffer
    /// indexed by `EntityId::index` must be sized with this value.
    pub fn entity_capacity(&self) -> usize {
        self.entities.len()
    }

    /// Iterator over all alive entities.
    pub fn iter_entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.archetypes
            .iter()
            .flat_map(|a| a.entities.iter().copied())
    }

    /// Location of the entity, validating the generation.
    pub(crate) fn locate(&self, entity: EntityId) -> Option<Location> {
        let idx = entity.index() as usize;
        let loc = *self.entities.get(idx)?;
        loc.filter(|_| self.generations[idx] == entity.generation())
    }

    // ------------------------------------------------------------------
    // Components
    // ------------------------------------------------------------------

    /// Inserts (or replaces) the component `T` in the entity.
    ///
    /// If the entity did not have it, it is moved to another archetype moving
    /// its data contiguously (swap-remove), without copying.
    pub fn insert<T: Component>(&mut self, entity: EntityId, value: T) {
        let Some(loc) = self.locate(entity) else {
            return;
        };
        let row = loc.row as usize;
        let src_id = loc.archetype;

        // In-place replacement (the entity already had the component).
        if let Some(pos) = self.archetypes[src_id as usize].position_of(T::ID) {
            let col = self.archetypes[src_id as usize].columns[pos]
                .as_any_mut()
                .downcast_mut::<ColumnImpl<T>>()
                .expect("insert: column of type T");
            col.data[row] = value;
            return;
        }

        // Migration to an archetype with the new component.
        let mut new_set = self.archetypes[src_id as usize].components.clone();
        new_set.push(T::ID);
        new_set.sort_unstable();
        let dst_id = self.archetype_for(&new_set);
        let dst_ids = self.archetypes[dst_id as usize].components.clone();

        let mut tmp = ColumnImpl {
            data: vec![value],
        };
        let (src, dst) = split_archetypes(&mut self.archetypes, src_id, dst_id);
        for cid in &dst_ids {
            let dst_col = dst.column_mut(*cid);
            if *cid == T::ID {
                dst_col.push_row(&mut tmp, 0);
            } else {
                let src_col = src.column_mut(*cid);
                dst_col.push_row(src_col, row);
            }
        }
        let displaced = src.remove_row_swap(row);
        let dst_row = dst.len() as u32;
        dst.entities.push(entity);
        if let Some(d) = displaced {
            self.entities[d.index() as usize] = Some(Location {
                archetype: src_id,
                row: row as u32,
            });
        }
        self.entities[entity.index() as usize] = Some(Location {
            archetype: dst_id,
            row: dst_row,
        });
    }

    /// Removes the component `T` from the entity, returning the value if it
    /// existed.
    pub fn remove<T: Component>(&mut self, entity: EntityId) -> Option<T> {
        let loc = self.locate(entity)?;
        let row = loc.row as usize;
        let src_id = loc.archetype;
        let src_set = self.archetypes[src_id as usize].components.clone();
        if !src_set.contains(&T::ID) {
            return None;
        }

        let mut new_set = src_set.clone();
        new_set.retain(|c| *c != T::ID);
        let dst_id = self.archetype_for(&new_set);
        let dst_ids = self.archetypes[dst_id as usize].components.clone();

        let removed;
        {
            let (src, dst) = split_archetypes(&mut self.archetypes, src_id, dst_id);
            for cid in &dst_ids {
                let dst_col = dst.column_mut(*cid);
                let src_col = src.column_mut(*cid);
                dst_col.push_row(src_col, row);
            }
            // T is not in dst_ids; its value still lives in row `row`.
            let t_pos = src.position_of(T::ID).expect("T present in src");
            let col = src.columns[t_pos]
                .as_any_mut()
                .downcast_mut::<ColumnImpl<T>>()
                .expect("column T");
            removed = col.data.swap_remove(row);

            let displaced = src.remove_row_swap(row);
            let dst_row = dst.len() as u32;
            dst.entities.push(entity);
            if let Some(d) = displaced {
                self.entities[d.index() as usize] = Some(Location {
                    archetype: src_id,
                    row: row as u32,
                });
            }
            self.entities[entity.index() as usize] = Some(Location {
                archetype: dst_id,
                row: dst_row,
            });
        }
        Some(removed)
    }

    /// Immutable access to a component.
    pub fn get<T: Component>(&self, entity: EntityId) -> Option<&T> {
        let loc = self.locate(entity)?;
        let arch = &self.archetypes[loc.archetype as usize];
        let pos = arch.position_of(T::ID)?;
        let col = arch.columns[pos].as_any().downcast_ref::<ColumnImpl<T>>()?;
        Some(&col.data[loc.row as usize])
    }

    /// Mutable access to a component.
    pub fn get_mut<T: Component>(&mut self, entity: EntityId) -> Option<&mut T> {
        let loc = self.locate(entity)?;
        let arch = &mut self.archetypes[loc.archetype as usize];
        let pos = arch.position_of(T::ID)?;
        let col = arch.columns[pos].as_any_mut().downcast_mut::<ColumnImpl<T>>()?;
        Some(&mut col.data[loc.row as usize])
    }

    /// Does the entity have the component `T`?
    pub fn has<T: Component>(&self, entity: EntityId) -> bool {
        self.get::<T>(entity).is_some()
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Iterates all entities with the component `T` (immutable).
    pub fn for_each1<T: Component>(&self, mut f: impl FnMut(EntityId, &T)) {
        for arch in &self.archetypes {
            if let Some(pos) = arch.position_of(T::ID) {
                let col = arch.columns[pos].as_any().downcast_ref::<ColumnImpl<T>>().expect("column T");
                for (row, value) in col.data.iter().enumerate() {
                    f(arch.entities[row], value);
                }
            }
        }
    }

    /// Iterates entities with the components `A` and `B` (both immutable).
    pub fn for_each2<A: Component, B: Component>(&self, mut f: impl FnMut(EntityId, &A, &B)) {
        assert_ne!(TypeId::of::<A>(), TypeId::of::<B>(), "A and B must be different");
        for arch in &self.archetypes {
            let (Some(pa), Some(pb)) = (arch.position_of(A::ID), arch.position_of(B::ID)) else {
                continue;
            };
            let ca = arch.columns[pa]
                .as_any()
                .downcast_ref::<ColumnImpl<A>>()
                .expect("column A");
            let cb = arch.columns[pb]
                .as_any()
                .downcast_ref::<ColumnImpl<B>>()
                .expect("column B");
            for (row, (a, b)) in ca.data.iter().zip(cb.data.iter()).enumerate() {
                f(arch.entities[row], a, b);
            }
        }
    }

    /// Iterates entities with the components `A`, `B` and `C` (all immutable).
    pub fn for_each3<A: Component, B: Component, C: Component>(
        &self,
        mut f: impl FnMut(EntityId, &A, &B, &C),
    ) {
        let (ta, tb, tc) = (TypeId::of::<A>(), TypeId::of::<B>(), TypeId::of::<C>());
        assert_ne!(ta, tb, "A and B must be different");
        assert_ne!(ta, tc, "A and C must be different");
        assert_ne!(tb, tc, "B and C must be different");
        for arch in &self.archetypes {
            let (Some(pa), Some(pb), Some(pc)) = (
                arch.position_of(A::ID),
                arch.position_of(B::ID),
                arch.position_of(C::ID),
            ) else {
                continue;
            };
            let ca = arch.columns[pa]
                .as_any()
                .downcast_ref::<ColumnImpl<A>>()
                .expect("column A");
            let cb = arch.columns[pb]
                .as_any()
                .downcast_ref::<ColumnImpl<B>>()
                .expect("column B");
            let cc = arch.columns[pc]
                .as_any()
                .downcast_ref::<ColumnImpl<C>>()
                .expect("column C");
            for (row, ((a, b), c)) in ca
                .data
                .iter()
                .zip(cb.data.iter())
                .zip(cc.data.iter())
                .enumerate()
            {
                f(arch.entities[row], a, b, c);
            }
        }
    }

    /// Iterates all entities with the component `T` (mutable).
    pub fn for_each1_mut<T: Component>(&mut self, mut f: impl FnMut(EntityId, &mut T)) {
        for arch in self.archetypes.iter_mut() {
            let Some(pos) = arch.position_of(T::ID) else {
                continue;
            };
            let crate::ecs::archetype::Archetype {
                entities, columns, ..
            } = arch;
            let col = columns[pos].as_any_mut().downcast_mut::<ColumnImpl<T>>().expect("column T");
            let ents = entities.as_slice();
            for (row, value) in col.data.iter_mut().enumerate() {
                f(ents[row], value);
            }
        }
    }

    /// Iterates entities with the components `A` and `B` (both mutable).
    ///
    /// Requires `A != B` (to iterate the same component twice, use
    /// `for_each1`).
    pub fn for_each2_mut<A: Component, B: Component>(&mut self, mut f: impl FnMut(EntityId, &mut A, &mut B)) {
        assert_ne!(TypeId::of::<A>(), TypeId::of::<B>(), "A and B must be different");
        for arch in self.archetypes.iter_mut() {
            let (Some(pa), Some(pb)) = (arch.position_of(A::ID), arch.position_of(B::ID)) else {
                continue;
            };
            let crate::ecs::archetype::Archetype {
                entities, columns, ..
            } = arch;
            let (lo, hi) = (pa.min(pb), pa.max(pb));
            let (left, right) = columns.split_at_mut(hi);
            let (ca, cb) = if pa < pb {
                (
                    left[lo].as_any_mut().downcast_mut::<ColumnImpl<A>>().expect("column A"),
                    right[0].as_any_mut().downcast_mut::<ColumnImpl<B>>().expect("column B"),
                )
            } else {
                (
                    right[0].as_any_mut().downcast_mut::<ColumnImpl<A>>().expect("column A"),
                    left[lo].as_any_mut().downcast_mut::<ColumnImpl<B>>().expect("column B"),
                )
            };
            let ents = entities.as_slice();
            for (row, (a, b)) in ca.data.iter_mut().zip(cb.data.iter_mut()).enumerate() {
                f(ents[row], a, b);
            }
        }
    }

    /// Iterates in parallel (by chunks) entities with the component `T`.
    pub fn par_for_each1_mut<T: Component>(&mut self, f: impl Fn(EntityId, &mut T) + Sync + Send) {
        for arch in self.archetypes.iter_mut() {
            let Some(pos) = arch.position_of(T::ID) else {
                continue;
            };
            let crate::ecs::archetype::Archetype {
                entities, columns, ..
            } = arch;
            let col = columns[pos].as_any_mut().downcast_mut::<ColumnImpl<T>>().expect("column T");
            let ents = entities.as_slice();
            col.data
                .par_chunks_mut(PAR_CHUNK)
                .zip(ents.par_chunks(PAR_CHUNK))
                .for_each(|(c, e)| {
                    for i in 0..c.len() {
                        f(e[i], &mut c[i]);
                    }
                });
        }
    }

    /// Iterates in parallel (by chunks) entities with the components `A` and `B`.
    pub fn par_for_each2_mut<A: Component, B: Component>(&mut self, f: impl Fn(EntityId, &mut A, &mut B) + Sync + Send) {
        assert_ne!(TypeId::of::<A>(), TypeId::of::<B>(), "A and B must be different");
        for arch in self.archetypes.iter_mut() {
            let (Some(pa), Some(pb)) = (arch.position_of(A::ID), arch.position_of(B::ID)) else {
                continue;
            };
            let crate::ecs::archetype::Archetype {
                entities, columns, ..
            } = arch;
            let (lo, hi) = (pa.min(pb), pa.max(pb));
            let (left, right) = columns.split_at_mut(hi);
            let (ca, cb) = if pa < pb {
                (
                    left[lo].as_any_mut().downcast_mut::<ColumnImpl<A>>().expect("column A"),
                    right[0].as_any_mut().downcast_mut::<ColumnImpl<B>>().expect("column B"),
                )
            } else {
                (
                    right[0].as_any_mut().downcast_mut::<ColumnImpl<A>>().expect("column A"),
                    left[lo].as_any_mut().downcast_mut::<ColumnImpl<B>>().expect("column B"),
                )
            };
            let ents = entities.as_slice();
            ca.data
                .par_chunks_mut(PAR_CHUNK)
                .zip(cb.data.par_chunks_mut(PAR_CHUNK))
                .zip(ents.par_chunks(PAR_CHUNK))
                .for_each(|((a, b), e)| {
                    for i in 0..a.len() {
                        f(e[i], &mut a[i], &mut b[i]);
                    }
                });
        }
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    /// Returns the archetype id for a component set, creating it if it does
    /// not exist.
    pub(crate) fn archetype_for(&mut self, set: &[ComponentId]) -> ArchetypeId {
        if let Some(id) = self.archetype_index.get(set) {
            return *id;
        }
        let mut sorted = set.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let id = self.archetypes.len() as ArchetypeId;
        let columns = sorted.iter().map(|cid| new_column(*cid)).collect();
        self.archetypes.push(Archetype::new(id, sorted.clone(), columns));
        self.archetype_index.insert(sorted, id);
        id
    }

    /// Number of archetypes.
    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    /// Access to the archetypes (for inspection/debug).
    pub fn archetypes(&self) -> &[Archetype] {
        &self.archetypes
    }

    /// Approximate bytes occupied by the `World`.
    pub fn memory_bytes(&self) -> usize {
        let mut total = 0usize;
        for arch in &self.archetypes {
            total += arch.memory_bytes();
        }
        total += self
            .entities
            .capacity()
            .saturating_mul(std::mem::size_of::<Option<Location>>());
        total += self
            .generations
            .capacity()
            .saturating_mul(std::mem::size_of::<EntityGeneration>());
        total += self
            .free
            .capacity()
            .saturating_mul(std::mem::size_of::<u32>());
        total
    }

    /// Completely empties the world (resets the empty archetype).
    pub fn clear(&mut self) {
        self.archetypes.clear();
        self.archetype_index.clear();
        self.entities.clear();
        self.generations.clear();
        self.free.clear();
        self.len_alive = 0;
        self.archetype_for(&[]);
    }
}

/// Two distinct archetypes with disjoint mutable borrows.
///
/// Free function (not a method) so that the borrow is limited to the
/// `archetypes` field and other `World` fields can be accessed in parallel.
fn split_archetypes(
    archetypes: &mut [Archetype],
    a: ArchetypeId,
    b: ArchetypeId,
) -> (&mut Archetype, &mut Archetype) {
    debug_assert_ne!(a, b, "cannot split the same archetype");
    let (lo, hi) = (a.min(b) as usize, a.max(b) as usize);
    let (left, right) = archetypes.split_at_mut(hi);
    if a < b {
        (&mut left[lo], &mut right[0])
    } else {
        (&mut right[0], &mut left[lo])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::register_all;
    use crate::components::{Mass, Position, Velocity};
    use crate::math::Vec3;

    fn world() -> World {
        register_all();
        World::new()
    }

    #[test]
    fn spawn_despawn_and_generations() {
        let mut w = world();
        let a = w.spawn();
        let b = w.spawn();
        assert!(w.is_alive(a));
        assert_eq!(w.len(), 2);

        assert!(w.despawn(a));
        assert!(!w.is_alive(a));
        assert!(!w.despawn(a)); // stale id

        // The index is recycled with an incremented generation.
        let c = w.spawn();
        assert_eq!(c.index(), a.index());
        assert_ne!(c.generation(), a.generation());
        assert!(w.is_alive(c));
        assert!(w.is_alive(b));
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn insert_get_remove() {
        let mut w = world();
        let e = w.spawn();
        assert!(w.get::<Position>(e).is_none());

        w.insert(e, Position(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(w.get::<Position>(e).unwrap().x, 1.0);
        assert_eq!(w.archetype_count(), 2); // empty + [Position]

        // In-place replacement does not change archetype.
        w.insert(e, Position(Vec3::new(9.0, 9.0, 9.0)));
        assert_eq!(w.get::<Position>(e).unwrap().x, 9.0);
        assert_eq!(w.archetype_count(), 2);

        let p = w.remove::<Position>(e);
        assert_eq!(p.unwrap().x, 9.0);
        assert!(w.get::<Position>(e).is_none());
    }

    #[test]
    fn migration_between_archetypes_keeps_alignment() {
        let mut w = world();
        let e = w.spawn();
        w.insert(e, Position(Vec3::new(1.0, 0.0, 0.0)));
        w.insert(e, Velocity(Vec3::new(2.0, 0.0, 0.0)));
        w.insert(e, Mass(5.0));

        // The three components point to the same entity and row.
        let mut seen = 0;
        w.for_each2_mut::<Position, Velocity>(|id, pos, vel| {
            assert_eq!(id, e);
            assert_eq!(pos.x, 1.0);
            assert_eq!(vel.x, 2.0);
            seen += 1;
        });
        assert_eq!(seen, 1);

        // Remove from the middle and verify the integrity of the other entities.
        let other = w.spawn();
        w.insert(other, Position(Vec3::ZERO));
        w.insert(other, Velocity(Vec3::ZERO));

        assert_eq!(w.remove::<Mass>(e).unwrap().0, 5.0);
        w.for_each2_mut::<Position, Velocity>(|id, _, _| {
            assert!(id == e || id == other);
        });

        // The value of the first entity survived intact.
        assert_eq!(w.get::<Position>(e).unwrap().x, 1.0);
        assert_eq!(w.get::<Velocity>(e).unwrap().x, 2.0);
        assert_eq!(w.get::<Position>(other).unwrap().x, 0.0);
    }

    #[test]
    fn despawn_swap_remove_preserves_data() {
        let mut w = world();
        let ids: Vec<_> = (0..5).map(|i| {
            let e = w.spawn();
            w.insert(e, Position(Vec3::new(i as f64, 0.0, 0.0)));
            e
        })
        .collect();

        // Removing entities from the middle forces a real swap-remove.
        assert!(w.despawn(ids[2]));
        assert!(w.despawn(ids[0]));

        let mut remaining: Vec<(EntityId, f64)> = Vec::new();
        w.for_each1::<Position>(|id, p| remaining.push((id, p.x)));
        assert_eq!(remaining.len(), 3);

        // Each survivor keeps its original position.
        for (id, x) in remaining {
            let expected = id.index() as f64;
            assert_eq!(x, expected, "position of {id} corrupted");
        }
        assert_eq!(w.len(), 3);
    }

    #[test]
    fn parallel_matches_sequential() {
        let mut w = world();
        let n = 1000;
        for i in 0..n {
            let e = w.spawn();
            w.insert(e, Position(Vec3::new(i as f64, 0.0, 0.0)));
            w.insert(e, Mass(1.0));
        }

        // Sequential: scale X by 2.
        w.for_each2_mut::<Position, Mass>(|_, p, m| {
            p.x += m.0;
        });

        // Parallel: scale X by 3 and verify the result is exact.
        w.par_for_each2_mut::<Position, Mass>(|_, p, m| {
            p.x += m.0;
        });

        let mut total: f64 = 0.0;
        w.for_each1::<Position>(|_, p| total += p.x);
        let expected: f64 = (0..n).map(|i| i as f64 + 2.0).sum();
        assert!((total - expected).abs() < 1e-9, "total {total} != {expected}");
    }

    #[test]
    fn restore_entity_preserves_ids() {
        let mut w = world();
        let e = w.restore_entity(77, 5);
        assert_eq!(e.index(), 77);
        assert_eq!(e.generation(), 5);
        w.insert(e, Position(Vec3::ONE));
        let same = w.restore_entity(77, 5);
        assert_eq!(w.get::<Position>(same).unwrap().x, 1.0);
    }

    #[test]
    fn stale_get_returns_none() {
        let mut w = world();
        let e = w.spawn();
        w.insert(e, Position(Vec3::ONE));
        assert!(w.despawn(e));
        // Zombie handle: no corrupt data, just None.
        assert!(w.get::<Position>(e).is_none());
    }
}
