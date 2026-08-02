//! Temperature sweep: equilibrate a fixed-T (NVT) sample and measure how
//! structure emerges, to locate the condensation temperature.
//!
//! For each T in the range it builds a fresh universe (cubic lattice, 512
//! atoms in a 24³ torus), drives it to T with the Berendsen thermostat and
//! reports, on a single line per T:
//!
//! ```text
//! T  T_avg  V(potential)  n_aggregates  largest  bound_pairs  g_peak_r  g_peak
//! ```
//!
//! Run with `cargo run --release --example temperature_sweep`.

use genesis::config::Config;
use genesis::math::Vec3;
use genesis::universe::Universe;

fn main() {
    let atoms = 512;
    let size = Vec3::new(24.0, 24.0, 24.0);
    let tau = 20.0;
    let ticks = 3000;

    println!(
        "{:>4} {:>7} {:>9} {:>5} {:>5} {:>5} {:>7} {:>6}",
        "T", "T_avg", "V", "agg", "max", "bnd", "g_r", "g_peak"
    );
    for t in (10..=320).step_by(10) {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = atoms;
        cfg.universe.size = size;
        cfg.physics.initial_temperature = t as f64;
        cfg.physics.thermal_constant = 0.01;
        cfg.physics.thermostat_temperature = t as f64;
        cfg.physics.thermostat_tau = tau;
        cfg.systems.enable_forces = true;
        cfg.systems.enable_collisions = false;
        cfg.systems.enable_thermostat = true;

        let mut u = Universe::new(cfg);
        u.run_ticks(ticks);

        let s = &u.stats.snapshot;
        let c = u.cluster_analysis();
        let r_max = size.x.min(size.y).min(size.z) * 0.5;
        let g = u.radial_distribution(r_max, 512);
        let (gr, gp) = g.peak_in(0.5, r_max * 0.5).unwrap_or((0.0, 0.0));
        println!(
            "{:>4} {:>7.1} {:>9.1} {:>5} {:>5} {:>5} {:>7.2} {:>6.2}",
            t, s.temperature_avg, s.energy_potential, c.aggregates, c.largest, c.bound_pairs, gr, gp
        );
    }
}
