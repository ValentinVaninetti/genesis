//! `ForceSystem` — the intermolecular force pass (Lennard-Jones + optional
//! electrostatics and gravity).
//!
//! It is the **only** place that creates interaction between particles beyond
//! the collision impulse. Using the same spatial grid as collisions, it
//! computes the force of each pair within the cutoff and accumulates it as
//! acceleration in the `Acceleration` component (consumed by the Verlet
//! integrator). It also accumulates the total potential energy in the
//! `PotentialEnergy` resource so statistics can report `E = K + V`.
//!
//! Three independent laws are composed here, all truncated with the same
//! smooth switch so the total force stays continuous and energy is conserved:
//!
//! - Lennard-Jones, always on, per species (σ, ε);
//! - electrostatics (Coulomb, `k_e·q_i·q_j/r²`), if
//!   `systems.enable_electrostatics`, using the per-species charge;
//! - gravity (`G·m_i·m_j/r²`), if `systems.enable_gravity`.
//!
//! The force pass is **parallel per particle** (rayon): each particle queries
//! its own neighborhood and accumulates only its own acceleration into a
//! private buffer, so there are no races and the pass scales with the cores.
//! Every pair is visited twice, once from each end — the forces are exactly
//! opposite (Newton's 3rd law, momentum conserved) and the potential is
//! halved. The result is collected by slot index, keeping the run
//! deterministic. Buffers are reused between ticks to avoid allocation churn.
//!
//! It knows nothing about species, bonds or reactions: only mass, position,
//! charge and atomic type (which provides σ and ε). All the "chemistry"
//! emerges from here.

use crate::components::{Acceleration, AtomType, Charge, Mass, Position};
use crate::config::Config;
use crate::math::Vec3;
use crate::physics::forces::{
    coulomb_raw, gravity_raw, smooth_cutoff, switched, LjTable, LJ_CUTOFF_FACTOR,
};
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
/// `run` (same pattern as `CollisionSystem`). The per-tick buffers are also
/// reused so the hot path only touches alive particles.
pub struct ForceSystem {
    grid: SpatialGrid,
    lj: LjTable,
    /// Largest enabled cutoff (the grid cell size).
    rc: f64,
    /// Cutoff of the electrostatics term (0 when disabled).
    coulomb_rc: f64,
    coulomb_r_on: f64,
    /// Cutoff of the gravitational term (0 when disabled).
    gravity_rc: f64,
    gravity_r_on: f64,
    // Reused buffers.
    particles: Vec<Particle>,
    types: Vec<u32>,
    charges: Vec<f64>,
    per_slot: Vec<(Vec3, f64)>,
    acc: Vec<Vec3>,
}

impl ForceSystem {
    pub fn new(cfg: &Config) -> Self {
        let lj = LjTable::new(cfg.physics.thermal_constant, LJ_CUTOFF_FACTOR);
        let max_sigma = AtomType::ALL
            .iter()
            .map(|&t| crate::physics::forces::sigma(t))
            .fold(0.0, f64::max);
        let lj_rc = lj.rc();
        let coulomb_rc = if cfg.systems.enable_electrostatics {
            cfg.physics.coulomb_cutoff.max(1.0) * max_sigma
        } else {
            0.0
        };
        let gravity_rc = if cfg.systems.enable_gravity {
            cfg.physics.gravity_cutoff.max(1.0) * max_sigma
        } else {
            0.0
        };
        let rc = lj_rc.max(coulomb_rc).max(gravity_rc);
        // Cells of at least the cutoff: two interacting particles can only
        // live in the same cell or in neighbor cells.
        let grid = SpatialGrid::new(cfg.universe.size, rc);
        Self {
            grid,
            lj,
            rc,
            coulomb_rc,
            coulomb_r_on: 0.9 * coulomb_rc,
            gravity_rc,
            gravity_r_on: 0.9 * gravity_rc,
            particles: Vec::new(),
            types: Vec::new(),
            charges: Vec::new(),
            per_slot: Vec::new(),
            acc: Vec::new(),
        }
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
            .reads::<Charge>()
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
        let use_coulomb = cfg.systems.enable_electrostatics;
        let use_gravity = cfg.systems.enable_gravity;
        let k_e = cfg.physics.coulomb_constant;
        let g = cfg.physics.gravity_constant;

        // Phase 1: collect the particles (position, mass, element, charge).
        self.particles.clear();
        self.types.clear();
        self.charges.clear();
        ctx.world.for_each3::<Position, Mass, AtomType>(|e, pos, mass, at| {
            self.particles.push(Particle {
                index: e.index(),
                pos: pos.0,
                vel: Vec3::ZERO,
                mass: mass.0,
            });
            self.types.push(element_index(*at) as u32);
        });
        if use_coulomb {
            ctx.world.for_each1::<Charge>(|_, c| {
                self.charges.push(c.0);
            });
        }
        if self.particles.is_empty() {
            return;
        }

        // Phase 2: broadphase — one grid rebuild per tick.
        self.grid.build(&self.particles);

        // Phase 3: parallel per-particle forces (a = F/m) and potential.
        //
        // Each particle queries its own neighborhood and accumulates only its
        // own force into a private buffer, so the pass is race-free and fully
        // parallel. Each pair is visited twice (once per end): the forces are
        // exactly opposite (Newton's 3rd law, momentum conserved) and the
        // potential must be halved at the end. The per-slot results are
        // collected in order, so the run stays deterministic.
        let lj = &self.lj;
        let grid = &self.grid;
        let particles = &self.particles;
        let types = &self.types;
        let charges = &self.charges;
        let rc = self.rc;
        let coulomb_rc = self.coulomb_rc;
        let coulomb_r_on = self.coulomb_r_on;
        let gravity_rc = self.gravity_rc;
        let gravity_r_on = self.gravity_r_on;

        self.per_slot = (0..particles.len())
            .into_par_iter()
            .map_init(Vec::new, |buf, i| {
                let a = &particles[i];
                let mut force = Vec3::ZERO;
                let mut local_v = 0.0;
                grid.neighbors_of(particles, i, rc, buf);
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

                    // Lennard-Jones (always on, truncated at its own rc).
                    if d < lj.rc() {
                        let p = lj.pair_indexed(types[i] as usize, types[j as usize] as usize);
                        let (f, v) = lj.force_switched(p, d, normal);
                        force += f / a.mass;
                        local_v += v;
                    }

                    // Electrostatics: F = k_e·q_a·q_b/r² (with the same smooth
                    // switch so the total force stays continuous).
                    if use_coulomb && d < coulomb_rc {
                        let r = d.max(0.5);
                        let (m, v) = coulomb_raw(k_e, charges[i], charges[j as usize], r);
                        let (sw, dsw) = smooth_cutoff(d, coulomb_r_on, coulomb_rc);
                        let (f_mag, v_sw) = switched(m, v, sw, dsw);
                        force += normal * (f_mag / a.mass);
                        local_v += v_sw;
                    }

                    // Gravity: F = −G·m_a·m_b/r², always attractive.
                    if use_gravity && d < gravity_rc {
                        let r = d.max(0.5);
                        let (m, v) = gravity_raw(g, a.mass, b.mass, r);
                        let (sw, dsw) = smooth_cutoff(d, gravity_r_on, gravity_rc);
                        let (f_mag, v_sw) = switched(m, v, sw, dsw);
                        force += normal * (f_mag / a.mass);
                        local_v += v_sw;
                    }
                }
                (force, local_v)
            })
            .collect();

        // Phase 4: apply accelerations by entity index and sum the potential
        // (each pair counted twice → half).
        self.acc.clear();
        self.acc.resize(capacity, Vec3::ZERO);
        let mut potential = 0.0;
        for (slot, (f, v)) in self.per_slot.iter().enumerate() {
            self.acc[particles[slot].index as usize] = *f;
            potential += v;
        }
        potential *= 0.5;

        ctx.world.par_for_each1_mut::<Acceleration>(|e, a| {
            a.0 = self.acc[e.index() as usize];
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

    #[test]
    fn energy_conserves_with_electrostatics_and_gravity() {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = 512; // 8³: exact cubic lattice
        cfg.universe.size = Vec3::new(24.0, 24.0, 24.0);
        cfg.physics.initial_temperature = 100.0;
        cfg.systems.enable_forces = true;
        cfg.systems.enable_collisions = false;
        cfg.systems.enable_electrostatics = true;
        cfg.systems.enable_gravity = true;
        // Small but visible: at the LJ scale a G of ~1e-3 is a weak long-range
        // attraction on top of the repulsion.
        cfg.physics.gravity_constant = 1e-3;
        cfg.physics.coulomb_constant = 1.0;

        let mut u = Universe::new(cfg);
        let p0n = momentum(&u.world).length();
        u.run_ticks(300);
        // With net attraction (ionic + gravity) the total energy is negative;
        // only the conservation matters here.
        let e_ref = u.stats.snapshot.energy_total;

        u.run_ticks(2000);
        let e1 = u.stats.snapshot.energy_total;
        let rel = (e1 - e_ref).abs() / e_ref.abs();
        let dp = (momentum(&u.world)).length() - p0n;
        assert!(rel < 1e-3, "relative energy drift with E+G: {rel:.3e}");
        assert!(dp.abs() < 1e-6 * p0n.max(1.0), "momentum drift: {dp:.3e}");
    }

    #[test]
    fn bond_observation_records_persistent_pairs() {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = 216; // 6³ lattice, dense enough to jam
        cfg.universe.size = Vec3::new(24.0, 24.0, 24.0);
        cfg.physics.initial_temperature = 300.0;
        cfg.physics.thermostat_temperature = 80.0;
        cfg.systems.enable_thermostat = true;
        cfg.systems.enable_bond_observation = true;

        let mut u = Universe::new(cfg);
        // Long cold quench: persistent pairs must appear.
        u.run_ticks(6000);
        let bonded = u.stats.snapshot.bonded_pairs;
        assert!(
            bonded > 0,
            "expected persistent bonds after a cold quench, got {bonded}"
        );
    }
}
