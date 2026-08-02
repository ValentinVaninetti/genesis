//! # ECS — Entity Component System
//!
//! Core of the engine. A bespoke, data-oriented implementation based on
//! *archetypes* (grouping entities that share exactly the same set of
//! components in contiguous arrays — SoA layout).
//!
//! Design decisions:
//!
//! - **Archetypes, not loose sparse-sets**: all entities with the same set of
//!   components live in parallel arrays (`Vec<T>` per component). This
//!   guarantees cache locality, column-wise iteration and natural alignment
//!   for chunk-based parallelism (especially important with millions of
//!   homogeneous entities such as atoms).
//! - **Generational EntityId**: ids are never reused without incrementing
//!   their generation, so a "zombie" handle never accesses another entity's
//!   data.
//! - **No borrows through generic `Any` references**: `unsafe` is absent;
//!   downcasting to typed columns uses `downcast_*` validated by a global
//!   type registry.
//! - **No OOP**: no inheritance, no objects with behavior methods. Only data
//!   (`Component`) and separated logic (`System`).
//!
//! The module exposes:
//! - [`entity::EntityId`] — generational identifier.
//! - [`component::Component`] — trait defining a data type.
//! - [`world::World`] — archetype store + queries + entities.
//! - [`resource::Resources`] — global resources typed by `TypeId`.

pub mod archetype;
pub mod component;
pub mod entity;
pub mod resource;
pub mod world;

pub use archetype::{Archetype, ArchetypeId};
pub use component::{Component, ComponentId};
pub use entity::EntityId;
pub use resource::Resources;
pub use world::{Location, World};
