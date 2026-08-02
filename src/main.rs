//! Punto de entrada del motor.
//!
//! Uso:
//! ```text
//! genesis [config.toml] [ticks] [reportar_cada]
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
    println!("> plan del scheduler (etapas sin conflicto de acceso):");
    let plan = universe.scheduler.plan().to_vec();
    for (i, stage) in plan.iter().enumerate() {
        let names: Vec<&str> = stage
            .systems
            .iter()
            .map(|&j| universe.scheduler.systems()[j].name())
            .collect();
        println!("    etapa {i}: {}", names.join(", "));
    }
    println!();

    let wall = Instant::now();
    for tick in 1..=ticks {
        universe.tick();
        if tick % report_every == 0 {
            let s = &universe.stats.snapshot;
            println!(
                "[tick {:>9}] t={:>10.3}s | entidades={:>9} | E={:>12.3} | K={:>12.3} | V={:>12.3} | E_avg={:>8.3} | T_avg={:>7.1} | |v|={:>6.3} | colisiones={:>8} | fps={:>7.1} | mem={:>8}kB",
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
            );
        }
    }
    let wall_secs = wall.elapsed().as_secs_f64();

    println!();
    println!("> {ticks} ticks en {wall_secs:.2}s → {:.0} ticks/s reales", ticks as f64 / wall_secs);
    println!("> {}", universe.status_line());
    println!("> histograma de velocidades:");
    print_histogram(&universe);

    // Demo de persistencia: guardar y retomar el universo.
    let save_path = "genesis-snapshot.bin";
    match universe.save(save_path) {
        Ok(()) => println!("> universo guardado en `{save_path}`"),
        Err(e) => eprintln!("! error al guardar: {e}"),
    }
    match Universe::load(save_path) {
        Ok(loaded) => println!(
            "> universo recargado: tick={} t={:.3}s entidades={} (idéntico al original)",
            loaded.time.tick,
            loaded.time.t,
            loaded.world.len(),
        ),
        Err(e) => eprintln!("! error al recargar: {e}"),
    }
}

/// Imprime el histograma de rapideces como barras ASCII.
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
