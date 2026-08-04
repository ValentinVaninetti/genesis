//! `BondObservationSystem` — persistent-bond observation (not a law).
//!
//! No bond law exists in the engine: binding **emerges** from the force laws
//! and is *measured* here. Each tick the system finds the pairs within the
//! binding threshold (`r < bond_k_bind·σ_ij`, mixed per pair) and feeds them
//! to a `PairTracker`. A pair whose bound episode survives
//! `stats.bond_min_periods` **vibrational periods of its own pair**
//! (`T_vib = 2π·√(μ/k_well)`) is recorded in the `Bonds` component — the
//! architectural placeholder for exactly this observation.
//!
//! The `Bonds` component is updated every tick (persistent pairs in, broken
//! pairs out) and a `BondObservation` resource summarizes the state for
//! statistics and exports.

use crate::analysis::pairs::{bind_cutoff, bound_pairs_with_grid, BoundPair, PairTracker};
use crate::components::{AtomType, Bonds, Position};
use crate::config::Config;
use crate::ecs::EntityId;
use crate::physics::forces::{mix_epsilon, mix_sigma, vib_period};
use crate::physics::grid::SpatialGrid;
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::BondObservation;
use std::collections::HashMap;

/// Debounce (in ticks) before a pair is considered broken: consecutive ticks
/// out of the threshold required to end an episode.
pub const DEBOUNCE: u64 = 2;

pub struct BondObservationSystem {
    grid: SpatialGrid,
    k_bind: f64,
    min_periods: f64,
    tracker: PairTracker,
    bound: Vec<BoundPair>,
}

impl BondObservationSystem {
    pub fn new(cfg: &Config) -> Self {
        Self {
            grid: SpatialGrid::new(cfg.universe.size, bind_cutoff(cfg.stats.bond_k_bind)),
            k_bind: cfg.stats.bond_k_bind,
            min_periods: cfg.stats.bond_min_periods.max(0.1),
            tracker: PairTracker::new(DEBOUNCE),
            bound: Vec::new(),
        }
    }
}

impl System for BondObservationSystem {
    fn name(&self) -> &'static str {
        "bond-observation"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Position>()
            .reads::<AtomType>()
            .writes::<Bonds>()
            .resource_read::<Config>()
            .resource_write::<BondObservation>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let Some(cfg) = ctx.resources.get::<Config>() else {
            return;
        };
        let dt = cfg.universe.dt;
        let world_size = cfg.universe.size;

        // 1. Current bound pairs and the tracker (episodes).
        self.bound.clear();
        self.bound.extend(bound_pairs_with_grid(
            ctx.world,
            world_size,
            self.k_bind,
            &mut self.grid,
        ));
        self.tracker.track_tick(&self.bound);

        // 2. Which open episodes have already survived the persistence
        // threshold? The threshold is per pair: `min_periods` of its own
        // vibrational period.
        let threshold_ticks = |a: AtomType, b: AtomType| -> u64 {
            let mu = a.mass() * b.mass() / (a.mass() + b.mass());
            let eps = mix_epsilon(cfg.physics.thermal_constant, a, b);
            let sig = mix_sigma(a, b);
            (self.min_periods * vib_period(eps, sig, mu) / dt).ceil() as u64
        };
        let mut persistent: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
        for (pair, ticks) in self.tracker.open_pairs() {
            let (Some(ta), Some(tb)) = (
                ctx.world.get::<AtomType>(pair.a),
                ctx.world.get::<AtomType>(pair.b),
            ) else {
                continue;
            };
            if ticks < threshold_ticks(*ta, *tb) {
                continue;
            }
            persistent.entry(pair.a).or_default().push(pair.b);
            persistent.entry(pair.b).or_default().push(pair.a);
        }

        // 3. Write the Bonds component (persistent pairs in, broken out).
        ctx.world.par_for_each1_mut::<Bonds>(|e, bonds| {
            bonds.neighbors.clear();
            if let Some(list) = persistent.get(&e) {
                bonds.neighbors.extend_from_slice(list);
            }
        });

        // 4. Summarize for statistics/exports.
        if let Some(bo) = ctx.resources.get_mut::<BondObservation>() {
            let mut bonded_pairs = 0usize;
            for (_, list) in persistent.iter() {
                bonded_pairs += list.len();
            }
            bonded_pairs /= 2;
            let bonded_entities = persistent.len();
            let mean_coordination = if bonded_entities > 0 {
                2.0 * bonded_pairs as f64 / bonded_entities as f64
            } else {
                0.0
            };
            bo.tick = ctx.time.tick;
            bo.bonded_pairs = bonded_pairs;
            bo.bonded_entities = bonded_entities;
            bo.mean_coordination = mean_coordination;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Acceleration, Mass, Velocity};
    use crate::config::Config;
    use crate::ecs::World;
    use crate::math::Vec3;

    fn small_world(positions: &[(Vec3, AtomType)]) -> World {
        crate::components::register_all();
        let mut w = World::new();
        for &(pos, at) in positions {
            let e = w.spawn();
            w.insert::<Position>(e, Position(pos));
            w.insert::<AtomType>(e, at);
            w.insert::<Mass>(e, Mass(at.mass()));
            w.insert::<Velocity>(e, Velocity(Vec3::ZERO));
            w.insert::<Acceleration>(e, Acceleration(Vec3::ZERO));
            w.insert::<Bonds>(e, Bonds::default());
        }
        w
    }

    #[test]
    fn binds_a_close_pair_and_drops_it_after_separation() {
        let cfg = Config::default_config();
        let size = cfg.universe.size;
        let world = small_world(&[
            (Vec3::new(0.0, 0.0, 0.0), AtomType::Carbon),
            (Vec3::new(2.0, 0.0, 0.0), AtomType::Carbon),
        ]);
        let mut sys = BondObservationSystem::new(&cfg);

        // C–C at 2.0 < 1.5·1.9 = 2.85 is bound.
        let mut bound = bound_pairs_with_grid(&world, size, sys.k_bind, &mut sys.grid);
        assert!(!bound.is_empty());
        let mut tracker = PairTracker::new(DEBOUNCE);
        for _ in 0..50 {
            tracker.track_tick(&bound);
        }
        // A one-tick gap is absorbed by the debounce; the episode stays open.
        bound.clear();
        tracker.track_tick(&bound);
        assert_eq!(tracker.open_count(), 1);
        // After `DEBOUNCE` consecutive out-of-threshold ticks it closes.
        tracker.track_tick(&bound);
        tracker.track_tick(&bound);
        assert_eq!(tracker.open_count(), 0);
    }

    #[test]
    fn persistence_threshold_is_per_pair() {
        let cfg = Config::default_config();
        let dt = cfg.universe.dt;
        let min = cfg.stats.bond_min_periods;
        let threshold = |a: AtomType, b: AtomType| {
            (min
                * vib_period(
                    mix_epsilon(cfg.physics.thermal_constant, a, b),
                    mix_sigma(a, b),
                    a.mass() * b.mass() / (a.mass() + b.mass()),
                )
                / dt)
                .ceil() as u64
        };
        // The light/shallow H–H pair vibrates faster than the heavy/deep
        // Fe–Fe pair (μ compensates ε), so its persistence threshold in ticks
        // is smaller: the same episode length counts as "persistent" earlier
        // for the fast pair. The threshold must therefore be per pair.
        let t_hh = threshold(AtomType::Hydrogen, AtomType::Hydrogen);
        let t_fefe = threshold(AtomType::Iron, AtomType::Iron);
        assert!(t_hh < t_fefe, "H–H {t_hh} should bind earlier than Fe–Fe {t_fefe}");
    }
}
