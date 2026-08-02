//! Definition of the `System` trait and the execution context.
//!
//! A system is a **law** or **transformation** that runs every tick over the
//! `World`. Each system explicitly declares which components it reads and
//! which it writes: that is the information that lets the scheduler validate
//! the order, detect conflicts and (in the future) run stages in parallel.

use crate::ecs::{ComponentId, Resources, World};
use crate::rng::Rng;
use crate::stats::StatsCollector;
use crate::universe::Time;
use std::any::{Any, TypeId};

/// Access declared by a system to components and resources.
#[derive(Debug, Clone, Default)]
pub struct Access {
    /// Components read (not modified).
    pub reads: Vec<ComponentId>,
    /// Components written.
    pub writes: Vec<ComponentId>,
    /// Resources read.
    pub resources_read: Vec<TypeId>,
    /// Resources written.
    pub resources_write: Vec<TypeId>,
}

impl Access {
    pub fn reads<C: crate::ecs::Component>(mut self) -> Self {
        self.reads.push(C::ID);
        self
    }

    pub fn writes<C: crate::ecs::Component>(mut self) -> Self {
        self.writes.push(C::ID);
        self
    }

    pub fn resource_read<R: Any + Send + Sync>(mut self) -> Self {
        self.resources_read.push(TypeId::of::<R>());
        self
    }

    pub fn resource_write<R: Any + Send + Sync>(mut self) -> Self {
        self.resources_write.push(TypeId::of::<R>());
        self
    }

    /// Two accesses conflict if they share a resource or component and at
    /// least one of the two writes it.
    pub fn conflicts_with(&self, other: &Access) -> bool {
        let component_conflict = self
            .writes
            .iter()
            .any(|c| other.reads.contains(c) || other.writes.contains(c))
            || other
                .writes
                .iter()
                .any(|c| self.reads.contains(c) || self.writes.contains(c));
        let resource_conflict = self
            .resources_write
            .iter()
            .any(|c| other.resources_read.contains(c) || other.resources_write.contains(c))
            || other
                .resources_write
                .iter()
                .any(|c| self.resources_read.contains(c) || self.resources_write.contains(c));
        component_conflict || resource_conflict
    }
}

/// Execution context of a system.
///
/// Gives access to everything a law needs without coupling to the full
/// `Universe`: world, resources, randomness, time and statistics.
pub struct SystemContext<'a> {
    pub world: &'a mut World,
    pub resources: &'a mut Resources,
    pub rng: &'a mut Rng,
    pub time: &'a Time,
    pub stats: &'a mut StatsCollector,
    /// Time delta of this tick.
    pub dt: f64,
}

/// A law of the universe. It must be deterministic except for `ctx.rng`.
pub trait System: Send + Sync {
    /// Short and stable name (for logs and debug).
    fn name(&self) -> &'static str;

    /// Access declaration (by default: no declared access).
    fn access(&self) -> Access {
        Access::default()
    }

    /// Runs the law.
    fn run(&mut self, ctx: &mut SystemContext<'_>);
}
