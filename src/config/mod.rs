//! Configuration of the universe.
//!
//! All the physical and startup parameters live **outside the code**, in
//! TOML. The structure is 100% typed and derived from `serde`, so an invalid
//! file fails at startup with a clear error, never at runtime with invented
//! values.

use crate::math::Vec3;
use crate::physics::forces::ElementOverride;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Effective Coulomb constant of the electrostatics law (energy·distance
    /// per elementary charge²). `F = k_e·q_i·q_j/r²`.
    #[serde(default = "default_coulomb_constant")]
    pub coulomb_constant: f64,
    /// Cutoff of the electrostatics term, in multiples of the maximum σ
    /// (default: same as the LJ).
    #[serde(default = "default_coulomb_cutoff")]
    pub coulomb_cutoff: f64,
    /// Cutoff of the gravitational term, in multiples of the maximum σ.
    #[serde(default = "default_gravity_cutoff")]
    pub gravity_cutoff: f64,
    /// Target temperature of the thermostat (kelvin).
    #[serde(default = "default_thermostat_temperature")]
    pub thermostat_temperature: f64,
    /// Thermostat relaxation time (in ticks).
    #[serde(default = "default_thermostat_tau")]
    pub thermostat_tau: f64,
    /// Affinity table overrides (σ, ε, charge) per element symbol. This is the
    /// knob to tune what "materials" the universe can form: a deep-ε element
    /// sticks to itself, a charged one binds electrostatically, a large-σ one
    /// reaches further. Keys are element symbols (`Na`, `O`, ...); missing
    /// fields keep the built-in default.
    #[serde(default)]
    pub elements: HashMap<String, ElementOverride>,
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

const fn default_coulomb_constant() -> f64 {
    1.0
}

const fn default_coulomb_cutoff() -> f64 {
    2.5
}

const fn default_gravity_cutoff() -> f64 {
    3.0
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
    /// Enables the electrostatics law (Coulomb) between charges, on top of the
    /// LJ term. Charge is a per-species law constant (see `forces::charge`).
    pub enable_electrostatics: bool,
    /// Enables the gravitational law between masses, on top of the LJ term.
    /// Truncated at `physics.gravity_cutoff`.
    pub enable_gravity: bool,
    /// Enables the Berendsen thermostat (velocity rescaling): drives the
    /// equipartition temperature toward `physics.thermostat_temperature`.
    /// It is an **instrument**, not a law (opt-in NVT).
    pub enable_thermostat: bool,
    /// Enables the persistent-bond observation: pairs whose bound episode
    /// survives `stats.bond_min_periods` vibrational periods are recorded in
    /// the `Bonds` component. Observation only — no bond law exists.
    pub enable_bond_observation: bool,
    /// Enables the physical coupling of the *observed* bonds: a pair recorded
    /// in `Bonds` feels a harmonic spring towards the LJ equilibrium distance
    /// (`k = well_curvature`, switched before the binding radius). The bond is
    /// still never programmed per species — it only exists because the
    /// observation measured it. Implies the observation itself is useful.
    pub enable_bond_interaction: bool,
}

impl Default for SystemsConfig {
    fn default() -> Self {
        Self {
            enable_movement: true,
            enable_boundaries: true,
            enable_collisions: false,
            enable_forces: true,
            enable_electrostatics: false,
            enable_gravity: false,
            enable_thermostat: false,
            enable_bond_observation: false,
            enable_bond_interaction: false,
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
    /// Episodes shorter than this many vibrational periods of the pair do not
    /// count as a persistent bond (bond observation).
    #[serde(default = "default_bond_min_periods")]
    pub bond_min_periods: f64,
    /// Binding threshold of the bond observation, as a multiple of the pair's
    /// mixed σ: a pair is "bound" while `r < bond_k_bind·σ_ij`.
    #[serde(default = "default_bond_k_bind")]
    pub bond_k_bind: f64,
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
            bond_min_periods: 10.0,
            bond_k_bind: 1.5,
        }
    }
}

const fn default_bond_min_periods() -> f64 {
    10.0
}

const fn default_bond_k_bind() -> f64 {
    1.5
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
                coulomb_constant: 1.0,
                coulomb_cutoff: 2.5,
                gravity_cutoff: 3.0,
                thermostat_temperature: 300.0,
                thermostat_tau: 20.0,
                elements: HashMap::new(),
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
        let c: Self = toml::from_str(text)?;
        c.validate()?;
        Ok(c)
    }

    /// Semantic validation: fails fast on values that would silently do nothing
    /// (e.g. an affinity-table key for an unknown element symbol).
    fn validate(&self) -> Result<(), ConfigError> {
        for symbol in self.physics.elements.keys() {
            if crate::components::AtomType::by_name(symbol).is_none() {
                return Err(ConfigError::Validation(format!(
                    "affinity table references unknown element symbol `{symbol}`"
                )));
            }
        }
        Ok(())
    }

    /// Tries to load the file; if it does not exist, uses the default and
    /// persists it (create-on-first-run). If the file exists but is invalid,
    /// falls back to the defaults **without touching the file**: a broken
    /// config must never be silently destroyed.
    pub fn from_file_or_default(path: impl AsRef<Path>) -> Self {
        match Self::from_file(path.as_ref()) {
            Ok(c) => c,
            Err(_) if !path.as_ref().exists() => {
                eprintln!(
                    "[genesis] configuration not found: creating `{}` with default values",
                    path.as_ref().display()
                );
                let c = Self::default_config();
                let _ = std::fs::create_dir_all(
                    path.as_ref().parent().unwrap_or(Path::new(".")),
                );
                let text = toml::to_string_pretty(&c).expect("config serializable");
                let _ = std::fs::write(path.as_ref(), text);
                c
            }
            Err(e) => {
                eprintln!(
                    "[genesis] configuration not loaded ({}): using default values",
                    e
                );
                Self::default_config()
            }
        }
    }
}

/// Configuration loading errors.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Validation(String),
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
            ConfigError::Validation(msg) => write!(f, "invalid configuration: {msg}"),
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
# Electrostatics (Coulomb): effective constant and cutoff (× max σ).
coulomb_constant = 1.0
coulomb_cutoff = 2.5
# Gravity cutoff (× max σ): the term is truncated and smoothly switched.
gravity_cutoff = 3.0
# Thermostat target temperature (kelvin) and relaxation time (ticks).
thermostat_temperature = 300.0
thermostat_tau = 20.0
# Affinity table: per-element overrides of σ, ε (kelvin) and charge. This is
# the knob that tunes what "materials" the universe can form: deep-ε elements
# stick to themselves, charged ones bind electrostatically, large-σ ones reach
# further. Missing fields keep the built-in default.
# [physics.elements]
# Na = { sigma = 2.2, epsilon_k = 130.0, charge = 1.0 }
# O = { charge = -1.0 }

[systems]
enable_movement = true
enable_boundaries = true
# The LJ forces replace the hard spheres: the short-range repulsion already
# prevents overlap, so impulse collisions are not necessary.
enable_collisions = false
enable_forces = true
# Additional laws, off by default: electrostatics (charges in the element
# table) and gravity (truncated). Both are added to the LJ force pass.
enable_electrostatics = false
enable_gravity = false
# Berendsen thermostat (velocity rescaling) for NVT runs. An instrument, not a
# law: off by default (NVE conserves energy).
enable_thermostat = false
# Persistent-bond observation: records in the Bonds component the pairs whose
# bound episode survives `bond_min_periods` vibrational periods. Observation
# only — no bond law exists.
enable_bond_observation = false
# Physical coupling of the observed bonds: a pair recorded in Bonds feels a
# harmonic spring towards the LJ equilibrium distance. The bond is still never
# programmed per species — it only exists because the observation measured it.
enable_bond_interaction = false

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
# Persistent-bond observation thresholds.
bond_min_periods = 10.0
bond_k_bind = 1.5
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "genesis-config-{}-{}",
            std::process::id(),
            name
        ))
    }

    #[test]
    fn invalid_existing_config_is_never_overwritten() {
        let p = temp_path("invalid.toml");
        let original = "this is not [valid toml\n";
        std::fs::write(&p, original).unwrap();

        let c = Config::from_file_or_default(&p);

        let after = std::fs::read_to_string(&p).unwrap();
        assert_eq!(after, original, "broken config was clobbered by defaults");
        assert_eq!(c.universe.name, "Genesis");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn missing_config_is_created_with_defaults() {
        let p = temp_path("missing.toml");
        let _ = std::fs::remove_file(&p);

        let c = Config::from_file_or_default(&p);

        assert!(p.exists(), "default config was not persisted");
        let reparsed = Config::from_file(&p).expect("persisted default reparses");
        assert_eq!(reparsed.universe.initial_atoms, c.universe.initial_atoms);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn physics_requires_its_mandatory_fields() {
        let text = r#"
            [universe]
            name = "t"
            size = { x = 10.0, y = 10.0, z = 10.0 }
            dt = 0.02
            initial_atoms = 100
            stats_history = 256

            [rng]
            seed = 1

            [physics]
            initial_temperature = 300.0
            speed_limit = 1000.0
        "#;
        assert!(
            Config::from_toml(text).is_err(),
            "gravity_constant must be required (fail-fast, never invented values)"
        );
    }

    #[test]
    fn partial_config_with_optional_knobs_only_works() {
        let text = r#"
            [universe]
            name = "t"
            size = { x = 10.0, y = 10.0, z = 10.0 }
            dt = 0.02
            initial_atoms = 100
            stats_history = 256

            [rng]
            seed = 1

            [physics]
            initial_temperature = 300.0
            speed_limit = 1000.0
            gravity_constant = 6.674e-11

            [systems]
            enable_electrostatics = true
            enable_bond_observation = true
            enable_bond_interaction = true
        "#;
        let c = Config::from_toml(text).expect("valid partial config");
        assert!(c.systems.enable_electrostatics);
        assert!(c.systems.enable_bond_observation);
        assert!(c.systems.enable_bond_interaction);
        assert_eq!(c.physics.coulomb_constant, 1.0);
        assert_eq!(c.stats.bond_min_periods, 10.0);
    }

    #[test]
    fn affinity_table_parses_and_roundtrips() {
        let text = r#"
            [universe]
            name = "t"
            size = { x = 10.0, y = 10.0, z = 10.0 }
            dt = 0.02
            initial_atoms = 100
            stats_history = 256

            [rng]
            seed = 1

            [physics]
            initial_temperature = 300.0
            speed_limit = 1000.0
            gravity_constant = 6.674e-11

            [physics.elements]
            Na = { sigma = 2.2, epsilon_k = 130.0, charge = 1.0 }
            O = { charge = -2.0 }

            [systems]
            enable_electrostatics = true
        "#;
        let c = Config::from_toml(text).expect("affinity table is valid");
        let na = c.physics.elements.get("Na").expect("Na override parsed");
        assert_eq!(na.sigma, Some(2.2));
        assert_eq!(na.epsilon_k, Some(130.0));
        assert_eq!(na.charge, Some(1.0));
        // Partial overrides stay partial.
        let o = c.physics.elements.get("O").expect("O override parsed");
        assert_eq!(o.sigma, None);
        assert_eq!(o.charge, Some(-2.0));
        // And the round trip preserves them.
        let reparsed = Config::from_toml(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(reparsed.physics.elements["Na"].charge, Some(1.0));
    }

    #[test]
    fn affinity_table_with_unknown_symbol_is_rejected() {
        let text = r#"
            [universe]
            name = "t"
            size = { x = 10.0, y = 10.0, z = 10.0 }
            dt = 0.02
            initial_atoms = 100
            stats_history = 256

            [rng]
            seed = 1

            [physics]
            initial_temperature = 300.0
            speed_limit = 1000.0
            gravity_constant = 6.674e-11

            [physics.elements]
            Faux = { charge = -2.0 }

            [systems]
            enable_electrostatics = true
        "#;
        let err = Config::from_toml(text).expect_err("unknown symbol fails validation");
        assert!(
            err.to_string().contains("Faux"),
            "error should name the offending symbol, got: {err}"
        );
    }
}
