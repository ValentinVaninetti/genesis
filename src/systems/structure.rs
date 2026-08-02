//! `StructureSystem` — observación de estructura emergente.
//!
//! Cada `interval` ticks detecta los agregados de átomos (friends-of-friends,
//! `analysis::clusters`) y guarda el resumen en el recurso `StructureStats`,
//! que `StatsSystem` copia al snapshot. No es una ley: **no modifica** nada
//! del mundo; solo mide qué ha producido la física.

use crate::analysis::clusters;
use crate::components::{AtomType, Position};
use crate::config::Config;
use crate::math::Vec3;
use crate::physics::grid::Particle;
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::StructureStats;

/// Sistema de análisis de estructura.
pub struct StructureSystem {
    interval: u64,
}

impl StructureSystem {
    pub fn new(interval: u64) -> Self {
        Self {
            interval: interval.max(1),
        }
    }
}

impl System for StructureSystem {
    fn name(&self) -> &'static str {
        "structure"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Position>()
            .reads::<AtomType>()
            .resource_read::<Config>()
            .resource_write::<StructureStats>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        if !ctx.time.tick.is_multiple_of(self.interval) {
            return;
        }
        let Some(cfg) = ctx.resources.get::<Config>() else {
            return;
        };

        let mut particles: Vec<Particle> = Vec::with_capacity(ctx.world.len());
        let mut types: Vec<AtomType> = Vec::with_capacity(ctx.world.len());
        ctx.world.for_each2::<Position, AtomType>(|e, pos, at| {
            particles.push(Particle {
                index: e.index(),
                pos: pos.0,
                vel: Vec3::ZERO,
                mass: 0.0,
            });
            types.push(*at);
        });
        let c = clusters(&particles, &types, cfg.universe.size, crate::analysis::BOND_FACTOR);

        if let Some(ss) = ctx.resources.get_mut::<StructureStats>() {
            ss.tick = ctx.time.tick;
            ss.monomers = c.monomers;
            ss.aggregates = c.aggregates;
            ss.largest = c.largest;
            ss.mean_size = c.mean_size;
            ss.bound_pairs = c.bound_pairs;
        }
    }
}
