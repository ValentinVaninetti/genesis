//! Temperature sweep: equilibrate a fixed-T (NVT) sample and measure how
//! structure emerges, to locate the condensation temperature.
//!
//! For each T in the range it builds a fresh universe (cubic lattice, 512
//! atoms in a 24³ torus), drives it to T with the Berendsen thermostat and
//! reports, on a single line per T:
//!
//! ```text
//! T  T_avg  V/N  n_aggregates  largest  bound_pairs  g_peak_r  g_peak
//! ```
//!
//! Mixed mode (default) adds the partial `g(r)` of the two extreme wells,
//! carbon and hydrogen (`g_peak(C-C)` and `g_peak(H-H)`), to show that they
//! condense at very different temperatures:
//!
//! ```text
//! ... g_peak_r(C-C)  g_peak(C-C)  g_peak_r(H-H)  g_peak(H-H)
//! ```
//!
//! Pure mode (`cargo run --release --example temperature_sweep -- Carbon`)
//! seeds a single species and prints `T* = T/ε`: the structural transition
//! (coordination collapse in `g_peak`, flattening of `V/N`) of the pure fluid
//! must fall below the known Lennard-Jones critical point `T_c* ≈ 1.31` (the
//! liquid–gas transition does not exist above it), and at this density the
//! coexistence sits near `T* ≈ 0.6–1.0`.
//!
//! Run with `cargo run --release --example temperature_sweep`.

use genesis::components::AtomType;
use genesis::config::Config;
use genesis::math::Vec3;
use genesis::universe::Universe;

fn main() {
    let atoms = 512;
    let size = Vec3::new(24.0, 24.0, 24.0);
    let tau = 20.0;
    let ticks = 3000;

    let element = std::env::args().nth(1).and_then(|a| AtomType::by_name(&a));
    let pure = if let Some(e) = element {
        println!(
            "pure species sweep: {} (epsilon = {} K)",
            e.symbol(),
            genesis::physics::forces::epsilon(e)
        );
        Some(e)
    } else {
        println!("mixed {}-element sweep (per-species g(r): C-C and H-H)", AtomType::COUNT);
        None
    };

    if pure.is_some() {
        println!(
            "{:>4} {:>7} {:>9} {:>5} {:>5} {:>5} {:>7} {:>6} {:>6}",
            "T", "T_avg", "V/N", "agg", "max", "bnd", "g_r", "g_peak", "T/eps"
        );
    } else {
        println!(
            "{:>4} {:>7} {:>9} {:>5} {:>5} {:>5} {:>7} {:>6} | {:>7} {:>6} {:>7} {:>6}",
            "T", "T_avg", "V/N", "agg", "max", "bnd", "g_r", "g_peak", "g_rCC", "g_CC", "g_rHH", "g_HH"
        );
    }

    let range: Box<dyn Iterator<Item = i32>> = match pure {
        Some(_) => Box::new((20..=200).step_by(10)),
        None => Box::new((10..=320).step_by(10)),
    };

    for t in range {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = atoms;
        cfg.universe.size = size;
        if let Some(e) = pure {
            cfg.universe.elements = vec![e];
        }
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
        let vn = s.energy_potential / s.entities as f64;
        if let Some(e) = pure {
            // Reduced temperature T* = T/ε: the known LJ critical point is
            // T_c* ≈ 1.31, so T_c ≈ 1.31·ε. The structural transition of the
            // pure fluid must fall below it (coexistence at this density).
            let t_star = t as f64 / genesis::physics::forces::epsilon(e);
            println!(
                "{:>4} {:>7.1} {:>9.1} {:>5} {:>5} {:>5} {:>7.2} {:>6.2} {:>6.2}",
                t, s.temperature_avg, vn, c.aggregates, c.largest, c.bound_pairs, gr, gp, t_star
            );
        } else {
            let cc = u.radial_distribution_between(AtomType::Carbon, AtomType::Carbon, r_max, 512);
            let (ccr, ccg) = cc.peak_in(0.5, r_max * 0.5).unwrap_or((0.0, 0.0));
            let hh = u.radial_distribution_between(AtomType::Hydrogen, AtomType::Hydrogen, r_max, 512);
            let (hhr, hhg) = hh.peak_in(0.5, r_max * 0.5).unwrap_or((0.0, 0.0));
            println!(
                "{:>4} {:>7.1} {:>9.1} {:>5} {:>5} {:>5} {:>7.2} {:>6.2} | {:>7.2} {:>6.2} {:>7.2} {:>6.2}",
                t, s.temperature_avg, vn, c.aggregates, c.largest, c.bound_pairs, gr, gp, ccr, ccg, hhr, hhg
            );
        }
    }
}
