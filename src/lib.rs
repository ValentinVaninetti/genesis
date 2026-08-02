//! # GENESIS
//!
//! Simulation engine for a universe whose laws are few, simple and general.
//! The goal is **not** to simulate life or chemistry: it is to program only
//! the fundamental rules and observe whether complexity emerges on its own.
//!
//! Main modules:
//! - [`ecs`]: a bespoke, data-oriented ECS based on archetypes.
//! - [`universe`]: facade (`Universe`), clock and global resources.
//! - [`scheduler`]: explicit ordering of systems + stage analysis.
//! - [`components`]: component catalog (position, velocity, energy…).
//! - [`systems`]: laws (for now, architecture demos).
//! - [`config`]: all physics in TOML, outside the code.
//! - [`serialization`]: save and resume complete universes.
//! - [`stats`]: per-tick metrics with history.
//! - [`math`]: `Vec3` and geometric primitives.
//! - [`physics`] / [`chemistry`]: reserved for future laws.
//!
//! # Mental model
//!
//! Everything that exists is an **entity** (initially just atoms). Entities
//! only have **data** (`Component`). The **laws** are independent `System`s,
//! explicitly ordered by the scheduler. No chemistry, no evolution: only
//! rules. If something life-like appears, it will be a consequence, never a
//! feature.

pub mod analysis;
pub mod chemistry;
pub mod components;
pub mod config;
pub mod ecs;
pub mod math;
pub mod physics;
pub mod rng;
pub mod scheduler;
pub mod serialization;
pub mod stats;
pub mod systems;
pub mod universe;
