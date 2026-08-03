//! Observability demo: run an NVT quench and export metrics CSV + XYZ frames.
//!
//! It seeds a cold-ish dense sample (4000 atoms in a 40³ torus), drives it to
//! `physics.thermostat_temperature` with the Berendsen thermostat and lets it
//! relax while writing:
//!
//! - `data/stats.csv` — one metrics row every 10 ticks (energy, temperature,
//!   aggregates, fps, memory…), for `tools/plot_stats.py`;
//! - `data/frames/frame_{tick:08}.xyz` — positions every 100 ticks, for OVITO
//!   or any XYZ viewer.
//!
//! Run with `cargo run --release --example observe_quench` from the repo root.
//! `tools/plot_stats.py data/stats.csv` plots the trajectory afterwards.

use genesis::config::Config;
use genesis::math::Vec3;
use genesis::universe::Universe;

fn main() {
    let mut config = Config::default_config();
    config.universe.name = "Quench demo".into();
    config.universe.size = Vec3::new(40.0, 40.0, 40.0);
    config.universe.initial_atoms = 4000;
    config.universe.stats_history = 4096;
    config.rng.seed = 7;
    config.physics.initial_temperature = 300.0;
    config.physics.thermostat_temperature = 40.0;
    config.systems.enable_thermostat = true;
    config.stats.structure_interval = 10;
    config.stats.csv_path = "data/stats.csv".into();
    config.stats.csv_interval = 10;
    config.stats.xyz_prefix = "data/frames/frame".into();
    config.stats.xyz_interval = 100;

    let dirs = ["data", "data/frames"];
    for d in dirs {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::remove_file("data/stats.csv");
    if let Ok(entries) = std::fs::read_dir("data/frames") {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }

    let mut universe = Universe::new(config);
    println!("{universe}");
    println!("quenching to {} K (NVT) for 4000 ticks…", universe.config.physics.thermostat_temperature);

    let ticks = 4000u64;
    for tick in 1..=ticks {
        universe.tick();
        let s = &universe.stats.snapshot;
        if tick % 200 == 0 {
            let structure = s
                .structure
                .map(|x| format!("T_avg={:.1} | ag={} largest={}", s.temperature_avg, x.aggregates, x.largest))
                .unwrap_or_else(|| format!("T_avg={:.1}", s.temperature_avg));
            println!("[tick {:>5}] E={:>10.3} V={:>10.3} {structure}", tick, s.energy_total, s.energy_potential);
        }
        if tick % universe.config.stats.csv_interval == 0
            && !universe.config.stats.csv_path.is_empty()
            && let Err(e) = genesis::export::append_csv(&universe.config.stats.csv_path, s)
        {
            eprintln!("! error writing CSV: {e}");
        }
        if tick % universe.config.stats.xyz_interval == 0
            && !universe.config.stats.xyz_prefix.is_empty()
            && let Err(e) =
                genesis::export::write_frame(&universe.config.stats.xyz_prefix, tick, &universe.world)
        {
            eprintln!("! error writing XYZ frame: {e}");
        }
    }

    let mut csv_rows = 0;
    if let Ok(content) = std::fs::read_to_string("data/stats.csv") {
        csv_rows = content.lines().count().saturating_sub(1);
    }
    let frames = std::fs::read_dir("data/frames")
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    let snapshot = std::fs::read_to_string("data/stats.csv").ok();
    let last = snapshot
        .and_then(|c| c.lines().next_back().map(str::to_string))
        .unwrap_or_default();
    println!();
    println!("done: {csv_rows} CSV rows, {frames} XYZ frames (data/)");
    println!("last CSV row: {last}");
}
