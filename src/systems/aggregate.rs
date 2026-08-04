//! `StatsSystem` — observation of the universe.
//!
//! It runs **last** in the schedule and aggregates a `StatsSnapshot` per tick.
//! It only reads: energy and temperature are **derived** from the velocities
//! (kinetic + equipartition), from the potential accumulated by the forces
//! and from the collision counter, instead of being stored as state.

use crate::components::{Mass, Velocity};
use crate::config::Config;
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::{
    BondEnergy, BondObservation, CollisionCounter, PotentialEnergy, StatsSnapshot, StructureStats,
};

pub struct StatsSystem;

impl System for StatsSystem {
    fn name(&self) -> &'static str {
        "stats"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Velocity>()
            .reads::<Mass>()
            .resource_read::<Config>()
            .resource_read::<CollisionCounter>()
            .resource_read::<PotentialEnergy>()
            .resource_read::<StructureStats>()
            .resource_read::<BondObservation>()
            .resource_read::<BondEnergy>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let cfg = ctx.resources.get::<Config>();
        let collisions = ctx
            .resources
            .get::<CollisionCounter>()
            .map(|c| c.0)
            .unwrap_or(0);
        let energy_potential = ctx
            .resources
            .get::<PotentialEnergy>()
            .map(|pe| pe.0)
            .unwrap_or(0.0);
        let structure = ctx
            .resources
            .get::<StructureStats>()
            .copied()
            .filter(|s| s.tick > 0);
        let bonded_pairs = ctx
            .resources
            .get::<BondObservation>()
            .map(|b| b.bonded_pairs)
            .unwrap_or(0);
        let bonds_formed = ctx
            .resources
            .get::<BondObservation>()
            .map(|b| b.bonds_formed)
            .unwrap_or(0);
        let bond_lifetime_ticks = ctx.resources.get::<BondObservation>().map_or(0.0, |b| {
            if b.bonds_formed > 0 {
                b.lifetime_sum_ticks / b.bonds_formed as f64
            } else {
                0.0
            }
        });
        let bond_matrix = ctx
            .resources
            .get::<BondObservation>()
            .map(|b| b.species_matrix.iter().map(|&c| c as f64).collect())
            .unwrap_or_default();
        let bond_energy = ctx
            .resources
            .get::<BondEnergy>()
            .map(|be| be.0)
            .unwrap_or(0.0);

        let mut kinetic = 0.0;
        let mut speed_sum = 0.0;
        let mut count = 0usize;
        ctx.world.for_each2::<Velocity, Mass>(|_, v, m| {
            kinetic += 0.5 * m.0 * v.0.length_squared();
            speed_sum += v.0.length();
            count += 1;
        });

        // Equipartition: with 3 translational degrees of freedom,
        // T = (2/3)·⟨K⟩/k.
        let k_eff = cfg.map(|c| c.physics.thermal_constant).unwrap_or(0.0);
        let energy_avg = if count > 0 {
            kinetic / count as f64
        } else {
            0.0
        };
        let temperature_avg = if k_eff > 0.0 {
            (2.0 / 3.0) * energy_avg / k_eff
        } else {
            0.0
        };

        let volume = cfg.map(|c| c.universe.size.x * c.universe.size.y * c.universe.size.z);
        let density = match volume {
            Some(v) if v > 0.0 => ctx.world.len() as f64 / v,
            _ => 0.0,
        };

        let snapshot = StatsSnapshot {
            tick: ctx.time.tick,
            time: ctx.time.t,
            entities: ctx.world.len(),
            // Total energy: kinetic + potential (including the observed-bond
            // harmonic term), conserved by Verlet in every mode.
            energy_total: kinetic + energy_potential + bond_energy,
            energy_avg,
            energy_potential,
            temperature_avg,
            mean_speed: if count > 0 {
                speed_sum / count as f64
            } else {
                0.0
            },
            density,
            collisions,
            systems_run: ctx.stats.systems_run,
            fps: ctx.stats.fps,
            memory_bytes: ctx.world.memory_bytes(),
            structure,
            bonded_pairs,
            bonds_formed,
            bond_lifetime_ticks,
            bond_energy,
            bond_matrix,
        };
        ctx.stats.record(snapshot);
    }
}
