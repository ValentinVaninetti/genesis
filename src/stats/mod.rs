//! Simulation statistics.
//!
//! A `StatsCollector` aggregates a `StatsSnapshot` every tick and keeps a
//! bounded history. Sampling is done by a system (the last one in the
//! schedule), so metric collection is part of the universe and not a side
//! effect of the main loop.

use serde::{Deserialize, Serialize};

/// Emergent structure measured by the analysis system (friends-of-friends
/// aggregates). It is observation, not physical state: it describes what the
/// laws produce, without feeding the simulation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StructureStats {
    /// Tick in which it was measured.
    pub tick: u64,
    /// Clusters of a single atom.
    pub monomers: usize,
    /// Clusters of ≥ 2 atoms.
    pub aggregates: usize,
    /// Size of the largest aggregate.
    pub largest: usize,
    /// Mean size over all clusters.
    pub mean_size: f64,
    /// Atom pairs in contact.
    pub bound_pairs: usize,
}

/// A snapshot of metrics at an instant of simulation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsSnapshot {
    pub tick: u64,
    pub time: f64,
    pub entities: usize,
    /// Total kinetic energy (derived from `Velocity` and `Mass`).
    pub energy_total: f64,
    pub energy_avg: f64,
    /// Total potential energy of the tick (Lennard-Jones, accumulated by forces).
    pub energy_potential: f64,
    /// Temperature derived by equipartition: `(2/3)·⟨K⟩/k`.
    pub temperature_avg: f64,
    /// Mean speed of the particles.
    pub mean_speed: f64,
    pub density: f64,
    pub collisions: u64,
    pub systems_run: u64,
    pub fps: f64,
    pub memory_bytes: usize,
    /// Structure summary of the last analysis sampling (None if there are no
    /// forces or it has not been sampled yet).
    pub structure: Option<StructureStats>,
    /// Persistent-bond pairs of the last tick (0 when the bond observation is
    /// off). A *different* lens than `structure.bound_pairs`: it requires the
    /// episode to have survived `bond_min_periods` vibrational periods.
    #[serde(default)]
    pub bonded_pairs: usize,
}

/// Metrics collector with history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsCollector {
    pub snapshot: StatsSnapshot,
    /// Total systems executed since the start.
    pub systems_run: u64,
    /// Real FPS measured by the main loop (not simulation time).
    pub fps: f64,
    history: Vec<StatsSnapshot>,
    history_cap: usize,
}

impl StatsCollector {
    pub fn new(history_cap: usize) -> Self {
        Self {
            snapshot: StatsSnapshot {
                tick: 0,
                time: 0.0,
                entities: 0,
                energy_total: 0.0,
                energy_avg: 0.0,
                energy_potential: 0.0,
                temperature_avg: 0.0,
                mean_speed: 0.0,
                density: 0.0,
                collisions: 0,
                systems_run: 0,
                fps: 0.0,
                memory_bytes: 0,
                structure: None,
                bonded_pairs: 0,
            },
            systems_run: 0,
            fps: 0.0,
            history: Vec::with_capacity(history_cap),
            history_cap,
        }
    }

    /// Records a new metric in the history.
    pub fn record(&mut self, snapshot: StatsSnapshot) {
        if self.history.len() == self.history_cap {
            self.history.remove(0);
        }
        self.snapshot = snapshot.clone();
        self.history.push(snapshot);
    }

    /// Latest recorded metric.
    pub fn snapshot(&self) -> &StatsSnapshot {
        &self.snapshot
    }

    /// Historical metrics (bounded).
    pub fn history(&self) -> &[StatsSnapshot] {
        &self.history
    }
}

/// Global resource: collision counter.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CollisionCounter(pub u64);

/// Global resource: total potential energy of the current tick
/// (Lennard-Jones), accumulated by the force system. Not persistent state: it
/// is fully recomputed every tick.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PotentialEnergy(pub f64);

/// Global resource: persistent-bond observation of the last tick, filled by
/// `BondObservationSystem` (only when enabled). Observation, not a law.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct BondObservation {
    /// Tick in which it was measured.
    pub tick: u64,
    /// Pairs whose episode has survived the persistence threshold.
    pub bonded_pairs: usize,
    /// Entities with at least one persistent bond.
    pub bonded_entities: usize,
    /// Mean coordination (bonds per bonded entity).
    pub mean_coordination: f64,
}

/// Speed (`|v|`) histogram observed at an instant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocityHistogram {
    /// Upper bound of the range.
    pub max_speed: f64,
    /// Width of each bin.
    pub bin_width: f64,
    /// Counts per bin.
    pub bins: Vec<u64>,
    /// Total samples.
    pub samples: u64,
    /// Samples out of range (`≥ max_speed`).
    pub overflow: u64,
}

/// Builds a speed histogram from the `World`.
pub fn velocity_histogram(
    world: &crate::ecs::World,
    max_speed: f64,
    bins: usize,
) -> VelocityHistogram {
    let bins = bins.clamp(1, 512);
    let max_speed = max_speed.max(1e-9);
    let bin_width = max_speed / bins as f64;
    let mut counts = vec![0u64; bins];
    let mut samples = 0u64;
    let mut overflow = 0u64;
    world.for_each1::<crate::components::Velocity>(|_, v| {
        let s = v.0.length();
        samples += 1;
        if s >= max_speed {
            overflow += 1;
        } else {
            let i = (s / bin_width) as usize;
            counts[i.min(bins - 1)] += 1;
        }
    });
    VelocityHistogram {
        max_speed,
        bin_width,
        bins: counts,
        samples,
        overflow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded() {
        let mut c = StatsCollector::new(3);
        for i in 0..10 {
            let mut s = c.snapshot.clone();
            s.tick = i;
            c.record(s);
        }
        assert_eq!(c.history().len(), 3);
        assert_eq!(c.snapshot.tick, 9);
    }
}
