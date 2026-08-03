//! `ForceSystem` — Lennard-Jones intermolecular forces.
//!
//! It is the **only** law that creates interaction between particles beyond
//! the collision impulse. Using the same spatial grid as collisions, it
//! computes the LJ force of each pair within the cutoff and accumulates it as
//! acceleration in the `Acceleration` component (consumed by the Verlet
//! integrator). It also accumulates the total potential energy in the
//! `PotentialEnergy` resource so statistics can report `E = K + V`.
//!
//! The force pass is **parallel per particle** (rayon): each particle queries
//! its own neighborhood and accumulates only its own acceleration into a
//! private buffer, so there are no races and the pass scales with the cores.
//! Every pair is visited twice, once from each end — the forces are exactly
//! opposite (Newton's 3rd law, momentum conserved) and the potential is
//! halved. The result is collected by slot index, keeping the run
//! deterministic.
//!
//! It knows nothing about species, bonds or reactions: only mass, position
//! and atomic type (which provides σ and ε). All the "chemistry" emerges from
//! here.

use crate::components::{Acceleration, AtomType, Mass, Position};
use crate::config::Config;
use crate::math::Vec3;
use crate::physics::forces::{LjTable, LJ_CUTOFF_FACTOR};
use crate::physics::grid::{min_image, Particle, SpatialGrid};
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::PotentialEnergy;
use rayon::prelude::*;

fn element_index(t: AtomType) -> usize {
    match t {
        AtomType::Hydrogen => 0,
        AtomType::Helium => 1,
        AtomType::Carbon => 2,
        AtomType::Nitrogen => 3,
        AtomType::Oxygen => 4,
        AtomType::Sodium => 5,
        AtomType::Silicon => 6,
        AtomType::Phosphorus => 7,
        AtomType::Sulfur => 8,
        AtomType::Iron => 9,
    }
}

/// Force system. The grid is reused between ticks and fully rebuilt in each
/// `run` (same pattern as `CollisionSystem`).
pub struct ForceSystem {
    grid: SpatialGrid,
    lj: LjTable,
    rc: f64,
}

impl ForceSystem {
    pub fn new(cfg: &Config) -> Self {
        let lj = LjTable::new(cfg.physics.thermal_constant, LJ_CUTOFF_FACTOR);
        let rc = lj.rc();
        // Cells of at least the cutoff: two interacting particles can only
        // live in the same cell or in neighbor cells.
        let grid = SpatialGrid::new(cfg.universe.size, rc);
        Self { grid, lj, rc }
    }
}

impl System for ForceSystem {
    fn name(&self) -> &'static str {
        "forces"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Position>()
            .reads::<Mass>()
            .reads::<AtomType>()
            .writes::<Acceleration>()
            .resource_read::<Config>()
            .resource_write::<PotentialEnergy>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let Some(cfg) = ctx.resources.get::<Config>() else {
            return;
        };
        let world_size = cfg.universe.size;
        let rc2 = self.rc * self.rc;
        let capacity = ctx.world.entity_capacity();

        // Phase 1: collect the particles (position, mass, element).
        let mut particles: Vec<Particle> = Vec::with_capacity(ctx.world.len());
        let mut types: Vec<u32> = Vec::with_capacity(ctx.world.len());
        ctx.world.for_each3::<Position, Mass, AtomType>(|e, pos, mass, at| {
            particles.push(Particle {
                index: e.index(),
                pos: pos.0,
                vel: Vec3::ZERO,
                mass: mass.0,
            });
            types.push(element_index(*at) as u32);
        });
        if particles.is_empty() {
            return;
        }

        // Phase 2: broadphase — one grid rebuild per tick.
        self.grid.build(&particles);

        // Phase 3: parallel per-particle forces (a = F/m) and potential.
        //
        // Each particle queries its own neighborhood and accumulates only its
        // own force into a private buffer, so the pass is race-free and fully
        // parallel. Each pair is visited twice (once per end): the forces are
        // exactly opposite (Newton's 3rd law, momentum conserved) and the
        // potential must be halved at the end. The per-slot results are
        // collected in order, so the run stays deterministic.
        let per_slot: Vec<(Vec3, f64)> = (0..particles.len())
            .into_par_iter()
            .map_init(Vec::new, |buf, i| {
                let a = &particles[i];
                let mut force = Vec3::ZERO;
                let mut local_v = 0.0;
                self.grid.neighbors_of(&particles, i, self.rc, buf);
                for &j in buf.iter() {
                    let b = &particles[j as usize];
                    let delta = min_image(a.pos - b.pos, world_size);
                    let d2 = delta.length_squared();
                    if d2 >= rc2 {
                        continue;
                    }
                    let d = d2.sqrt();
                    if d <= f64::EPSILON {
                        continue;
                    }
                    let normal = delta * (1.0 / d);
                    let p = self.lj.pair_indexed(types[i] as usize, types[j as usize] as usize);
                    let (f, v) = self.lj.force_switched(p, d, normal);
                    force += f / a.mass;
                    local_v += v;
                }
                (force, local_v)
            })
            .collect();

        // Phase 4: apply accelerations by entity index and sum the potential
        // (each pair counted twice → half).
        let mut acc = vec![Vec3::ZERO; capacity];
        let mut potential = 0.0;
        for (slot, (f, v)) in per_slot.iter().enumerate() {
            acc[particles[slot].index as usize] = *f;
            potential += v;
        }
        potential *= 0.5;

        ctx.world.par_for_each1_mut::<Acceleration>(|e, a| {
            a.0 = acc[e.index() as usize];
        });

        if let Some(pe) = ctx.resources.get_mut::<PotentialEnergy>() {
            pe.0 = potential;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::universe::Universe;

    fn momentum(w: &crate::ecs::World) -> Vec3 {
        let mut p = Vec3::ZERO;
        w.for_each2::<crate::components::Velocity, Mass>(|_, v, m| p += v.0 * m.0);
        p
    }

    #[test]
    fn energy_and_momentum_conserve_with_forces() {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = 512; // 8³: exact cubic lattice
        cfg.universe.size = Vec3::new(24.0, 24.0, 24.0);
        cfg.physics.initial_temperature = 100.0;
        cfg.physics.thermal_constant = 0.01;
        cfg.systems.enable_forces = true;
        cfg.systems.enable_collisions = false;

        let mut u = Universe::new(cfg);
        let p0 = momentum(&u.world);
        let p0n = p0.length();

        // Initial relaxation: the cold lattice reorganizes; total energy is
        // already conserved from the first tick (Verlet + internal forces).
        u.run_ticks(300);

        let e_ref = u.stats.snapshot.energy_total;
        assert!(e_ref > 0.0, "total energy not positive: {e_ref}");
        assert!(
            u.stats.snapshot.energy_potential < 0.0,
            "expected net attraction (V < 0), got {}",
            u.stats.snapshot.energy_potential
        );

        u.run_ticks(2000);

        let e1 = u.stats.snapshot.energy_total;
        let rel = (e1 - e_ref).abs() / e_ref.abs();
        let dp = (momentum(&u.world) - p0).length();
        assert!(rel < 1e-3, "relative energy drift: {rel:.3e}");
        assert!(dp < 1e-6 * p0n.max(1.0), "momentum drift: {dp:.3e}");
    }
}
