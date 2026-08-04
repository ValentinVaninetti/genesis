//! Persistent-pair experiment: do "bonds" emerge and survive?
//!
//! No bond law exists anywhere in the engine — binding is *observed* from
//! positions. This experiment feeds the per-tick bound pairs
//! (`r < k_bind · σ_ij`, threshold mixed per pair) into a `PairTracker` and
//! measures, for every contiguous "episode" of binding, its lifetime in
//! **vibrational periods** `T_vib = 2π·√(μ/k_well)` of that specific pair.
//!
//! A pair whose episode lasts many `T_vib` has genuinely bound (a persistent
//! bond); a pair that crosses the threshold once is a fly-by. The question is
//! whether persistent bonding *emerges* from the LJ law alone.
//!
//! Usage:
//! ```text
//! cargo run --release --example pairs [config.toml] [ticks]
//! ```
//! Collisions and dt come from the config file, so the same measurement can
//! be compared with `enable_collisions` on/off and `dt` halved. Results are
//! printed and written to `data/pairs.csv`.

use genesis::analysis::pairs::{collect_bound_pairs, Episode, PairTracker, DEFAULT_K_BIND};
use genesis::components::AtomType;
use genesis::config::Config;
use genesis::ecs::World;
use genesis::math::Vec3;
use genesis::physics::forces::{mix_epsilon, mix_sigma, vib_period};
use genesis::universe::Universe;

/// An episode counts as a *persistent bond* if it survives at least this
/// many vibrational periods of its own pair.
const PERSISTENT_PERIODS: f64 = 10.0;

/// A bound pair plus its measured episode, normalized to the pair's own
/// vibrational period.
#[derive(Clone, Copy)]
struct EpisodeStats {
    a: AtomType,
    b: AtomType,
    ticks: u64,
    vib_periods: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config_path = args.get(1).cloned();
    let ticks: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6_000);

    let config = match config_path.as_deref() {
        Some(path) => Config::from_file(path).unwrap_or_else(|e| {
            eprintln!("! cannot load {path}: {e}");
            std::process::exit(1);
        }),
        None => {
            let mut c = Config::default_config();
            c.universe.name = "Pairs experiment".into();
            c.universe.size = Vec3::new(40.0, 40.0, 40.0);
            c.universe.initial_atoms = 4_000;
            c.rng.seed = 7;
            c.physics.initial_temperature = 300.0;
            c.physics.thermostat_temperature = 150.0;
            c.systems.enable_thermostat = true;
            c
        }
    };

    let _ = std::fs::create_dir_all("data");
    let _ = std::fs::remove_file("data/pairs.csv");

    let mut universe = Universe::new(config);
    println!("{universe}");
    println!(
        "> tracking bound pairs (r < {DEFAULT_K_BIND}·σ_ij) for {ticks} ticks; \
         persistent ≥ {PERSISTENT_PERIODS:.0} T_vib …"
    );

    let mut tracker = PairTracker::new(1);
    for tick in 1..=ticks {
        universe.tick();
        let bound = collect_bound_pairs(
            &universe.world,
            universe.config.universe.size,
            DEFAULT_K_BIND,
            &universe.elements,
        );
        tracker.track_tick(&bound);
        if tick % 1_000 == 0 {
            println!(
                "[tick {:>5}] bound now={:>4} open episodes={:>4} closed={:>6} | E={:>10.3} T={:>6.1}",
                tick,
                bound.len(),
                tracker.open_count(),
                tracker.completed().len(),
                universe.stats.snapshot.energy_total,
                universe.stats.snapshot.temperature_avg,
            );
        }
    }
    tracker.close_all();

    let stats: Vec<EpisodeStats> = tracker
        .completed()
        .iter()
        .map(|e| normalize(world_pair(&universe.world, e), universe.config.universe.dt, universe.config.physics.thermal_constant))
        .collect();
    let still_open = tracker.open_count();
    write_csv("data/pairs.csv", &stats).expect("write pairs.csv");

    println!();
    print_summary(&stats, still_open);
}

/// Resolves the species of a completed episode (via the world that spawned
/// the entities).
fn world_pair(world: &World, e: &Episode) -> (Episode, AtomType, AtomType) {
    let a = world
        .get::<AtomType>(e.pair.a)
        .copied()
        .unwrap_or(AtomType::Hydrogen);
    let b = world
        .get::<AtomType>(e.pair.b)
        .copied()
        .unwrap_or(AtomType::Hydrogen);
    (*e, a, b)
}

/// Converts an episode to its lifetime in the pair's own vibrational periods:
/// `T_vib = 2π·√(μ/k_well)` with `k_well = 72·2^(−1/3)·ε_ij/σ_ij²` and the
/// reduced mass `μ`. `dt` turns ticks into simulation-time.
fn normalize((e, a, b): (Episode, AtomType, AtomType), dt: f64, k_thermal: f64) -> EpisodeStats {
    let mu = a.mass() * b.mass() / (a.mass() + b.mass());
    let eps = mix_epsilon(k_thermal, a, b);
    let sig = mix_sigma(a, b);
    EpisodeStats {
        a,
        b,
        ticks: e.ticks,
        vib_periods: e.ticks as f64 * dt / vib_period(eps, sig, mu),
    }
}

fn print_summary(stats: &[EpisodeStats], still_open: usize) {
    let total = stats.len();
    let mut persistent = 0usize;
    let mut total_ticks = 0u128;
    let mut total_periods = 0.0;
    let mut max_ticks = 0u64;
    let mut max: Option<EpisodeStats> = None;
    for s in stats {
        total_ticks += s.ticks as u128;
        total_periods += s.vib_periods;
        max_ticks = max_ticks.max(s.ticks);
        if s.vib_periods >= PERSISTENT_PERIODS {
            persistent += 1;
        }
        if max.is_none_or(|m| s.vib_periods > m.vib_periods) {
            max = Some(*s);
        }
    }
    let denom = total.max(1) as f64;

    println!("> pair episodes (contiguous bound stretches):");
    println!("    episodes: {total:>6} | still open at end: {still_open:>6}");
    println!(
        "    lifetime: mean {:.1} ticks ({:.1} T_vib) | max {max_ticks} ticks ({:.1} T_vib)",
        total_ticks as f64 / denom,
        total_periods / denom,
        max.map(|m| m.vib_periods).unwrap_or(0.0),
    );
    println!(
        "    persistent (≥ {PERSISTENT_PERIODS:.0} T_vib): {persistent}/{total} ({:.1}%)",
        persistent as f64 / denom * 100.0,
    );
    if let Some(m) = max {
        println!(
            "    longest episode: {a}–{b}: {} ticks = {:.1} T_vib",
            m.ticks,
            m.vib_periods,
            a = m.a.symbol(),
            b = m.b.symbol(),
        );
    }
    print_by_species(stats);
    println!("> episodes written to data/pairs.csv");
}

/// Per-species-pair breakdown: which element pairs actually bind. The
/// "emergent chemistry" of the run — Fe–Fe is expected to be the deepest
/// (well depth in kelvin: Fe 350 ≫ H 12).
fn print_by_species(stats: &[EpisodeStats]) {
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Acc {
        episodes: usize,
        persistent: usize,
        sum_ticks: u64,
        sum_periods: f64,
        max_periods: f64,
    }
    let mut by: BTreeMap<(&str, &str), Acc> = BTreeMap::new();
    for s in stats {
        let (sa, sb) = match s.a.symbol().cmp(s.b.symbol()) {
            std::cmp::Ordering::Less => (s.a.symbol(), s.b.symbol()),
            _ => (s.b.symbol(), s.a.symbol()),
        };
        let acc = by.entry((sa, sb)).or_default();
        acc.episodes += 1;
        if s.vib_periods >= PERSISTENT_PERIODS {
            acc.persistent += 1;
        }
        acc.sum_ticks += s.ticks;
        acc.sum_periods += s.vib_periods;
        acc.max_periods = acc.max_periods.max(s.vib_periods);
    }
    println!("    by species (episodes | mean T_vib | max T_vib | persistent%):");
    for ((a, b), acc) in &by {
        println!(
            "      {a:>2}–{b:<2}  {:>7}  {:>8.2}  {:>9.2}  {:>7.1}%",
            acc.episodes,
            acc.sum_periods / acc.episodes as f64,
            acc.max_periods,
            acc.persistent as f64 / acc.episodes as f64 * 100.0,
        );
    }
}

fn write_csv(path: &str, stats: &[EpisodeStats]) -> std::io::Result<()> {
    let mut out = String::from("species_a,species_b,ticks,vib_periods\n");
    for s in stats {
        out.push_str(&format!(
            "{},{},{},{:.3}\n",
            s.a.symbol(),
            s.b.symbol(),
            s.ticks,
            s.vib_periods
        ));
    }
    std::fs::write(path, out)
}
