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
//! - gravity (`G·m_i·m_j/r²`), if `systems.enable_gravity`;
//! - the *observed* bond (harmonic spring towards the LJ well minimum), if
//!   `systems.enable_bond_interaction`: a pair recorded in `Bonds` by the
//!   bond observation is given a physical coupling. The bond is still not a
//!   law of the species — it only exists because the observation measured it.
//!
//! The force pass is **parallel once per candidate pair** (rayon): the grid
//! reports every pair exactly once and each pair contributes its force
//! magnitude symmetrically to both ends (Newton's 3rd law, momentum conserved
//! exactly), so the potential is counted once. Each thread accumulates into a
//! private force array and the parts are merged in thread order, keeping the
//! run deterministic. Buffers are reused between ticks to avoid allocation
//! churn.
//!
//! It knows nothing about species, bonds or reactions: only mass, position,
//! charge and atomic type (which provides σ and ε). All the "chemistry"
//! emerges from here.

use crate::components::{Acceleration, AtomType, Bonds, Charge, Mass, Position};
use crate::config::Config;
use crate::math::Vec3;
use crate::physics::forces::{
    bond_harmonic_raw, coulomb_raw, equilibrium_distance, gravity_raw, smooth_cutoff, switched,
    well_curvature, ElementTable, LjTable, LJ_CUTOFF_FACTOR,
};
use crate::physics::grid::{min_image, Pair, Particle, SpatialGrid};
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::{BondEnergy, PotentialEnergy};
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

/// Per-thread accumulation buffers of one force pass: sparse `(particle, ΔF)`
/// pairs plus the local potential-energy (LJ + Coulomb + gravity) and
/// bond-energy sums. Merged in thread order at the end so the run stays
/// deterministic regardless of the number of threads.
type ForceParts = (Vec<(u32, Vec3)>, f64, f64);

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
    /// Binding threshold of the bond observation (`stats.bond_k_bind`), used as
    /// the truncation of the harmonic bond term.
    bond_k_bind: f64,
    // Reused buffers.
    particles: Vec<Particle>,
    types: Vec<u32>,
    charges: Vec<f64>,
    /// Candidate pairs of the current tick (each pair exactly once).
    pairs: Vec<Pair>,
    /// Slot → bonded slots (adjacency of the observed `Bonds`, only filled
    /// when the bond interaction is enabled).
    bond_of: Vec<Vec<u32>>,
    /// Entity index → particle slot (to resolve `Bonds` neighbors).
    index_to_slot: Vec<Option<u32>>,
    acc: Vec<Vec3>,
}

impl ForceSystem {
    pub fn new(cfg: &Config) -> Self {
        let mut elements = ElementTable::default_table();
        if let Err(sym) = elements.apply_overrides(&cfg.physics.elements) {
            panic!("[genesis] invalid affinity table: {sym}");
        }
        let lj = LjTable::new(&elements, cfg.physics.thermal_constant, LJ_CUTOFF_FACTOR);
        let max_sigma = AtomType::ALL
            .iter()
            .map(|&t| elements.sigma(t))
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
            bond_k_bind: cfg.stats.bond_k_bind,
            particles: Vec::new(),
            types: Vec::new(),
            charges: Vec::new(),
            pairs: Vec::new(),
            bond_of: Vec::new(),
            index_to_slot: Vec::new(),
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
            .reads::<Bonds>()
            .writes::<Acceleration>()
            .resource_read::<Config>()
            .resource_write::<PotentialEnergy>()
            .resource_write::<BondEnergy>()
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
        let use_bonds = cfg.systems.enable_bond_interaction;
        let k_e = cfg.physics.coulomb_constant;
        let g = cfg.physics.gravity_constant;
        let bond_k_bind = self.bond_k_bind;

        // Phase 1: collect the particles (position, mass, element, charge) and
        // the entity-index → slot map (to resolve `Bonds` neighbors).
        self.particles.clear();
        self.types.clear();
        self.charges.clear();
        self.index_to_slot.clear();
        self.index_to_slot.resize(capacity, None);
        let mut slot = 0u32;
        ctx.world.for_each3::<Position, Mass, AtomType>(|e, pos, mass, at| {
            self.index_to_slot[e.index() as usize] = Some(slot);
            self.particles.push(Particle {
                index: e.index(),
                pos: pos.0,
                vel: Vec3::ZERO,
                mass: mass.0,
            });
            self.types.push(element_index(*at) as u32);
            slot += 1;
        });
        if use_coulomb {
            ctx.world.for_each1::<Charge>(|_, c| {
                self.charges.push(c.0);
            });
        }
        if self.particles.is_empty() {
            return;
        }

        // Phase 1.5: adjacency of the observed bonds in slot space (only when
        // the bond interaction is enabled). Each persistent pair is a harmonic
        // spring towards the LJ equilibrium distance.
        if use_bonds {
            self.bond_of.clear();
            self.bond_of.resize(self.particles.len(), Vec::new());
            ctx.world.for_each1::<Bonds>(|e, bonds| {
                let Some(si) = self.index_to_slot[e.index() as usize] else {
                    return;
                };
                for &n in &bonds.neighbors {
                    if e.index() >= n.index() {
                        continue; // each pair once (the list is symmetric)
                    }
                    let Some(sj) = self.index_to_slot[n.index() as usize] else {
                        continue;
                    };
                    self.bond_of[si as usize].push(sj);
                    self.bond_of[sj as usize].push(si);
                }
            });
        }

        // Phase 2: broadphase — one grid rebuild, then every candidate pair
        // exactly once.
        self.grid.build(&self.particles);
        self.pairs.clear();
        self.grid.neighbors(&self.particles, self.rc, &mut self.pairs);

        // Phase 3: parallel once-per-pair pass. Each pair contributes its
        // force magnitude once; the symmetric accelerations ±F/m are applied
        // to both ends (Newton's 3rd law, momentum conserved exactly). The
        // potential is counted once per pair, so there is no halving. Each
        // split accumulates sparsely (only the touched slots), so the merge
        // stays O(pairs) no matter how rayon splits the pair list; the parts
        // are merged in split order, keeping the run deterministic.
        let lj = &self.lj;
        let particles = &self.particles;
        let types = &self.types;
        let charges = &self.charges;
        let bond_of = &self.bond_of;
        let coulomb_rc = self.coulomb_rc;
        let coulomb_r_on = self.coulomb_r_on;
        let gravity_rc = self.gravity_rc;
        let gravity_r_on = self.gravity_r_on;

        let parts: Vec<ForceParts> = self
            .pairs
            .par_iter()
            .fold(
                || (Vec::new(), 0.0, 0.0),
                |mut acc, pair| {
                    let (forces, v, vbond) = &mut acc;
                    let pa = &particles[pair.a];
                    let pb = &particles[pair.b];
                    let delta = min_image(pa.pos - pb.pos, world_size);
                    let d2 = delta.length_squared();
                    if d2 >= rc2 {
                        return acc;
                    }
                    let d = d2.sqrt();
                    if d <= f64::EPSILON {
                        return acc;
                    }
                    let normal = delta * (1.0 / d);
                    let mut fmag = 0.0;

                    // Lennard-Jones (always on, truncated at its own rc).
                    if d < lj.rc() {
                        let p = lj.pair_indexed(types[pair.a] as usize, types[pair.b] as usize);
                        let (m, vv) = lj.force_switched_scalar(p, d);
                        fmag += m;
                        *v += vv;
                    }

                    // Electrostatics: F = k_e·q_a·q_b/r² (with the same smooth
                    // switch so the total force stays continuous).
                    if use_coulomb && d < coulomb_rc {
                        let r = d.max(0.5);
                        let (m, vv) = coulomb_raw(k_e, charges[pair.a], charges[pair.b], r);
                        let (sw, dsw) = smooth_cutoff(d, coulomb_r_on, coulomb_rc);
                        let (m2, vv2) = switched(m, vv, sw, dsw);
                        fmag += m2;
                        *v += vv2;
                    }

                    // Gravity: F = −G·m_a·m_b/r², always attractive.
                    if use_gravity && d < gravity_rc {
                        let r = d.max(0.5);
                        let (m, vv) = gravity_raw(g, pa.mass, pb.mass, r);
                        let (sw, dsw) = smooth_cutoff(d, gravity_r_on, gravity_rc);
                        let (m2, vv2) = switched(m, vv, sw, dsw);
                        fmag += m2;
                        *v += vv2;
                    }

                    // Observed bond: harmonic spring towards the LJ well
                    // minimum, switched off before the binding radius so the
                    // force stays continuous (the pair only stays bonded while
                    // it remains bound).
                    if use_bonds && bond_of[pair.a].contains(&(pair.b as u32)) {
                        let p = lj.pair_indexed(types[pair.a] as usize, types[pair.b] as usize);
                        let r_eq = equilibrium_distance(p.sigma);
                        let rc_bond = bond_k_bind * p.sigma;
                        if d < rc_bond {
                            let k = well_curvature(p.epsilon, p.sigma);
                            let (m, vv) = bond_harmonic_raw(k, r_eq, d);
                            let (sw, dsw) = smooth_cutoff(d, r_eq, rc_bond);
                            let (m2, vv2) = switched(m, vv, sw, dsw);
                            fmag += m2;
                            *vbond += vv2;
                        }
                    }

                    if fmag != 0.0 {
                        forces.push((pair.a as u32, normal * fmag));
                        forces.push((pair.b as u32, normal * (-fmag)));
                    }
                    acc
                },
            )
            .collect();

        // Phase 4: merge the sparse per-split forces deterministically
        // (a = F/m) and sum the potentials (each pair counted exactly once).
        self.acc.clear();
        self.acc.resize(capacity, Vec3::ZERO);
        let mut potential = 0.0;
        let mut bond_potential = 0.0;
        for (forces, vi, vbi) in parts {
            potential += vi;
            bond_potential += vbi;
            for (s, x) in forces {
                self.acc[particles[s as usize].index as usize] +=
                    x * (1.0 / particles[s as usize].mass);
            }
        }

        ctx.world.par_for_each1_mut::<Acceleration>(|e, a| {
            a.0 = self.acc[e.index() as usize];
        });

        if let Some(pe) = ctx.resources.get_mut::<PotentialEnergy>() {
            pe.0 = potential;
        }
        if let Some(be) = ctx.resources.get_mut::<BondEnergy>() {
            be.0 = bond_potential;
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

    #[test]
    fn energy_conserves_with_a_fixed_observed_bond() {
        // One bond placed by hand (observation off, so nothing rewrites the
        // `Bonds` component) must conserve energy: the harmonic term of the
        // force pass is the exact derivative of its switched potential.
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = 8; // 2³ lattice
        cfg.universe.size = Vec3::new(32.0, 32.0, 32.0);
        cfg.physics.initial_temperature = 0.0;
        cfg.systems.enable_collisions = false;
        cfg.systems.enable_thermostat = false;
        cfg.systems.enable_bond_observation = false;
        cfg.systems.enable_bond_interaction = true;

        let mut u = Universe::new(cfg);
        let mut ents = Vec::new();
        u.world.for_each1::<crate::components::Position>(|e, _| ents.push(e));
        // Pin the elements (the seed picks random species) so the pair's LJ
        // parameters are known.
        for e in &ents {
            u.world.insert::<crate::components::AtomType>(*e, AtomType::Carbon);
            u.world.insert::<crate::components::Mass>(
                *e,
                crate::components::Mass(AtomType::Carbon.mass()),
            );
        }
        let lj = crate::physics::forces::LjTable::new(
            &crate::physics::forces::ElementTable::default_table(),
            u.config.physics.thermal_constant,
            LJ_CUTOFF_FACTOR,
        );
        let p = lj.pair(AtomType::Carbon, AtomType::Carbon);
        let r_eq = equilibrium_distance(p.sigma);

        // The pair, slightly stretched from the well minimum.
        let (e0, e1) = (ents[0], ents[1]);
        let half = r_eq * 0.5 + 0.15;
        u.world.get_mut::<crate::components::Position>(e0).unwrap().0 = Vec3::new(-half, 0.0, 0.0);
        u.world.get_mut::<crate::components::Position>(e1).unwrap().0 = Vec3::new(half, 0.0, 0.0);
        // The other atoms at far corners of a 32³ box: even through the
        // periodic wrap they stay at least 6 apart (rc ≈ 5.75), and 20+ away
        // from the pair, so nothing else interacts.
        let far = [
            Vec3::new(13.0, 13.0, 13.0),
            Vec3::new(13.0, 13.0, -13.0),
            Vec3::new(13.0, -13.0, 13.0),
            Vec3::new(13.0, -13.0, -13.0),
            Vec3::new(-13.0, 13.0, 13.0),
            Vec3::new(-13.0, 13.0, -13.0),
        ];
        for (k, e) in ents.iter().skip(2).enumerate() {
            u.world.get_mut::<crate::components::Position>(*e).unwrap().0 = far[k];
        }
        let mut b0 = crate::components::Bonds::default();
        b0.neighbors.push(e1);
        let mut b1 = crate::components::Bonds::default();
        b1.neighbors.push(e0);
        u.world.insert::<crate::components::Bonds>(e0, b0);
        u.world.insert::<crate::components::Bonds>(e1, b1);

        u.run_ticks(100); // cold relaxation (T = 0)
        let p0 = momentum(&u.world);
        let p0n = p0.length();
        // The two-atom total energy ripples with the oscillation phase, so
        // measure drift with window means instead of a single sample.
        let mean_e = |u: &mut Universe| -> f64 {
            let mut sum = 0.0;
            for _ in 0..100 {
                u.tick();
                sum += u.stats.snapshot.energy_total;
            }
            sum / 100.0
        };
        let e_ref = mean_e(&mut u);
        u.run_ticks(1500);
        let e_end = mean_e(&mut u);

        let rel = (e_end - e_ref).abs() / e_ref.abs();
        let dp = (momentum(&u.world) - p0).length();
        assert!(rel < 1e-3, "relative energy drift with a bond: {rel:.3e}");
        assert!(dp < 1e-6 * p0n.max(1.0), "momentum drift: {dp:.3e}");
        assert!(
            u.stats.snapshot.bond_energy > 0.0,
            "expected the observed bond to contribute energy"
        );
    }
}
