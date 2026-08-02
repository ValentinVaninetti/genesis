//! # ECS — Entity Component System
//!
//! Núcleo del motor. Implementación **propia**, orientada a datos y basada en
//! *arquetipos* (agrupación de entidades que comparten exactamente el mismo
//! conjunto de componentes en arrays contiguos — estructura SoA).
//!
//! Decisiones de diseño:
//!
//! - **Arquetipos, no sparse-sets sueltos**: todas las entidades con el mismo
//!   set de componentes viven en arrays paralelos (`Vec<T>` por componente).
//!   Esto garantiza localidad de caché, iteración en columnas y alineación
//!   natural para paralelización por chunks (especialmente importante con
//!   millones de entidades homogéneas como los átomos).
//! - **EntityId generacional**: los ids nunca se reutilizan sin incrementar su
//!   generación, así un handle "zombie" no accede nunca a datos de otra entidad.
//! - **Sin borrows a través de referencias genéricas a `Any`**: el `unsafe`
//!   está ausente; el downcasting a columnas tipadas se hace con `downcast_*`
//!   validado por un registro global de tipos.
//! - **Sin OOP**: no hay herencia, no hay objetos con métodos de comportamiento.
//!   Solo datos (`Component`) y lógica separada (`System`).
//!
//! El módulo expone:
//! - [`entity::EntityId`] — identificador generacional.
//! - [`component::Component`] — trait que define un tipo de dato.
//! - [`world::World`] — almacén de arquetipos + consultas + entidades.
//! - [`resource::Resources`] — recursos globales tipados por `TypeId`.

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
