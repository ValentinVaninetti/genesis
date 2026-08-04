//! Persistent-pair tracking: observation of whether "bonds" survive.
//!
//! A `PairTracker` watches candidate pairs across ticks: a pair that stays
//! within the binding threshold for a contiguous stretch is an *episode*.
//! Episodes are the raw material to decide whether binding *emerges* (a pair
//! that survives many vibrational periods) — without ever programming a bond.
//!
//! The threshold is **per pair**: `r < k_bind · σ_ij`, with `σ_ij` the
//! Lorentz–Berthelot mixing of the two elements (same rule as the LJ table).
//! A single absolute threshold would systematically under-estimate the large
//! elements (Fe, Si) and over-estimate the small ones (H, He).

use crate::components::{AtomType, Position};
use crate::ecs::{EntityId, World};
use crate::math::Vec3;
use crate::physics::forces::ElementTable;
use crate::physics::grid::{min_image, Particle, SpatialGrid};
use std::collections::{HashMap, HashSet};

/// Default binding threshold, as a multiple of the pair's mixed σ.
pub const DEFAULT_K_BIND: f64 = 1.5;

/// A candidate bound pair, ids normalized (`a < b`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundPair {
    pub a: EntityId,
    pub b: EntityId,
}

/// A contiguous stretch in which a pair stayed within the threshold.
#[derive(Debug, Clone, Copy)]
pub struct Episode {
    pub pair: BoundPair,
    /// Consecutive ticks bound.
    pub ticks: u64,
}

/// Tracks pair episodes with an optional debounce: a pair is only considered
/// broken after `debounce` consecutive ticks out of the threshold. This
/// avoids counting thermal flicker at the edge of the threshold
/// (`r ≈ k·σ_ij`) as repeated break/rebind of the same event. `debounce = 1`
/// means no debounce (break on the first absent tick).
pub struct PairTracker {
    /// Consecutive out-of-threshold ticks required to close an episode.
    pub debounce: u64,
    open: HashMap<BoundPair, u64>,
    missing: HashMap<BoundPair, u64>,
    completed: Vec<Episode>,
}

impl PairTracker {
    pub fn new(debounce: u64) -> Self {
        Self {
            debounce: debounce.max(1),
            open: HashMap::new(),
            missing: HashMap::new(),
            completed: Vec::new(),
        }
    }

    /// Feeds the bound pairs of one tick.
    pub fn track_tick(&mut self, bound: &[BoundPair]) {
        let current: HashSet<BoundPair> = bound.iter().copied().collect();

        for &pair in &current {
            self.missing.remove(&pair);
            self.open.entry(pair).and_modify(|t| *t += 1).or_insert(1);
        }

        let absent: Vec<BoundPair> = self
            .open
            .keys()
            .copied()
            .filter(|p| !current.contains(p))
            .collect();
        for pair in absent {
            let missing = self.missing.entry(pair).or_insert(0);
            *missing += 1;
            if *missing >= self.debounce {
                let ticks = self.open.remove(&pair).unwrap();
                self.missing.remove(&pair);
                self.completed.push(Episode { pair, ticks });
            }
        }
    }

    /// Completes the open episodes (call at the end of a run).
    pub fn close_all(&mut self) {
        for (pair, ticks) in self.open.drain() {
            self.completed.push(Episode { pair, ticks });
        }
        self.missing.clear();
    }

    /// Episodes already closed (their stretch ended).
    pub fn completed(&self) -> &[Episode] {
        &self.completed
    }

    /// Pairs bound at the current tick (open, uninterrupted).
    pub fn open_count(&self) -> usize {
        self.open.len()
    }

    /// Open episodes and their consecutive bound ticks so far.
    pub fn open_pairs(&self) -> impl Iterator<Item = (BoundPair, u64)> + '_ {
        self.open.iter().map(|(&p, &t)| (p, t))
    }
}

/// The binding cutoff of `k_bind` σ_ij, in simulation units.
pub fn bind_cutoff(elements: &ElementTable, k_bind: f64) -> f64 {
    k_bind * AtomType::ALL.iter().map(|&t| elements.sigma(t)).fold(0.0, f64::max)
}

/// Candidate bound pairs of the current state: `r < k_bind · σ_ij`, with the
/// mixed `σ_ij` per pair and the spatial grid as broad-phase.
pub fn collect_bound_pairs(
    world: &World,
    world_size: Vec3,
    k_bind: f64,
    elements: &ElementTable,
) -> Vec<BoundPair> {
    let cutoff = bind_cutoff(elements, k_bind);
    let mut grid = SpatialGrid::new(world_size, cutoff);
    bound_pairs_with_grid(world, world_size, k_bind, &mut grid, elements)
}

/// Same as [`collect_bound_pairs`] but reusing the caller's grid and buffers
/// (for systems that run every tick). The grid must have been built with a
/// cell size ≥ the binding cutoff.
pub fn bound_pairs_with_grid(
    world: &World,
    world_size: Vec3,
    k_bind: f64,
    grid: &mut SpatialGrid,
    elements: &ElementTable,
) -> Vec<BoundPair> {
    let cutoff = bind_cutoff(elements, k_bind);

    let mut particles: Vec<Particle> = Vec::with_capacity(world.len());
    let mut types: Vec<AtomType> = Vec::with_capacity(world.len());
    let mut ids: HashMap<u32, EntityId> = HashMap::with_capacity(world.len());
    world.for_each2::<Position, AtomType>(|e, pos, at| {
        ids.insert(e.index(), e);
        particles.push(Particle {
            index: e.index(),
            pos: pos.0,
            vel: Vec3::ZERO,
            mass: 0.0,
        });
        types.push(*at);
    });

    grid.build(&particles);
    let mut candidates = Vec::new();
    grid.neighbors(&particles, cutoff, &mut candidates);

    let mut out = Vec::new();
    for pair in candidates {
        let (pa, pb) = (&particles[pair.a], &particles[pair.b]);
        let d = min_image(pa.pos - pb.pos, world_size).length();
        let threshold = k_bind * elements.mix_sigma(types[pair.a], types[pair.b]);
        if d < threshold {
            let mut a = ids[&pa.index];
            let mut b = ids[&pb.index];
            if b < a {
                std::mem::swap(&mut a, &mut b);
            }
            out.push(BoundPair { a, b });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Acceleration, Mass, Velocity};

    fn world_with(positions: &[(Vec3, AtomType)]) -> World {
        crate::components::register_all();
        let mut w = World::new();
        for &(pos, at) in positions {
            let e = w.spawn();
            w.insert::<Position>(e, Position(pos));
            w.insert::<AtomType>(e, at);
            w.insert::<Mass>(e, Mass(at.mass()));
            w.insert::<Velocity>(e, Velocity(Vec3::ZERO));
            w.insert::<Acceleration>(e, Acceleration(Vec3::ZERO));
        }
        w
    }

    #[test]
    fn collect_respects_per_pair_threshold() {
        let size = Vec3::new(64.0, 64.0, 64.0);
        // H–H mixed σ = 1.6; at 2.0 < 1.5·1.6 = 2.4 the pair is bound.
        let w = world_with(&[
            (Vec3::new(0.0, 0.0, 0.0), AtomType::Hydrogen),
            (Vec3::new(2.0, 0.0, 0.0), AtomType::Hydrogen),
        ]);
        assert_eq!(
            collect_bound_pairs(&w, size, DEFAULT_K_BIND, &ElementTable::default_table()).len(),
            1
        );

        // The same absolute 3.3 distance is bound for Si–Si (σ = 2.3,
        // threshold 3.45) but NOT for H–H (threshold 2.4): the threshold
        // scales with the mixed σ of each pair.
        let si = world_with(&[
            (Vec3::new(0.0, 0.0, 0.0), AtomType::Silicon),
            (Vec3::new(3.3, 0.0, 0.0), AtomType::Silicon),
        ]);
        assert_eq!(
            collect_bound_pairs(&si, size, DEFAULT_K_BIND, &ElementTable::default_table()).len(),
            1
        );
        let h = world_with(&[
            (Vec3::new(0.0, 0.0, 0.0), AtomType::Hydrogen),
            (Vec3::new(3.3, 0.0, 0.0), AtomType::Hydrogen),
        ]);
        assert_eq!(
            collect_bound_pairs(&h, size, DEFAULT_K_BIND, &ElementTable::default_table()).len(),
            0
        );
    }

    #[test]
    fn tracker_counts_episodes_and_debounce() {
        let mut t = PairTracker::new(1);
        let p = BoundPair { a: EntityId::new(1, 0), b: EntityId::new(2, 0) };
        // bound for 5 ticks
        for _ in 0..5 {
            t.track_tick(&[p]);
        }
        // absent for 2 ticks → with debounce 1 it closes after the first.
        t.track_tick(&[]);
        assert_eq!(t.completed().len(), 1);
        assert_eq!(t.completed()[0].ticks, 5);
        assert_eq!(t.open_count(), 0);

        // rebind for 3 more ticks
        for _ in 0..3 {
            t.track_tick(&[p]);
        }
        t.close_all();
        assert_eq!(t.completed().len(), 2);
        assert_eq!(t.completed()[1].ticks, 3);

        // With debounce = 3 a one-tick gap does NOT close the episode.
        let mut t2 = PairTracker::new(3);
        for _ in 0..5 {
            t2.track_tick(&[p]);
        }
        t2.track_tick(&[]); // 1 absent tick < 3 → still open
        assert_eq!(t2.completed().len(), 0);
        assert_eq!(t2.open_count(), 1);
        t2.track_tick(&[]);
        t2.track_tick(&[]); // now 3 consecutive absent → closed
        assert_eq!(t2.completed().len(), 1);
        assert_eq!(t2.completed()[0].ticks, 5);
    }
}
