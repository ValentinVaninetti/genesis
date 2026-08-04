//! Engine entry point.
//!
//! Usage:
//! ```text
//! genesis [config.toml] [ticks] [report_every]
//! ```

use genesis::config::Config;
use genesis::stats::velocity_histogram;
use genesis::universe::Universe;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "config/universe.toml".to_string());
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1_000);
    let report_every: u64 = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let config = Config::from_file_or_default(&config_path);
    let mut universe = Universe::new(config);

    println!("{universe}");
    println!("> scheduler plan (access-conflict-free stages):");
    let plan = universe.scheduler.plan().to_vec();
    for (i, stage) in plan.iter().enumerate() {
        let names: Vec<&str> = stage
            .systems
            .iter()
            .map(|&j| universe.scheduler.systems()[j].name())
            .collect();
        println!("    stage {i}: {}", names.join(", "));
    }
    println!();

    let wall = Instant::now();
    for tick in 1..=ticks {
        universe.tick();
        if tick % report_every == 0 {
            let s = &universe.stats.snapshot;
            let structure = s
                .structure
                .map(|x| format!(" | ag={} mon={} largest={} bound={} bonds={} lt={:.0}", x.aggregates, x.monomers, x.largest, x.bound_pairs, s.bonded_pairs, s.bond_lifetime_ticks))
                .unwrap_or_default();
            println!(
                "[tick {:>9}] t={:>10.3}s | entities={:>9} | E={:>12.3} | K={:>12.3} | V={:>12.3} | E_avg={:>8.3} | T_avg={:>7.1} | |v|={:>6.3} | collisions={:>8} | fps={:>7.1} | mem={:>8}kB{}",
                s.tick,
                s.time,
                s.entities,
                s.energy_total,
                s.energy_total - s.energy_potential,
                s.energy_potential,
                s.energy_avg,
                s.temperature_avg,
                s.mean_speed,
                s.collisions,
                s.fps,
                s.memory_bytes / 1024,
                structure,
            );
        }
        if tick % universe.config.stats.csv_interval == 0
            && !universe.config.stats.csv_path.is_empty()
            && let Err(e) =
                genesis::export::append_csv(&universe.config.stats.csv_path, &universe.stats.snapshot)
        {
            eprintln!("! error writing CSV: {e}");
        }
        if tick % universe.config.stats.xyz_interval == 0
            && !universe.config.stats.xyz_prefix.is_empty()
            && let Err(e) = genesis::export::write_frame(
                &universe.config.stats.xyz_prefix,
                tick,
                &universe.world,
            )
        {
            eprintln!("! error writing XYZ frame: {e}");
        }
    }
    let wall_secs = wall.elapsed().as_secs_f64();

    println!();
    println!("> {ticks} ticks in {wall_secs:.2}s → {:.0} real ticks/s", ticks as f64 / wall_secs);
    println!("> {}", universe.status_line());
    println!("> velocity histogram:");
    print_histogram(&universe);
    print_structure(&universe);

    // Persistence demo: save and resume the universe.
    let save_path = "genesis-snapshot.bin";
    match universe.save(save_path) {
        Ok(()) => println!("> universe saved to `{save_path}`"),
        Err(e) => eprintln!("! error saving: {e}"),
    }
    match Universe::load(save_path) {
        Ok(loaded) => println!(
            "> universe reloaded: tick={} t={:.3}s entities={} (identical to the original)",
            loaded.time.tick,
            loaded.time.t,
            loaded.world.len(),
        ),
        Err(e) => eprintln!("! error reloading: {e}"),
    }
}

/// Prints the emergent structure: aggregates and the first peak of g(r).
fn print_structure(universe: &Universe) {
    let clusters = universe.cluster_analysis();
    println!("> emergent structure (observation, not a law):");
    println!(
        "    aggregates: {} | monomers: {} | largest: {} | bound pairs: {} | mean size: {:.2}",
        clusters.aggregates,
        clusters.monomers,
        clusters.largest,
        clusters.bound_pairs,
        clusters.mean_size,
    );

    let size = universe.config.universe.size;
    let r_max = size.x.min(size.y).min(size.z) * 0.5;
    let g = universe.radial_distribution(r_max, 512);
    // Coordination shell window: half the range avoids the stacking of torus
    // distances near L/2.
    match g.peak_in(0.5, r_max * 0.5) {
        Some((r, value)) => println!(
            "    g(r): first neighbor at r≈{r:.2} (g={value:.2}) → {}",
            if value > 3.0 {
                "clear local order (condensed phase)"
            } else if value > 1.5 {
                "some local order (liquid/aggregates)"
            } else {
                "no local order (gas)"
            },
        ),
        None => println!("    g(r): no peaks (empty system)"),
    }

    // The "chemistry" lens: connected components of the observed persistent-
    // bond graph, each labeled by its stoichiometry and binding energy.
    if let Some(chem) = universe.stats.snapshot().chemical.as_ref() {
        let formulas = chem
            .compositions
            .iter()
            .take(8)
            .map(|e| format!("{}:{}@{:.2}", e.formula, e.count, e.mean_binding))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "    chemical (observed bond graph): {} aggregates | largest: {} | bonded: {} | monomers: {} | stoichiometries: {}",
            chem.aggregates, chem.largest, chem.bound_entities, chem.monomers, formulas
        );
        println!(
            "    lifecycle (since last sample): +{} appeared, −{} disappeared, {} fusions, {} scissions",
            chem.appeared, chem.disappeared, chem.fusions, chem.scissions
        );
    }
}

/// Prints the speed histogram as ASCII bars.
fn print_histogram(universe: &Universe) {
    let cfg = &universe.config.stats;
    let hist = velocity_histogram(&universe.world, cfg.histogram_max_speed, cfg.histogram_bins);
    let max = hist.bins.iter().copied().max().unwrap_or(0).max(1);

    const WIDTH: usize = 32;
    for (i, &count) in hist.bins.iter().enumerate() {
        let lo = i as f64 * hist.bin_width;
        let hi = lo + hist.bin_width;
        let bar = (count as f64 / max as f64 * WIDTH as f64).round() as usize;
        let pct = count as f64 / hist.samples.max(1) as f64 * 100.0;
        println!(
            "  {:>5.2}–{:>5.2} | {:<WIDTH$} {:>9} {:>5.1}%",
            lo,
            hi,
            "█".repeat(bar),
            count,
            pct,
            WIDTH = WIDTH,
        );
    }
    if hist.overflow > 0 {
        println!(
            "  >{:.2}   | {:<WIDTH$} {:>9}",
            hist.max_speed,
            "…",
            hist.overflow,
            WIDTH = WIDTH,
        );
    }
}
