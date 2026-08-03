//! Observability exports: metrics CSV and position frames (XYZ).
//!
//! Pure I/O to plot outside the engine (matplotlib, gnuplot, OVITO). It is
//! observation, not a law: nothing here feeds the simulation, and the
//! functions are used by the CLI entry point and the examples.

use crate::components::{AtomType, Position};
use crate::ecs::World;
use crate::stats::StatsSnapshot;
use std::io::Write;
use std::path::Path;

/// Column header of the metrics CSV (one row per sampled tick).
pub const CSV_HEADER: &str = "tick,time_s,entities,energy_total,energy_kinetic,energy_potential,energy_avg,temperature_avg,mean_speed,density,collisions,systems_run,fps,memory_kb,aggregates,largest,monomers,bound_pairs";

/// One row of metrics for the CSV.
pub fn csv_row(s: &StatsSnapshot) -> String {
    let (aggregates, monomers, largest, bound) = match &s.structure {
        Some(st) => (st.aggregates, st.monomers, st.largest, st.bound_pairs),
        None => (0, 0, 0, 0),
    };
    format!(
        "{},{:.6},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{:.3},{:.1},{},{},{},{}",
        s.tick,
        s.time,
        s.entities,
        s.energy_total,
        s.energy_total - s.energy_potential,
        s.energy_potential,
        s.energy_avg,
        s.temperature_avg,
        s.mean_speed,
        s.density,
        s.collisions,
        s.systems_run,
        s.fps,
        s.memory_bytes as f64 / 1024.0,
        aggregates,
        largest,
        monomers,
        bound,
    )
}

/// Appends one metrics row to `path`, creating the file with the header if it
/// does not exist yet.
pub fn append_csv(path: impl AsRef<Path>, s: &StatsSnapshot) -> std::io::Result<()> {
    let path = path.as_ref();
    let fresh = !path.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if fresh {
        writeln!(file, "{CSV_HEADER}")?;
    }
    writeln!(file, "{}", csv_row(s))
}

/// Writes the current positions of the world in XYZ format (element + x y z).
///
/// The file is named `{prefix}_{tick:08}.xyz` so frames can be concatenated
/// or animated later.
pub fn write_frame(
    prefix: impl AsRef<Path>,
    tick: u64,
    world: &World,
) -> std::io::Result<()> {
    let mut out = String::new();
    out.push_str(&format!("{}\nframe {tick}\n", world.len()));
    world.for_each2::<Position, AtomType>(|_, pos, at| {
        out.push_str(&format!(
            "{} {:.6} {:.6} {:.6}\n",
            at.symbol(),
            pos.0.x,
            pos.0.y,
            pos.0.z
        ));
    });
    let path = format!("{}_{tick:08}.xyz", prefix.as_ref().display());
    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Acceleration, Mass, Velocity};
    use crate::math::Vec3;

    #[test]
    fn csv_row_matches_header_column_count() {
        let s = StatsSnapshot {
            tick: 7,
            time: 0.42,
            entities: 3,
            energy_total: 5.0,
            energy_avg: 1.5,
            energy_potential: 2.0,
            temperature_avg: 300.0,
            mean_speed: 1.1,
            density: 0.02,
            collisions: 1,
            systems_run: 6,
            fps: 10.0,
            memory_bytes: 2048,
            structure: Some(crate::stats::StructureStats {
                tick: 7,
                monomers: 1,
                aggregates: 1,
                largest: 2,
                mean_size: 1.5,
                bound_pairs: 1,
            }),
        };
        assert_eq!(CSV_HEADER.split(',').count(), csv_row(&s).split(',').count());
        let row = csv_row(&s);
        assert!(row.starts_with("7,0.420000,3,"));
        assert!(row.ends_with("1,2,1,1"));
    }

    #[test]
    fn xyz_frame_has_header_and_atoms() {
        crate::components::register_all();
        let mut w = World::new();
        for (pos, at) in [
            (Vec3::new(1.0, 2.0, 3.0), AtomType::Hydrogen),
            (Vec3::new(-1.0, 0.0, 0.5), AtomType::Carbon),
        ] {
            let e = w.spawn();
            w.insert::<Position>(e, Position(pos));
            w.insert::<AtomType>(e, at);
            w.insert::<Mass>(e, Mass(at.mass()));
            w.insert::<Velocity>(e, Velocity(Vec3::ZERO));
            w.insert::<Acceleration>(e, Acceleration(Vec3::ZERO));
        }
        let dir = std::env::temp_dir().join(format!("genesis-xyz-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let prefix = dir.join("frame");
        write_frame(&prefix, 0, &w).unwrap();
        let content = std::fs::read_to_string(dir.join("frame_00000000.xyz")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next(), Some("2"));
        assert_eq!(lines.next(), Some("frame 0"));
        let mut atoms = lines.collect::<Vec<_>>();
        atoms.sort_unstable();
        assert_eq!(atoms[0], "C -1.000000 0.000000 0.500000");
        assert_eq!(atoms[1], "H 1.000000 2.000000 3.000000");
    }
}
