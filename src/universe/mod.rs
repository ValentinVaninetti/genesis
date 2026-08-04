//! `Universe`: facade of the whole simulation.
//!
//! It is the only entry point. It holds everything: time, RNG, configuration,
//! `World`, resources, scheduler and statistics. Its API is deliberately small
//! (`new`, `tick`, `run_ticks`, `save`, `load`): everything else is reached
//! through its public fields.

pub mod time;

pub use time::Time;

use crate::components::{
    Acceleration, AtomType, Bonds, Charge, Mass, Position, Velocity,
};
use crate::config::Config;
use crate::ecs::{Resources, World};
use crate::math::Vec3;
use crate::physics::grid::Particle;
use crate::rng::Rng;
use crate::scheduler::{Scheduler, SystemContext};
use crate::serialization::{load_universe, save_universe, LoadError, SaveError, UniverseState};
use crate::stats::{
    BondEnergy, ChemicalStructure, CollisionCounter, PotentialEnergy, StatsCollector, StructureStats,
};
use crate::systems::{
    BondObservationSystem, BondStructureSystem, BoundarySystem, CollisionSystem, ForceSystem,
    MovementSystem, PositionDrift, StatsSystem, StructureSystem, ThermostatSystem,
    VelocityHalfKick,
};
use std::fmt;
use std::path::Path;
use std::time::Instant;

/// The complete universe.
pub struct Universe {
    pub config: Config,
    pub time: Time,
    pub rng: Rng,
    pub world: World,
    pub resources: Resources,
    pub scheduler: Scheduler,
    pub stats: StatsCollector,
    /// Resolved affinity table (built-in defaults + config overrides). Owned
    /// once here so every system and lens uses the same physical parameters.
    pub elements: crate::physics::forces::ElementTable,
    last_tick: Instant,
}

impl Universe {
    /// Builds the affinity table from the config, panicking only on an
    /// unvalidated config (all real paths validate at parse time).
    fn affinity_table(config: &Config) -> crate::physics::forces::ElementTable {
        let mut elements = crate::physics::forces::ElementTable::default_table();
        if let Err(sym) = elements.apply_overrides(&config.physics.elements) {
            panic!("[genesis] invalid affinity table: {sym}");
        }
        elements
    }
    /// Creates a new universe from the configuration and seeds the initial
    /// population of atoms.
    pub fn new(config: Config) -> Self {
        crate::components::register_all();

        let time = Time::new(config.universe.dt);
        let rng = Rng::new(config.rng.seed);
        let stats_cap = config.universe.stats_history;

        let mut resources = Resources::new();
        resources.insert(config.clone());
        resources.insert(CollisionCounter::default());
        resources.insert(PotentialEnergy::default());
        resources.insert(BondEnergy::default());
        resources.insert(StructureStats {
            tick: 0,
            monomers: 0,
            aggregates: 0,
            largest: 0,
            mean_size: 0.0,
            bound_pairs: 0,
        });
        resources.insert(crate::stats::BondObservation::default());
        resources.insert(ChemicalStructure::default());

        let mut scheduler = Scheduler::new();
        build_schedule(&mut scheduler, &config);

        let elements = Self::affinity_table(&config);
        let mut universe = Self {
            config,
            time,
            rng,
            world: World::new(),
            resources,
            scheduler,
            stats: StatsCollector::new(stats_cap),
            elements,
            last_tick: Instant::now(),
        };
        universe.seed_atoms();
        universe
    }

    /// Rebuilds a universe from a saved state (for deserialization).
    pub(crate) fn from_state(config: Config, state: UniverseState) -> Self {
        crate::components::register_all();

        let mut resources = Resources::new();
        resources.insert(config.clone());
        resources.insert(CollisionCounter::default());
        resources.insert(PotentialEnergy::default());
        resources.insert(BondEnergy::default());
        resources.insert(StructureStats {
            tick: 0,
            monomers: 0,
            aggregates: 0,
            largest: 0,
            mean_size: 0.0,
            bound_pairs: 0,
        });
        resources.insert(crate::stats::BondObservation::default());
        resources.insert(ChemicalStructure::default());

        let mut scheduler = Scheduler::new();
        build_schedule(&mut scheduler, &config);

        let elements = Self::affinity_table(&config);
        Self {
            config,
            time: state.time,
            rng: state.rng,
            world: World::new(),
            resources,
            scheduler,
            stats: state.stats,
            elements,
            last_tick: Instant::now(),
        }
    }

    /// Advances one tick: advances the clock and runs the full schedule.
    pub fn tick(&mut self) {
        self.time.advance();
        let start = Instant::now();
        let dt = self.time.dt;
        {
            let mut ctx = SystemContext {
                world: &mut self.world,
                resources: &mut self.resources,
                rng: &mut self.rng,
                time: &self.time,
                stats: &mut self.stats,
                dt,
            };
            self.scheduler.run(&mut ctx);
        }
        let elapsed = start.elapsed().as_secs_f64();
        self.stats.fps = if elapsed > 0.0 { 1.0 / elapsed } else { 0.0 };
        self.last_tick = start;
    }

    /// Runs `n` consecutive ticks.
    pub fn run_ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Wall-clock time elapsed since the last tick.
    pub fn last_tick_elapsed(&self) -> std::time::Duration {
        self.last_tick.elapsed()
    }

    /// Saves the complete universe to a file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SaveError> {
        save_universe(self, path)
    }

    /// Loads a saved universe.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        load_universe(path)
    }

    /// Seeds the initial population of atoms according to the configuration.
    ///
    /// This is **not** a law of the universe: it is the "big bang" of the
    /// simulation, the only moment in which matter is created from the
    /// configuration. Velocities are sampled from a Maxwell-Boltzmann
    /// distribution (each component ~ Normal(0, √(k·T/m))): temperature is a
    /// seeding input, not a state of the universe.
    ///
    /// With forces enabled, seeding uses a **cubic lattice** with thermal
    /// jitter (the standard initialization of molecular dynamics): a fully
    /// random seeding would overlap nuclei and the r⁻¹² repulsion of
    /// Lennard-Jones would turn them into a numerical explosion.
    fn seed_atoms(&mut self) {
        let count = self.config.universe.initial_atoms;
        let temp = self.config.physics.initial_temperature;
        let k = self.config.physics.thermal_constant;
        let mut elements = self.config.universe.elements.clone();
        if elements.is_empty() {
            elements = AtomType::ALL.to_vec();
        }
        if self.config.systems.enable_forces {
            self.seed_lattice(count, temp, k, &elements);
        } else {
            self.seed_random(count, temp, k, &elements);
        }
    }

    /// Uniform random seeding (only without forces: without short-range
    /// repulsion there are no problematic overlaps).
    fn seed_random(&mut self, count: usize, temp: f64, k: f64, elements: &[AtomType]) {
        let half = self.config.universe.size.scale(0.5);
        for _ in 0..count {
            self.spawn_atom(elements, temp, k, |rng| rng.in_box(half));
        }
    }

    /// Seeding on a cubic lattice with thermal jitter. The count is rounded to
    /// the nearest perfect cube below the requested value (`n³ ≤ count`).
    fn seed_lattice(&mut self, count: usize, temp: f64, k: f64, elements: &[AtomType]) {
        let size = self.config.universe.size;
        let n = (count as f64).powf(1.0 / 3.0).floor().max(1.0) as u32;
        let cell = Vec3::new(
            size.x / n as f64,
            size.y / n as f64,
            size.z / n as f64,
        );
        let half = size.scale(0.5);
        let jitter = cell.scale(0.1);
        for ix in 0..n {
            for iy in 0..n {
                for iz in 0..n {
                    let base = Vec3::new(
                        (ix as f64 + 0.5) * cell.x - half.x,
                        (iy as f64 + 0.5) * cell.y - half.y,
                        (iz as f64 + 0.5) * cell.z - half.z,
                    );
                    self.spawn_atom(elements, temp, k, |rng| base + rng.in_box(jitter));
                }
            }
        }
    }

    /// Creates an atom with position, element, mass, charge, thermal velocity
    /// and zero acceleration.
    fn spawn_atom(
        &mut self,
        elements: &[AtomType],
        temp: f64,
        k: f64,
        pos: impl FnOnce(&mut Rng) -> Vec3,
    ) {
        let e = self.world.spawn();
        let pos = pos(&mut self.rng);
        self.world.insert::<Position>(e, Position(pos));

        let at = elements[self.rng.int(0, (elements.len() - 1) as u64) as usize];
        self.world.insert::<AtomType>(e, at);
        self.world.insert::<Mass>(e, Mass(at.mass()));
        self.world
            .insert::<Charge>(e, Charge(self.elements.charge(at)));
        self.world.insert::<Bonds>(e, Bonds::default());

        let sigma = (k * temp / at.mass()).sqrt();
        let vel = Velocity(Vec3::new(
            self.rng.gaussian() * sigma,
            self.rng.gaussian() * sigma,
            self.rng.gaussian() * sigma,
        ));
        self.world.insert::<Velocity>(e, vel);
        self.world.insert::<Acceleration>(e, Acceleration(Vec3::ZERO));
    }

    /// Analysis observables: compact particles + atomic types.
    fn observable_particles(&self) -> (Vec<Particle>, Vec<AtomType>) {
        let mut particles = Vec::with_capacity(self.world.len());
        let mut types = Vec::with_capacity(self.world.len());
        self.world.for_each2::<Position, AtomType>(|e, pos, at| {
            particles.push(Particle {
                index: e.index(),
                pos: pos.0,
                vel: Vec3::ZERO,
                mass: 0.0,
            });
            types.push(*at);
        });
        (particles, types)
    }

    /// `g(r)` of the current state (a lens, not a law). `r_max` is clipped to
    /// half of the shortest side of the torus.
    pub fn radial_distribution(&self, r_max: f64, bins: usize) -> crate::analysis::RadialDistribution {
        let (particles, _) = self.observable_particles();
        crate::analysis::radial_distribution(&particles, self.config.universe.size, r_max, bins)
    }

    /// Partial `g_ab(r)` between two species (a lens, not a law).
    pub fn radial_distribution_between(
        &self,
        ta: AtomType,
        tb: AtomType,
        r_max: f64,
        bins: usize,
    ) -> crate::analysis::RadialDistribution {
        let (particles, types) = self.observable_particles();
        crate::analysis::radial_distribution_between(
            &particles,
            &types,
            ta,
            tb,
            self.config.universe.size,
            r_max,
            bins,
        )
    }

    /// Emergent aggregates of the current state (friends-of-friends).
    pub fn cluster_analysis(&self) -> crate::analysis::ClusterStats {
        let (particles, types) = self.observable_particles();
        crate::analysis::clusters(
            &particles,
            &types,
            self.config.universe.size,
            crate::analysis::BOND_FACTOR,
            &self.elements,
        )
    }

    /// One-line summary with the most recent metrics.
    pub fn status_line(&self) -> String {
        let s = &self.stats.snapshot;
        format!(
            "tick={} t={:.3}s entities={} E={:.3} (K={:.3} V={:.3}) E_avg={:.3} T_avg={:.1} collisions={} fps={:.1} mem={}kB",
            s.tick,
            s.time,
            s.entities,
            s.energy_total,
            s.energy_total - s.energy_potential,
            s.energy_potential,
            s.energy_avg,
            s.temperature_avg,
            s.collisions,
            s.fps,
            s.memory_bytes / 1024,
        )
    }
}

/// Builds the schedule according to the configuration, in registration order.
///
/// With forces enabled it uses **velocity Verlet** (kick–drift–force–kick);
/// without them it keeps the classic Euler integration for movement and
/// collisions.
fn build_schedule(scheduler: &mut Scheduler, cfg: &Config) {
    if cfg.systems.enable_forces {
        scheduler.add_system(VelocityHalfKick);
        scheduler.add_system(PositionDrift);
        if cfg.systems.enable_boundaries {
            scheduler.add_system(BoundarySystem);
        }
        scheduler.add_system(ForceSystem::new(cfg));
        if cfg.systems.enable_collisions {
            scheduler.add_system(CollisionSystem::new(cfg));
        }
        scheduler.add_system(VelocityHalfKick);
        if cfg.systems.enable_thermostat {
            scheduler.add_system(ThermostatSystem::new(cfg));
        }
    } else {
        if cfg.systems.enable_movement {
            scheduler.add_system(MovementSystem);
        }
        if cfg.systems.enable_boundaries {
            scheduler.add_system(BoundarySystem);
        }
        if cfg.systems.enable_collisions {
            scheduler.add_system(CollisionSystem::new(cfg));
        }
        if cfg.systems.enable_thermostat {
            scheduler.add_system(ThermostatSystem::new(cfg));
        }
    }
    if cfg.systems.enable_forces {
        if cfg.systems.enable_bond_observation {
            scheduler.add_system(BondObservationSystem::new(cfg));
            scheduler.add_system(BondStructureSystem::new(cfg.stats.structure_interval));
        }
        scheduler.add_system(StructureSystem::new(cfg.stats.structure_interval));
    }
    scheduler.add_system(StatsSystem);
}

impl fmt::Display for Universe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let vol = self.config.universe.size.x * self.config.universe.size.y * self.config.universe.size.z;
        writeln!(f, "┌─ GENESIS ─────────────────────────────────────────────")?;
        writeln!(f, "│ universe       : {}", self.config.universe.name)?;
        writeln!(
            f,
            "│ size           : {} (vol {:.0} u³)",
            self.config.universe.size, vol
        )?;
        writeln!(f, "│ dt             : {:.4}s ({} ticks/s)", self.time.dt, 1.0 / self.time.dt)?;
        writeln!(f, "│ RNG seed       : {}", self.config.rng.seed)?;
        writeln!(
            f,
            "│ population     : {} atoms (requested: {})",
            self.world.len(),
            self.config.universe.initial_atoms
        )?;
        writeln!(f, "│ ECS archetypes : {}", self.world.archetype_count())?;
        writeln!(f, "│ systems        : {}", self.scheduler.systems().len())?;
        writeln!(f, "└──────────────────────────────────────────────────────")?;
        Ok(())
    }
}
