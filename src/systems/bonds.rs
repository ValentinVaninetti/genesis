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
use crate::physics::forces::{element_index, mix_epsilon, mix_sigma, vib_period};
use crate::physics::grid::SpatialGrid;
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::BondObservation;
use std::collections::{HashMap, HashSet};

/// Debounce (in ticks) before a pair is considered broken: consecutive ticks
/// out of the threshold required to end an episode.
pub const DEBOUNCE: u64 = 2;

pub struct BondObservationSystem {
    grid: SpatialGrid,
    k_bind: f64,
    min_periods: f64,
    tracker: PairTracker,
    bound: Vec<BoundPair>,
    /// Persistent pairs currently active, with the tick they first reached the
    /// persistence threshold (to measure bond lifetimes).
    active_since: HashMap<BoundPair, u64>,
}

impl BondObservationSystem {
    pub fn new(cfg: &Config) -> Self {
        Self {
            grid: SpatialGrid::new(cfg.universe.size, bind_cutoff(cfg.stats.bond_k_bind)),
            k_bind: cfg.stats.bond_k_bind,
            min_periods: cfg.stats.bond_min_periods.max(0.1),
            tracker: PairTracker::new(DEBOUNCE),
            bound: Vec::new(),
            active_since: HashMap::new(),
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

        // 4. Summarize for statistics/exports: counts, per-species bond
        // matrix and bond lifetimes (a persistent pair that breaks is a
        // "formed" bond of that lifetime).
        if let Some(bo) = ctx.resources.get_mut::<BondObservation>() {
            let mut bonded_pairs = 0usize;
            let mut matrix = vec![0u64; AtomType::COUNT * AtomType::COUNT];
            let mut current: HashSet<BoundPair> = HashSet::with_capacity(persistent.len());
            for (e, list) in persistent.iter() {
                let Some(ta) = ctx.world.get::<AtomType>(*e) else {
                    continue;
                };
                for &n in list {
                    let pair = BoundPair {
                        a: (*e).min(n),
                        b: (*e).max(n),
                    };
                    if !current.insert(pair) {
                        continue;
                    }
                    bonded_pairs += 1;
                    let Some(tb) = ctx.world.get::<AtomType>(n) else {
                        continue;
                    };
                    let (ia, ib) = (element_index(*ta), element_index(*tb));
                    let c = AtomType::COUNT;
                    matrix[ia * c + ib] += 1;
                    matrix[ib * c + ia] += 1;
                }
            }

            // Bond lifetimes: a pair active in a previous tick that is no
            // longer persistent has broken — count it as a formed bond.
            let now = ctx.time.tick;
            let broken: Vec<BoundPair> = self
                .active_since
                .keys()
                .copied()
                .filter(|p| !current.contains(p))
                .collect();
            for p in broken {
                if let Some(since) = self.active_since.remove(&p) {
                    bo.bonds_formed += 1;
                    bo.lifetime_sum_ticks += (now - since) as f64;
                }
            }
            for p in &current {
                self.active_since.entry(*p).or_insert(now);
            }

            let bonded_entities = persistent.len();
            let mean_coordination = if bonded_entities > 0 {
                2.0 * bonded_pairs as f64 / bonded_entities as f64
            } else {
                0.0
            };
            bo.tick = now;
            bo.bonded_pairs = bonded_pairs;
            bo.bonded_entities = bonded_entities;
            bo.mean_coordination = mean_coordination;
            bo.species_matrix = matrix;
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

    #[test]
    fn lifetime_and_species_matrix_are_measured_by_the_system() {
        use crate::ecs::Resources;
        use crate::rng::Rng;
        use crate::scheduler::SystemContext;
        use crate::stats::{BondObservation, StatsCollector};
        use crate::universe::Time;

        let cfg = Config::default_config();
        let dt = cfg.universe.dt;
        let min = cfg.stats.bond_min_periods;
        let eps =
            mix_epsilon(cfg.physics.thermal_constant, AtomType::Hydrogen, AtomType::Hydrogen);
        let sig = mix_sigma(AtomType::Hydrogen, AtomType::Hydrogen);
        let mu = AtomType::Hydrogen.mass() / 2.0;
        let threshold = (min * vib_period(eps, sig, mu) / dt).ceil() as u64;

        let mut world = small_world(&[
            (Vec3::new(0.0, 0.0, 0.0), AtomType::Hydrogen),
            (Vec3::new(1.0, 0.0, 0.0), AtomType::Hydrogen),
        ]);
        let mut resources = Resources::new();
        resources.insert(BondObservation::default());
        resources.insert(cfg.clone());
        let mut rng = Rng::new(7);
        let mut time = Time::new(dt);
        let mut stats = StatsCollector::new(16);
        let mut sys = BondObservationSystem::new(&cfg);

        let drive = |sys: &mut BondObservationSystem,
                         world: &mut World,
                         resources: &mut Resources,
                         rng: &mut Rng,
                         time: &mut Time,
                         stats: &mut StatsCollector,
                         ticks: u64| {
            for _ in 0..ticks {
                time.advance();
                let mut ctx = SystemContext {
                    world,
                    resources,
                    rng,
                    time,
                    stats,
                    dt: time.dt,
                };
                sys.run(&mut ctx);
            }
        };

        // Bound (H–H at 1.0 < 1.5·1.6 = 2.4) well past the per-pair threshold
        // → a persistent bond appears and stays active long enough to measure
        // a real lifetime.
        drive(&mut sys, &mut world, &mut resources, &mut rng, &mut time, &mut stats, threshold + 500);
        let bo = resources.get::<BondObservation>().unwrap();
        assert_eq!(bo.bonded_pairs, 1, "bound pair must become persistent");
        assert_eq!(bo.bonds_formed, 0, "still active, not formed yet");
        let hh = element_index(AtomType::Hydrogen) * AtomType::COUNT + element_index(AtomType::Hydrogen);
        assert_eq!(bo.species_matrix[hh], 2, "H–H must be counted symmetrically");

        // Separate them: after the debounce closes the episode the bond
        // breaks and its lifetime (time above the threshold) is recorded.
        let ents: Vec<EntityId> = {
            let mut v = Vec::new();
            world.for_each1::<Position>(|e, _| v.push(e));
            v
        };
        world.get_mut::<Position>(ents[0]).unwrap().0 = Vec3::ZERO;
        world.get_mut::<Position>(ents[1]).unwrap().0 = Vec3::new(6.0, 0.0, 0.0);
        drive(&mut sys, &mut world, &mut resources, &mut rng, &mut time, &mut stats, DEBOUNCE + 2);

        let bo = resources.get::<BondObservation>().unwrap();
        assert_eq!(bo.bonded_pairs, 0, "bond must break after separation");
        assert_eq!(bo.bonds_formed, 1, "a broken persistent pair is a formed bond");
        assert!(
            bo.lifetime_sum_ticks >= 400.0,
            "lifetime {} should cover the ~500 active ticks above the threshold",
            bo.lifetime_sum_ticks
        );
    }
}
