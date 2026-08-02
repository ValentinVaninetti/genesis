//! Persistence of the universe.
//!
//! A `Snapshot` serializes **all** the state: configuration, clock, RNG,
//! statistics and the complete `World` (entities with their components and
//! generations preserved byte by byte). Binary format (`bincode`) for speed
//! and size; the saved RNG allows resuming the simulation with identical
//! results.
//!
//! The component list is generated with the `for_each_component!` macro: any
//! future component is serialized automatically.

use crate::components::for_each_component;
use crate::components::{Acceleration, AtomType, Bonds, Charge, Mass, Position, Velocity};
use crate::config::Config;
use crate::ecs::{EntityId, World};
use crate::rng::Rng;
use crate::stats::StatsCollector;
use crate::universe::{time::Time, Universe};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Version of the save format. Increment on format breaks.
pub const FORMAT_VERSION: u32 = 4;

/// Global state of the universe (everything except the `World`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniverseState {
    pub time: Time,
    pub rng: Rng,
    pub stats: StatsCollector,
}

/// Complete snapshot, ready to write to disk.
#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    pub config: Config,
    pub state: UniverseState,
    pub world: Vec<EntitySnapshot>,
}

/// Data of one entity: generational id + all its components.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntitySnapshot {
    pub index: u32,
    pub generation: u32,
    pub position: Option<crate::components::Position>,
    pub velocity: Option<crate::components::Velocity>,
    pub mass: Option<crate::components::Mass>,
    pub charge: Option<crate::components::Charge>,
    pub atom_type: Option<crate::components::AtomType>,
    pub bonds: Option<crate::components::Bonds>,
    pub acceleration: Option<crate::components::Acceleration>,
}

macro_rules! gen_snapshot_impl {
    ($(($t:ident, $name:ident)),* $(,)?) => {
        impl EntitySnapshot {
            /// Captures an entity from the world.
            pub fn capture(world: &World, e: EntityId) -> Self {
                let mut s = EntitySnapshot {
                    index: e.index(),
                    generation: e.generation(),
                    $( $name: None, )*
                };
                $( s.$name = world.get::<$t>(e).cloned(); )*
                s
            }

            /// Restores the entity in the world (preserving its id).
            pub fn apply(self, world: &mut World) -> EntityId {
                let e = world.restore_entity(self.index, self.generation);
                $( if let Some(v) = self.$name {
                    world.insert::<$t>(e, v);
                } )*
                e
            }
        }
    };
}
for_each_component!(gen_snapshot_impl);

/// Saves the complete universe to a binary file.
pub fn save_universe(universe: &Universe, path: impl AsRef<Path>) -> Result<(), SaveError> {
    let world: Vec<EntitySnapshot> = universe
        .world
        .iter_entities()
        .map(|e| EntitySnapshot::capture(&universe.world, e))
        .collect();

    let snap = Snapshot {
        format: FORMAT_VERSION,
        config: universe.config.clone(),
        state: UniverseState {
            time: universe.time,
            rng: universe.rng.clone(),
            stats: universe.stats.clone(),
        },
        world,
    };

    let bytes = bincode::serialize(&snap)?;
    std::fs::write(path.as_ref(), bytes)?;
    Ok(())
}

/// Loads a saved universe.
pub fn load_universe(path: impl AsRef<Path>) -> Result<Universe, LoadError> {
    let bytes = std::fs::read(path.as_ref())?;
    let snap: Snapshot = bincode::deserialize(&bytes)?;
    if snap.format != FORMAT_VERSION {
        return Err(LoadError::Format(snap.format));
    }

    let mut universe = Universe::from_state(snap.config, snap.state);
    for entity in snap.world {
        entity.apply(&mut universe.world);
    }
    Ok(universe)
}

#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    Encode(bincode::Error),
}

impl From<std::io::Error> for SaveError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<bincode::Error> for SaveError {
    fn from(e: bincode::Error) -> Self {
        Self::Encode(e)
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "I/O error: {e}"),
            SaveError::Encode(e) => write!(f, "encoding error: {e}"),
        }
    }
}

impl std::error::Error for SaveError {}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Decode(bincode::Error),
    Format(u32),
}

impl From<std::io::Error> for LoadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<bincode::Error> for LoadError {
    fn from(e: bincode::Error) -> Self {
        Self::Decode(e)
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "I/O error: {e}"),
            LoadError::Decode(e) => write!(f, "decoding error: {e}"),
            LoadError::Format(v) => write!(f, "incompatible snapshot format (v{v})"),
        }
    }
}

impl std::error::Error for LoadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::universe::Universe;

    #[test]
    fn binary_roundtrip() {
        let config = Config::default_config();
        let mut u = Universe::new(config.clone());
        // A few ticks with some movement.
        u.run_ticks(10);

        let tmp = tempfile_path("roundtrip.bin");
        save_universe(&u, &tmp).unwrap();

        let loaded = load_universe(&tmp).unwrap();
        assert_eq!(loaded.world.len(), u.world.len());
        assert_eq!(loaded.time.tick, u.time.tick);
        assert_eq!(loaded.time.t, u.time.t);
        assert_eq!(loaded.config.universe.name, u.config.universe.name);
        assert_eq!(loaded.stats.snapshot.entities, u.stats.snapshot.entities);

        let _ = std::fs::remove_file(&tmp);
        let _ = tmp;
    }

    fn tempfile_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("genesis-test-{name}"));
        // make sure no old file exists
        if p.exists() {
            std::fs::remove_file(&p).ok();
        }
        p
    }
}
