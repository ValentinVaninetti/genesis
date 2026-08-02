//! `Universe`: fachada de toda la simulación.
//!
//! Es el único punto de entrada. Contiene todo: tiempo, RNG, configuración,
//! `World`, recursos, scheduler y estadísticas. Su API es deliberadamente
//! pequeña (`new`, `tick`, `run_ticks`, `save`, `load`): todo lo demás se
//! alcanza a través de sus campos públicos.

pub mod time;

pub use time::Time;

use crate::components::{Acceleration, AtomType, Charge, Mass, Position, Velocity};
use crate::config::Config;
use crate::ecs::{Resources, World};
use crate::math::Vec3;
use crate::rng::Rng;
use crate::scheduler::{Scheduler, SystemContext};
use crate::serialization::{load_universe, save_universe, LoadError, SaveError, UniverseState};
use crate::stats::{CollisionCounter, PotentialEnergy, StatsCollector};
use crate::systems::{
    BoundarySystem, CollisionSystem, ForceSystem, MovementSystem, PositionDrift, StatsSystem,
    VelocityHalfKick,
};
use std::fmt;
use std::path::Path;
use std::time::Instant;

/// El universo completo.
pub struct Universe {
    pub config: Config,
    pub time: Time,
    pub rng: Rng,
    pub world: World,
    pub resources: Resources,
    pub scheduler: Scheduler,
    pub stats: StatsCollector,
    last_tick: Instant,
}

impl Universe {
    /// Crea un universo nuevo a partir de la configuración y siembra la
    /// población inicial de átomos.
    pub fn new(config: Config) -> Self {
        crate::components::register_all();

        let time = Time::new(config.universe.dt);
        let rng = Rng::new(config.rng.seed);
        let stats_cap = config.universe.stats_history;

        let mut resources = Resources::new();
        resources.insert(config.clone());
        resources.insert(CollisionCounter::default());
        resources.insert(PotentialEnergy::default());

        let mut scheduler = Scheduler::new();
        build_schedule(&mut scheduler, &config);

        let mut universe = Self {
            config,
            time,
            rng,
            world: World::new(),
            resources,
            scheduler,
            stats: StatsCollector::new(stats_cap),
            last_tick: Instant::now(),
        };
        universe.seed_atoms();
        universe
    }

    /// Reconstruye un universo desde un estado guardado (para deserialización).
    pub(crate) fn from_state(config: Config, state: UniverseState) -> Self {
        crate::components::register_all();

        let mut resources = Resources::new();
        resources.insert(config.clone());
        resources.insert(CollisionCounter::default());
        resources.insert(PotentialEnergy::default());

        let mut scheduler = Scheduler::new();
        build_schedule(&mut scheduler, &config);

        Self {
            config,
            time: state.time,
            rng: state.rng,
            world: World::new(),
            resources,
            scheduler,
            stats: state.stats,
            last_tick: Instant::now(),
        }
    }

    /// Avanza un tick: avanza el reloj y ejecuta el schedule completo.
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

    /// Ejecuta `n` ticks seguidos.
    pub fn run_ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Tiempo de pared transcurrido desde el último tick.
    pub fn last_tick_elapsed(&self) -> std::time::Duration {
        self.last_tick.elapsed()
    }

    /// Guarda el universo completo en un archivo.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SaveError> {
        save_universe(self, path)
    }

    /// Carga un universo guardado.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        load_universe(path)
    }

    /// Siembra la población inicial de átomos según la configuración.
    ///
    /// Esto **no** es una ley del universo: es el "big bang" de la simulación,
    /// el único momento en que se crea materia desde la configuración. Las
    /// velocidades se muestrean de una distribución de Maxwell-Boltzmann
    /// (cada componente ~ Normal(0, √(k·T/m))): la temperatura es un dato de
    /// entrada del seeding, no un estado del universo.
    ///
    /// Con fuerzas activas se siembra en una **red cúbica** con jitter
    /// térmico (la inicialización estándar de dinámica molecular): un sembrado
    /// totalmente aleatorio superpondría núcleos y la repulsión r⁻¹² de
    /// Lennard-Jones los convertiría en una explosión numérica.
    fn seed_atoms(&mut self) {
        let count = self.config.universe.initial_atoms;
        let temp = self.config.physics.initial_temperature;
        let k = self.config.physics.thermal_constant;
        let elements = [
            AtomType::Hydrogen,
            AtomType::Helium,
            AtomType::Carbon,
            AtomType::Nitrogen,
            AtomType::Oxygen,
        ];
        if self.config.systems.enable_forces {
            self.seed_lattice(count, temp, k, &elements);
        } else {
            self.seed_random(count, temp, k, &elements);
        }
    }

    /// Sembrado aleatorio uniforme (solo sin fuerzas: sin repulsión de corto
    /// alcance no hay superposiciones problemáticas).
    fn seed_random(&mut self, count: usize, temp: f64, k: f64, elements: &[AtomType]) {
        let half = self.config.universe.size.scale(0.5);
        for _ in 0..count {
            self.spawn_atom(elements, temp, k, |rng| rng.in_box(half));
        }
    }

    /// Sembrado en red cúbica con jitter térmico. El conteo se redondea al
    /// cubo perfecto más cercano por debajo del pedido (`n³ ≤ count`).
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

    /// Crea un átomo con posición, elemento, masa, carga, velocidad térmica
    /// y aceleración nula.
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
        self.world.insert::<Charge>(e, Charge(0.0));

        let sigma = (k * temp / at.mass()).sqrt();
        let vel = Velocity(Vec3::new(
            self.rng.gaussian() * sigma,
            self.rng.gaussian() * sigma,
            self.rng.gaussian() * sigma,
        ));
        self.world.insert::<Velocity>(e, vel);
        self.world.insert::<Acceleration>(e, Acceleration(Vec3::ZERO));
    }

    /// Resumen de una línea con las métricas más recientes.
    pub fn status_line(&self) -> String {
        let s = &self.stats.snapshot;
        format!(
            "tick={} t={:.3}s entidades={} E={:.3} (K={:.3} V={:.3}) E_avg={:.3} T_avg={:.1} colisiones={} fps={:.1} mem={}kB",
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

/// Construye el schedule según la configuración, en orden de registro.
///
/// Con fuerzas activas usa **velocity Verlet** (kick–drift–force–kick); sin
/// ellas conserva la integración Euler clásica para movimiento y colisiones.
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
    }
    scheduler.add_system(StatsSystem);
}

impl fmt::Display for Universe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let vol = self.config.universe.size.x * self.config.universe.size.y * self.config.universe.size.z;
        writeln!(f, "┌─ GENESIS ─────────────────────────────────────────────")?;
        writeln!(f, "│ universo       : {}", self.config.universe.name)?;
        writeln!(
            f,
            "│ tamaño         : {} (vol {:.0} u³)",
            self.config.universe.size, vol
        )?;
        writeln!(f, "│ dt             : {:.4}s ({} ticks/s)", self.time.dt, 1.0 / self.time.dt)?;
        writeln!(f, "│ semilla RNG    : {}", self.config.rng.seed)?;
        writeln!(
            f,
            "│ población      : {} átomos (pedido: {})",
            self.world.len(),
            self.config.universe.initial_atoms
        )?;
        writeln!(f, "│ arquetipos ECS : {}", self.world.archetype_count())?;
        writeln!(f, "│ sistemas       : {}", self.scheduler.systems().len())?;
        writeln!(f, "└──────────────────────────────────────────────────────")?;
        Ok(())
    }
}
