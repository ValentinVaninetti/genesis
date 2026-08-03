//! Configuration of the universe.
//!
//! All the physical and startup parameters live **outside the code**, in
//! TOML. The structure is 100% typed and derived from `serde`, so an invalid
//! file fails at startup with a clear error, never at runtime with invented
//! values.

use crate::math::Vec3;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Root configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    pub universe: UniverseConfig,
    pub rng: RngConfig,
    pub physics: PhysicsConfig,
    pub systems: SystemsConfig,
    #[serde(default)]
    pub stats: StatsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UniverseConfig {
    /// Symbolic name of the simulation.
    pub name: String,
    /// Size (total extent) of the universe on each axis.
    pub size: Vec3,
    /// Time delta per tick.
    pub dt: f64,
    /// Atoms seeded at startup.
    pub initial_atoms: usize,
    /// Elements seeded at startup; empty means all of them. A starting point,
    /// not a law: the universe does not know about species, only the seeding
    /// uses this table.
    #[serde(default = "default_elements")]
    pub elements: Vec<crate::components::AtomType>,
    /// Maximum capacity of the metrics history.
    pub stats_history: usize,
}

/// Elements seeded by default (all of them).
fn default_elements() -> Vec<crate::components::AtomType> {
    crate::components::AtomType::ALL.to_vec()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RngConfig {
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PhysicsConfig {
    /// Collision radius of the particles.
    #[serde(default = "default_particle_radius")]
    pub particle_radius: f64,
    /// Uniform initial temperature of the atoms.
    pub initial_temperature: f64,
    /// Speed limit (prevents numeric escapes).
    pub speed_limit: f64,
    /// Effective thermal constant of the universe (equipartition: T = 2/3·⟨K⟩/k).
    #[serde(default = "default_thermal_constant")]
    pub thermal_constant: f64,
    /// Reference gravitational acceleration (future law).
    pub gravity_constant: f64,
    /// Target temperature of the thermostat (kelvin).
    #[serde(default = "default_thermostat_temperature")]
    pub thermostat_temperature: f64,
    /// Thermostat relaxation time (in ticks).
    #[serde(default = "default_thermostat_tau")]
    pub thermostat_tau: f64,
}

const fn default_particle_radius() -> f64 {
    0.4
}

const fn default_thermal_constant() -> f64 {
    0.01
}

const fn default_thermostat_temperature() -> f64 {
    300.0
}

const fn default_thermostat_tau() -> f64 {
    20.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct SystemsConfig {
    /// Enables the movement system (Euler integration of position).
    pub enable_movement: bool,
    /// Enables the boundary system (periodic wrapping).
    pub enable_boundaries: bool,
    /// Enables elastic collisions (hard spheres).
    pub enable_collisions: bool,
    /// Enables intermolecular forces (Lennard-Jones) and velocity Verlet
    /// integration. When active, `enable_movement` is ignored.
    pub enable_forces: bool,
    /// Enables the Berendsen thermostat (velocity rescaling): drives the
    /// equipartition temperature toward `physics.thermostat_temperature`.
    /// It is an **instrument**, not a law (opt-in NVT).
    pub enable_thermostat: bool,
}

impl Default for SystemsConfig {
    fn default() -> Self {
        Self {
            enable_movement: true,
            enable_boundaries: true,
            enable_collisions: false,
            enable_forces: true,
            enable_thermostat: false,
        }
    }
}

/// Observation configuration (not physics: only how things are measured and
/// reported).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct StatsConfig {
    /// Number of bins of the velocity histogram.
    pub histogram_bins: usize,
    /// Maximum speed of the histogram (range 0..max).
    pub histogram_max_speed: f64,
    /// Every how many ticks the structure (aggregates) is sampled.
    pub structure_interval: u64,
    /// Path of the metrics CSV. Empty disables the export; a row is appended
    /// every `csv_interval` ticks (the file is created with the header).
    pub csv_path: String,
    /// Every how many ticks a metrics row is appended to `csv_path`.
    pub csv_interval: u64,
    /// Prefix of the position frames (XYZ). Empty disables the dump; each
    /// sampled tick writes `{prefix}_{tick:08}.xyz`.
    pub xyz_prefix: String,
    /// Every how many ticks a position frame is dumped.
    pub xyz_interval: u64,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            histogram_bins: 32,
            histogram_max_speed: 10.0,
            structure_interval: 100,
            csv_path: String::new(),
            csv_interval: 10,
            xyz_prefix: String::new(),
            xyz_interval: 100,
        }
    }
}

impl Config {
    /// Default configuration (equivalent to the embedded TOML).
    pub fn default_config() -> Self {
        Self {
            universe: UniverseConfig {
                name: "Genesis".into(),
                size: Vec3::new(128.0, 128.0, 128.0),
                dt: 1.0 / 60.0,
                initial_atoms: 10_000,
                elements: default_elements(),
                stats_history: 1024,
            },
            rng: RngConfig { seed: 42 },
            physics: PhysicsConfig {
                particle_radius: 0.4,
                initial_temperature: 300.0,
                speed_limit: 1_000.0,
                thermal_constant: 0.01,
                gravity_constant: 6.674e-11,
                thermostat_temperature: 300.0,
                thermostat_tau: 20.0,
            },
            systems: SystemsConfig::default(),
            stats: StatsConfig::default(),
        }
    }

    /// Loads the configuration from a TOML file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path.as_ref())?;
        Self::from_toml(&text)
    }

    /// Parses configuration from TOML text.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(text)?)
    }

    /// Tries to load the file; if it does not exist, uses the default and
    /// persists it.
    pub fn from_file_or_default(path: impl AsRef<Path>) -> Self {
        match Self::from_file(path.as_ref()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "[genesis] configuration not loaded ({}): using default values",
                    e
                );
                let c = Self::default_config();
                let _ = std::fs::create_dir_all(
                    path.as_ref().parent().unwrap_or(Path::new(".")),
                );
                let text = toml::to_string_pretty(&c).expect("config serializable");
                let _ = std::fs::write(path.as_ref(), text);
                c
            }
        }
    }
}

/// Configuration loading errors.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        Self::Parse(e)
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "I/O error: {e}"),
            ConfigError::Parse(e) => write!(f, "invalid TOML: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Writes an example configuration file.
pub fn write_example(path: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::write(path, EXAMPLE_TOML)
}

pub const EXAMPLE_TOML: &str = r#"# =============================================================
#  GENESIS — universe configuration
#  Every physical parameter lives here, outside the code.
# =============================================================

[universe]
name = "Genesis"
# Total size of the universe (extent per axis, periodic torus).
size = { x = 128.0, y = 128.0, z = 128.0 }
# Time delta per tick (seconds of simulation).
dt = 0.016666666666666666
# Atoms seeded at startup.
initial_atoms = 100000
# Elements seeded at startup (symbols or full names); empty = all.
elements = ["Hydrogen", "Helium", "Carbon", "Nitrogen", "Oxygen"]
# Capacity of the metrics history.
stats_history = 1024

[rng]
seed = 42

[physics]
# Collision radius of each particle (diameter = 2·radius).
particle_radius = 0.4
# Initial temperature (defines the thermal velocity of the seeding).
initial_temperature = 300.0
# Speed limit (numeric safety).
speed_limit = 1000.0
# Effective thermal constant: T = (2/3)·⟨K⟩/thermal_constant.
thermal_constant = 0.01
# Reference gravitational constant (future law).
gravity_constant = 6.674e-11
# Thermostat target temperature (kelvin) and relaxation time (ticks).
thermostat_temperature = 300.0
thermostat_tau = 20.0

[systems]
enable_movement = true
enable_boundaries = true
# The LJ forces replace the hard spheres: the short-range repulsion already
# prevents overlap, so impulse collisions are not necessary.
enable_collisions = false
enable_forces = true
# Berendsen thermostat (velocity rescaling) for NVT runs. An instrument, not a
# law: off by default (NVE conserves energy).
enable_thermostat = false

[stats]
# Observation, not physics: velocity histogram.
histogram_bins = 32
histogram_max_speed = 10.0
# Every how many ticks the aggregates are measured (emergent structure).
structure_interval = 100
# Observability exports (disabled when empty): metrics CSV and position
# frames (XYZ) to plot outside the engine (matplotlib, gnuplot, OVITO).
csv_path = "data/stats.csv"
csv_interval = 10
xyz_prefix = "data/frames/frame"
xyz_interval = 200
"#;
