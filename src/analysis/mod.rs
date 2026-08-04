//! Analysis of emergent structure.
//!
//! These are not laws: they are **lenses**. All the physics lives in
//! `physics/`; here we only measure consequences of the laws to observe
//! whether structure emerges (condensation, aggregates, order). Nothing in
//! here feeds the simulation: it is pure observation, with the same
//! philosophy as `stats`.

use crate::components::AtomType;
use crate::math::Vec3;
use crate::physics::forces::sigma;
use crate::physics::grid::{min_image, Particle, SpatialGrid};

pub mod pairs;

/// "Contact" factor for the aggregate analysis: two atoms belong to the same
/// cluster when `r < BOND_FACTOR · σ_ij`. `1.5·σ` is inside the attractive
/// well of the LJ potential (whose minimum lives at `1.12·σ`).
pub const BOND_FACTOR: f64 = 1.5;

/// Population cap for the `g(r)` pair counting. The broadphase grid uses a
/// cell of `r_max`, and with `r_max ≈ L/2` (the default of the CLI summary) a
/// large box leaves few cells and O(n²) pairs. Above this cap a deterministic
/// evenly-spaced subsample is used, which keeps the box coverage and the mean
/// density of the sample, so `g(r)` stays representative while the cost is
/// bounded.
const G_SAMPLE_CAP: usize = 8192;

/// Deterministic, evenly-spaced subsample of a population (and its types).
/// Returns the input unchanged (as owned copies) when below the cap.
fn g_subsample(particles: &[Particle], types: &[AtomType]) -> (Vec<Particle>, Vec<AtomType>) {
    let with_types = types.len() == particles.len();
    if particles.len() <= G_SAMPLE_CAP {
        return (particles.to_vec(), types.to_vec());
    }
    let step = particles.len() / G_SAMPLE_CAP + 1;
    let count = particles.len() / step;
    let pts: Vec<Particle> = (0..count).map(|i| particles[i * step]).collect();
    let tys: Vec<AtomType> = if with_types {
        (0..count).map(|i| types[i * step]).collect()
    } else {
        Vec::new()
    };
    (pts, tys)
}

/// Radial distribution function `g(r)` of the system.
///
/// Normalized so that `g(r) → 1` in an ideal gas: each shell is divided by
/// the expected number of neighbors of a homogeneous gas at the mean density.
#[derive(Debug, Clone)]
pub struct RadialDistribution {
    /// Maximum distance considered (≤ half of the shortest side of the torus).
    pub r_max: f64,
    /// Width of each bin.
    pub dr: f64,
    /// `g(r)` per bin (bin `i` centered at `(i + 0.5)·dr`).
    pub bins: Vec<f64>,
}

impl RadialDistribution {
    /// Maximum of `g(r)` in the window `[from, to]`: `(r, g)`. With `to`
    /// below the torus stacking (≈ `L/2`), it captures the coordination
    /// shell (first neighbor). A high, well-defined peak reveals local order
    /// (liquid/solid); its absence, a gas.
    pub fn peak_in(&self, from: f64, to: f64) -> Option<(f64, f64)> {
        let mut best: Option<(f64, f64)> = None;
        for (i, &g) in self.bins.iter().enumerate() {
            let r = (i as f64 + 0.5) * self.dr;
            if r < from || r > to {
                continue;
            }
            if g > best.map(|(_, bg)| bg).unwrap_or(f64::NEG_INFINITY) {
                best = Some((r, g));
            }
        }
        best
    }
}

/// Builds `g(r)` from the particles and the world size.
///
/// `r_max` is clipped to `min(size)/2`: on a torus the minimum-image
/// distances are only exact up to half of the shortest side. The spatial
/// grid avoids the O(n²) double loop.
pub fn radial_distribution(
    particles: &[Particle],
    world_size: Vec3,
    r_max: f64,
    nbins: usize,
) -> RadialDistribution {
    let nbins = nbins.clamp(1, 1 << 16);
    let half_min = world_size.x.min(world_size.y).min(world_size.z) * 0.5;
    let r_max = r_max.max(1e-9).min(half_min);
    let dr = r_max / nbins as f64;
    let vol = world_size.x * world_size.y * world_size.z;

    let (subsample, _) = g_subsample(particles, &[]);
    let particles = subsample.as_slice();
    let n = particles.len();

    let mut grid = SpatialGrid::new(world_size, r_max);
    grid.build(particles);
    let mut pairs = Vec::new();
    grid.neighbors(particles, r_max, &mut pairs);

    let mut counts = vec![0u64; nbins];
    for &p in &pairs {
        let a = &particles[p.a];
        let b = &particles[p.b];
        let d = min_image(a.pos - b.pos, world_size).length();
        if d >= r_max {
            continue;
        }
        counts[(d / dr) as usize] += 1;
    }

    let mut bins = vec![0.0f64; nbins];
    if n < 2 {
        return RadialDistribution { r_max, dr, bins };
    }
    for (i, &c) in counts.iter().enumerate() {
        let r_lo = i as f64 * dr;
        let r_hi = r_lo + dr;
        let shell = 4.0 / 3.0 * std::f64::consts::PI * (r_hi * r_hi * r_hi - r_lo * r_lo * r_lo);
        // Unordered pairs expected in the shell of a homogeneous gas: the
        // grid reports each pair only once, so N(N−1)/2.
        let expected = (n * (n - 1)) as f64 * 0.5 * shell / vol;
        bins[i] = c as f64 / expected.max(f64::MIN_POSITIVE);
    }
    RadialDistribution { r_max, dr, bins }
}

/// Partial radial distribution `g_ab(r)` between two species.
///
/// Counts the pairs with one atom of type `ta` and one of `tb` (`ta == tb`
/// counts only both of that type) and normalizes per species, so that a
/// random homogeneous mixture gives `g → 1` in each shell: the expected pairs
/// are `N_a·N_b·shell/V` (`a≠b`) or `N_a(N_a−1)/2·shell/V` (`a==b`), and the
/// grid reports each pair exactly once. A peak reveals that both species
/// coordinate at that distance, well below the other elements' average.
pub fn radial_distribution_between(
    particles: &[Particle],
    types: &[AtomType],
    ta: AtomType,
    tb: AtomType,
    world_size: Vec3,
    r_max: f64,
    nbins: usize,
) -> RadialDistribution {
    let nbins = nbins.clamp(1, 1 << 16);
    let half_min = world_size.x.min(world_size.y).min(world_size.z) * 0.5;
    let r_max = r_max.max(1e-9).min(half_min);
    let dr = r_max / nbins as f64;
    let vol = world_size.x * world_size.y * world_size.z;

    let (subsample, subsample_types) = g_subsample(particles, types);
    let particles = subsample.as_slice();
    let types = subsample_types.as_slice();
    let na = types.iter().filter(|&&t| t == ta).count();
    let nb = types.iter().filter(|&&t| t == tb).count();

    let mut grid = SpatialGrid::new(world_size, r_max);
    grid.build(particles);
    let mut pairs = Vec::new();
    grid.neighbors(particles, r_max, &mut pairs);

    let mut counts = vec![0u64; nbins];
    for &p in &pairs {
        let (ta_p, tb_p) = (types[p.a], types[p.b]);
        if !((ta_p == ta && tb_p == tb) || (ta_p == tb && tb_p == ta)) {
            continue;
        }
        let a = &particles[p.a];
        let b = &particles[p.b];
        let d = min_image(a.pos - b.pos, world_size).length();
        if d >= r_max {
            continue;
        }
        counts[(d / dr) as usize] += 1;
    }

    let pair_count = if ta == tb {
        (na as f64) * (na.saturating_sub(1) as f64) * 0.5
    } else {
        (na as f64) * (nb as f64)
    };
    let mut bins = vec![0.0f64; nbins];
    if pair_count <= 0.0 {
        return RadialDistribution { r_max, dr, bins };
    }
    for (i, &c) in counts.iter().enumerate() {
        let r_lo = i as f64 * dr;
        let r_hi = r_lo + dr;
        let shell = 4.0 / 3.0 * std::f64::consts::PI * (r_hi * r_hi * r_hi - r_lo * r_lo * r_lo);
        let expected = pair_count * shell / vol;
        bins[i] = c as f64 / expected.max(f64::MIN_POSITIVE);
    }
    RadialDistribution { r_max, dr, bins }
}

/// Summary of aggregates detected with friends-of-friends.
#[derive(Debug, Clone, Default, Copy, PartialEq)]
pub struct ClusterStats {
    /// Clusters of a single atom.
    pub monomers: usize,
    /// Clusters of ≥ 2 atoms.
    pub aggregates: usize,
    /// Size of the largest aggregate.
    pub largest: usize,
    /// Mean size over all clusters (including monomers).
    pub mean_size: f64,
    /// Atom pairs in contact (edges of the aggregation graph).
    pub bound_pairs: usize,
}

/// Detects aggregates: two atoms belong to the same cluster if they are at
/// `r < bond_factor · σ_ij` (σ_ij by Lorentz mixing). Union-find with path
/// compression over the grid pairs (which acts as a superset).
pub fn clusters(
    particles: &[Particle],
    types: &[AtomType],
    world_size: Vec3,
    bond_factor: f64,
) -> ClusterStats {
    let empty = ClusterStats::default();
    let n = particles.len();
    if n == 0 {
        return empty;
    }
    debug_assert_eq!(particles.len(), types.len());

    // Sigma of each particle and maximum sigma (grid cutoff: superset).
    let sigmas: Vec<f64> = types.iter().map(|&t| sigma(t)).collect();
    let max_sigma = sigmas.iter().copied().fold(0.0, f64::max);
    let cutoff = (bond_factor.max(0.1) * max_sigma).max(1e-9);

    let mut grid = SpatialGrid::new(world_size, cutoff);
    grid.build(particles);
    let mut pairs = Vec::new();
    grid.neighbors(particles, cutoff, &mut pairs);

    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank = vec![0u32; n];
    let mut bound_pairs = 0usize;
    for &p in &pairs {
        let d = min_image(particles[p.a].pos - particles[p.b].pos, world_size).length();
        let bond = bond_factor * 0.5 * (sigmas[p.a] + sigmas[p.b]);
        if d < bond {
            union(&mut parent, &mut rank, p.a, p.b);
            bound_pairs += 1;
        }
    }

    let mut sizes = vec![0usize; n];
    for i in 0..n {
        sizes[find(&mut parent, i)] += 1;
    }
    let mut monomers = 0usize;
    let mut aggregates = 0usize;
    let mut largest = 0usize;
    let mut total = 0usize;
    for &s in &sizes {
        if s == 0 {
            continue;
        }
        total += 1;
        if s == 1 {
            monomers += 1;
        } else {
            aggregates += 1;
        }
        largest = largest.max(s);
    }
    ClusterStats {
        monomers,
        aggregates,
        largest,
        mean_size: n as f64 / total.max(1) as f64,
        bound_pairs,
    }
}

/// Union by rank with path compression.
fn union(parent: &mut [usize], rank: &mut [u32], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra == rb {
        return;
    }
    let (hi, lo) = if rank[ra] < rank[rb] { (rb, ra) } else { (ra, rb) };
    parent[lo] = hi;
    if rank[hi] == rank[lo] {
        rank[hi] += 1;
    }
}

/// Root with path compression.
fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    fn hydrogen_at(pos: Vec3) -> (Particle, AtomType) {
        (
            Particle {
                index: 0,
                pos,
                vel: Vec3::ZERO,
                mass: 1.008,
            },
            AtomType::Hydrogen,
        )
    }

    #[test]
    fn g_r_of_a_pair_peaks_at_the_distance() {
        // A single pair at distance d → all the weight in the bin at r≈d.
        let world = Vec3::new(32.0, 32.0, 32.0);
        let d = 1.7;
        let particles = vec![
            Particle { index: 0, pos: Vec3::new(0.0, 0.0, 0.0), vel: Vec3::ZERO, mass: 1.0 },
            Particle { index: 1, pos: Vec3::new(d, 0.0, 0.0), vel: Vec3::ZERO, mass: 1.0 },
        ];
        let g = radial_distribution(&particles, world, 4.0, 400);
        assert!((g.dr - 0.01).abs() < 1e-12);
        let (r_peak, g_peak) = g.peak_in(0.5, 3.0).expect("there must be a peak");
        assert!((r_peak - d).abs() < 0.03, "peak at r={r_peak}, expected ~{d}");
        assert!(g_peak > 1_000.0, "a concentrated pair must spike g(r): {g_peak}");
    }

    #[test]
    fn g_r_of_random_gas_is_approximately_one() {
        let world = Vec3::new(32.0, 32.0, 32.0);
        let mut rng = Rng::new(7);
        let particles: Vec<Particle> = (0..400)
            .map(|i| Particle {
                index: i,
                pos: rng.in_box(world.scale(0.5)),
                vel: Vec3::ZERO,
                mass: 1.0,
            })
            .collect();
        let g = radial_distribution(&particles, world, 8.0, 32);
        let mean = g.bins.iter().sum::<f64>() / g.bins.len() as f64;
        assert!(
            (0.8..=1.2).contains(&mean),
            "ideal gas → g≈1, got {mean}"
        );
    }

    #[test]
    fn partial_g_r_isolates_the_requested_species() {
        // One C–C pair at r=1.8 inside a dense sea of random H: g_CC must
        // spike, g_HH must stay ~1 (ideal gas) and g_CH must show no
        // coordination. Proves the species filter of `_between`.
        let world = Vec3::new(32.0, 32.0, 32.0);
        let mut particles: Vec<Particle> = Vec::new();
        let mut types = Vec::new();
        let mut rng = Rng::new(11);
        for i in 0..2000 {
            particles.push(Particle {
                index: i,
                pos: rng.in_box(world.scale(0.5)),
                vel: Vec3::ZERO,
                mass: 1.0,
            });
            types.push(AtomType::Hydrogen);
        }
        particles[0].pos = Vec3::new(0.0, 0.0, 0.0);
        particles[1].pos = Vec3::new(1.8, 0.0, 0.0);
        types[0] = AtomType::Carbon;
        types[1] = AtomType::Carbon;

        let g_cc = radial_distribution_between(&particles, &types, AtomType::Carbon, AtomType::Carbon, world, 4.0, 400);
        let (_, gp_cc) = g_cc.peak_in(0.5, 3.0).expect("C-C peak");
        assert!(gp_cc > 500.0, "a single C-C pair must spike g_CC: {gp_cc}");

        let g_hh = radial_distribution_between(&particles, &types, AtomType::Hydrogen, AtomType::Hydrogen, world, 4.0, 400);
        let mean = g_hh.bins.iter().sum::<f64>() / g_hh.bins.len() as f64;
        assert!(
            (0.9..=1.15).contains(&mean),
            "H-H must behave as an ideal gas (g≈1), got {mean}"
        );

        let g_ch = radial_distribution_between(&particles, &types, AtomType::Carbon, AtomType::Hydrogen, world, 4.0, 400);
        let peak_ch = g_ch.peak_in(0.5, 3.0).map(|(_, g)| g).unwrap_or(0.0);
        assert!(
            gp_cc / peak_ch > 100.0,
            "g_CC must dominate over the Poisson noise of g_CH ({} vs {})",
            gp_cc,
            peak_ch
        );
    }

    #[test]
    fn partial_g_r_empty_species_is_flat() {
        let world = Vec3::new(16.0, 16.0, 16.0);
        let particles = vec![
            Particle { index: 0, pos: Vec3::new(0.0, 0.0, 0.0), vel: Vec3::ZERO, mass: 1.0 },
            Particle { index: 1, pos: Vec3::new(2.0, 0.0, 0.0), vel: Vec3::ZERO, mass: 1.0 },
        ];
        let types = vec![AtomType::Hydrogen, AtomType::Hydrogen];
        let g = radial_distribution_between(&particles, &types, AtomType::Sodium, AtomType::Sodium, world, 4.0, 100);
        assert!(g.bins.iter().all(|&x| x == 0.0), "absent species → empty g(r)");
    }

    #[test]
    fn clusters_detect_dimers_and_monomers() {
        // Two H dimers at bond distance (2.2 < 1.5·σ_H = 2.4) and a pair of
        // isolated monomers.
        let world = Vec3::new(64.0, 64.0, 64.0);
        let data = [
            hydrogen_at(Vec3::new(10.0, 10.0, 10.0)),
            hydrogen_at(Vec3::new(12.0, 10.0, 10.0)),
            hydrogen_at(Vec3::new(30.0, 10.0, 10.0)),
            hydrogen_at(Vec3::new(32.0, 10.0, 10.0)),
            hydrogen_at(Vec3::new(50.0, 50.0, 50.0)),
            hydrogen_at(Vec3::new(5.0, 50.0, 50.0)),
        ];
        let types: Vec<AtomType> = data.iter().map(|(_, t)| *t).collect();
        let particles: Vec<Particle> = data.iter().map(|(p, _)| *p).collect();
        let s = clusters(&particles, &types, world, BOND_FACTOR);
        assert_eq!(s.aggregates, 2);
        assert_eq!(s.monomers, 2);
        assert_eq!(s.largest, 2);
        assert_eq!(s.bound_pairs, 2);
        assert_eq!(s.mean_size, 1.5);
    }

    #[test]
    fn clusters_empty_and_isolated() {
        let world = Vec3::new(64.0, 64.0, 64.0);
        let empty = clusters(&[], &[], world, BOND_FACTOR);
        assert_eq!(empty.largest, 0);
        assert_eq!(empty.mean_size, 0.0);

        let data = [
            hydrogen_at(Vec3::new(10.0, 10.0, 10.0)),
            hydrogen_at(Vec3::new(50.0, 50.0, 50.0)),
        ];
        let types: Vec<AtomType> = data.iter().map(|(_, t)| *t).collect();
        let particles: Vec<Particle> = data.iter().map(|(p, _)| *p).collect();
        let s = clusters(&particles, &types, world, BOND_FACTOR);
        assert_eq!(s.monomers, 2);
        assert_eq!(s.aggregates, 0);
        assert_eq!(s.bound_pairs, 0);
    }
}
