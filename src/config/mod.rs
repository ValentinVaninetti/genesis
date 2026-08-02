//! Configuración del universo.
//!
//! Todos los parámetros físicos y de arranque viven **fuera del código**, en
//! TOML. La estructura es 100% tipada y derivada de `serde`, de modo que un
//! archivo inválido falla en el arranque con un error claro, nunca en runtime
//! con valores inventados.

use crate::math::Vec3;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configuración raíz.
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
    /// Nombre simbólico de la simulación.
    pub name: String,
    /// Tamaño (extensión total) del universo en cada eje.
    pub size: Vec3,
    /// Delta de tiempo por tick.
    pub dt: f64,
    /// Átomos sembrados al arrancar.
    pub initial_atoms: usize,
    /// Capacidad máxima de historia de métricas.
    pub stats_history: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RngConfig {
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PhysicsConfig {
    /// Radio de colisión de las partículas.
    #[serde(default = "default_particle_radius")]
    pub particle_radius: f64,
    /// Temperatura inicial uniforme de los átomos.
    pub initial_temperature: f64,
    /// Límite de velocidad (evita escapes numéricos).
    pub speed_limit: f64,
    /// Constante térmica efectiva del universo (equipartición: T = 2/3·⟨K⟩/k).
    #[serde(default = "default_thermal_constant")]
    pub thermal_constant: f64,
    /// Aceleración gravitatoria de referencia (ley futura).
    pub gravity_constant: f64,
}

const fn default_particle_radius() -> f64 {
    0.4
}

const fn default_thermal_constant() -> f64 {
    0.01
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct SystemsConfig {
    /// Activa el sistema de movimiento (integración Euler de posición).
    pub enable_movement: bool,
    /// Activa el sistema de límites (envoltura periódica).
    pub enable_boundaries: bool,
    /// Activa el sistema de colisiones elásticas (esferas duras).
    pub enable_collisions: bool,
    /// Activa las fuerzas intermoleculares (Lennard-Jones) y la integración
    /// de velocity Verlet. Cuando está activa, `enable_movement` se ignora.
    pub enable_forces: bool,
}

impl Default for SystemsConfig {
    fn default() -> Self {
        Self {
            enable_movement: true,
            enable_boundaries: true,
            enable_collisions: false,
            enable_forces: true,
        }
    }
}

/// Configuración de observación (no es física: solo cómo se mide e informa).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct StatsConfig {
    /// Cantidad de bins del histograma de velocidades.
    pub histogram_bins: usize,
    /// Velocidad máxima del histograma (rango 0..max).
    pub histogram_max_speed: f64,
    /// Cada cuántos ticks se muestrea la estructura (agregados).
    pub structure_interval: u64,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            histogram_bins: 32,
            histogram_max_speed: 10.0,
            structure_interval: 100,
        }
    }
}

impl Config {
    /// Configuración por defecto (equivalente al TOML embebido).
    pub fn default_config() -> Self {
        Self {
            universe: UniverseConfig {
                name: "Genesis".into(),
                size: Vec3::new(128.0, 128.0, 128.0),
                dt: 1.0 / 60.0,
                initial_atoms: 10_000,
                stats_history: 1024,
            },
            rng: RngConfig { seed: 42 },
            physics: PhysicsConfig {
                particle_radius: 0.4,
                initial_temperature: 300.0,
                speed_limit: 1_000.0,
                thermal_constant: 0.01,
                gravity_constant: 6.674e-11,
            },
            systems: SystemsConfig::default(),
            stats: StatsConfig::default(),
        }
    }

    /// Carga la configuración desde un archivo TOML.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path.as_ref())?;
        Self::from_toml(&text)
    }

    /// Parsea configuración desde texto TOML.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(text)?)
    }

    /// Intenta cargar el archivo; si no existe, usa el default y lo persiste.
    pub fn from_file_or_default(path: impl AsRef<Path>) -> Self {
        match Self::from_file(path.as_ref()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "[genesis] configuración no cargada ({}): usando valores por defecto",
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

/// Errores de carga de configuración.
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
            ConfigError::Io(e) => write!(f, "error de I/O: {e}"),
            ConfigError::Parse(e) => write!(f, "TOML inválido: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Escribe un archivo de configuración de ejemplo.
pub fn write_example(path: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::write(path, EXAMPLE_TOML)
}

pub const EXAMPLE_TOML: &str = r#"# =============================================================
#  GENESIS — configuración del universo
#  Todo parámetro físico vive aquí, fuera del código.
# =============================================================

[universe]
name = "Genesis"
# Tamaño total del universo (extensión por eje, toro periódico).
size = { x = 128.0, y = 128.0, z = 128.0 }
# Delta de tiempo por tick (segundos de simulación).
dt = 0.016666666666666666
# Átomos sembrados al arrancar.
initial_atoms = 100000
# Capacidad del historial de métricas.
stats_history = 1024

[rng]
seed = 42

[physics]
# Radio de colisión de cada partícula (diámetro = 2·radio).
particle_radius = 0.4
# Temperatura inicial (define la velocidad térmica del seeding).
initial_temperature = 300.0
# Límite de velocidad (seguridad numérica).
speed_limit = 1000.0
# Constante térmica efectiva: T = (2/3)·⟨K⟩/thermal_constant.
thermal_constant = 0.01
# Constante gravitatoria de referencia (ley futura).
gravity_constant = 6.674e-11

[systems]
enable_movement = true
enable_boundaries = true
# Las fuerzas LJ sustituyen a las esferas duras: la repulsión de corto alcance
# ya impide la superposición, y las colisiones por impulso no son necesarias.
enable_collisions = false
enable_forces = true

[stats]
# Observación, no física: histograma de velocidades.
histogram_bins = 32
histogram_max_speed = 10.0
# Cada cuántos ticks se miden los agregados (estructura emergente).
structure_interval = 100
"#;
