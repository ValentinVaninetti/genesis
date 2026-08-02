//! Análisis de estructura emergente.
//!
//! No son leyes: son **lentes**. Toda la física vive en `physics/`; aquí solo
//! se miden consecuencias de las leyes para observar si emerge estructura
//! (condensación, agregados, orden). Nada de lo que hay aquí alimenta la
//! simulación: es observación pura, con la misma filosofía que `stats`.

use crate::components::AtomType;
use crate::math::Vec3;
use crate::physics::forces::sigma;
use crate::physics::grid::{min_image, Particle, SpatialGrid};

/// Factor de "contacto" para el análisis de agregados: dos átomos pertenecen
/// al mismo cluster cuando `r < BOND_FACTOR · σ_ij`. `1.5·σ` está dentro del
/// pozo atractivo del potencial LJ (cuyo mínimo vive en `1.12·σ`).
pub const BOND_FACTOR: f64 = 1.5;

/// Función de distribución radial `g(r)` del sistema.
///
/// Normalizada para que `g(r) → 1` en un gas ideal: cada shell se divide por
/// el número esperado de vecinos de un gas homogéneo a la densidad media.
#[derive(Debug, Clone)]
pub struct RadialDistribution {
    /// Distancia máxima considerada (≤ mitad del lado más corto del toro).
    pub r_max: f64,
    /// Ancho de cada bin.
    pub dr: f64,
    /// `g(r)` por bin (bin `i` centrado en `(i + 0.5)·dr`).
    pub bins: Vec<f64>,
}

impl RadialDistribution {
    /// Máximo de `g(r)` en la ventana `[from, to]`: `(r, g)`. Con `to` por
    /// debajo del apilamiento del toro (≈ `L/2`), captura la capa de
    /// coordinación (primer vecino). Un pico alto y definido revela orden
    /// local (líquido/sólido); su ausencia, un gas.
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

/// Construye `g(r)` a partir de las partículas y el tamaño del mundo.
///
/// `r_max` se recorta a `min(size)/2`: en un toro las distancias de imagen
/// mínima solo son exactas hasta la mitad del lado más corto. El grid
/// espacial evita el doble loop O(n²).
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

    let n = particles.len();
    let mut bins = vec![0.0f64; nbins];
    if n < 2 {
        return RadialDistribution { r_max, dr, bins };
    }
    for (i, &c) in counts.iter().enumerate() {
        let r_lo = i as f64 * dr;
        let r_hi = r_lo + dr;
        let shell = 4.0 / 3.0 * std::f64::consts::PI * (r_hi * r_hi * r_hi - r_lo * r_lo * r_lo);
        // Pares no ordenados esperados en la shell de un gas homogéneo: el
        // grid reporta cada par una sola vez, así que N(N−1)/2.
        let expected = (n * (n - 1)) as f64 * 0.5 * shell / vol;
        bins[i] = c as f64 / expected.max(f64::MIN_POSITIVE);
    }
    RadialDistribution { r_max, dr, bins }
}

/// Resumen de agregados detectados con friends-of-friends.
#[derive(Debug, Clone, Default, Copy, PartialEq)]
pub struct ClusterStats {
    /// Clusters de un solo átomo.
    pub monomers: usize,
    /// Clusters de ≥ 2 átomos.
    pub aggregates: usize,
    /// Tamaño del agregado más grande.
    pub largest: usize,
    /// Tamaño medio sobre todos los clusters (incluidos los monómeros).
    pub mean_size: f64,
    /// Pares de átomos en contacto (aristas del grafo de agregación).
    pub bound_pairs: usize,
}

/// Detecta agregados: dos átomos pertenecen al mismo cluster si están a
/// `r < bond_factor · σ_ij` (σ_ij por mezcla de Lorentz). Union-find con
/// compresión de caminos sobre los pares del grid (que actúa como superset).
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

    // Sigma de cada partícula y sigma máximo (cutoff del grid: superset).
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

/// Unión por rango con compresión de caminos.
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

/// Raíz con compresión de caminos.
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
    fn g_r_de_un_par_pica_en_la_distancia() {
        // Un único par a distancia d → todo el peso en el bin de r≈d.
        let world = Vec3::new(32.0, 32.0, 32.0);
        let d = 1.7;
        let particles = vec![
            Particle { index: 0, pos: Vec3::new(0.0, 0.0, 0.0), vel: Vec3::ZERO, mass: 1.0 },
            Particle { index: 1, pos: Vec3::new(d, 0.0, 0.0), vel: Vec3::ZERO, mass: 1.0 },
        ];
        let g = radial_distribution(&particles, world, 4.0, 400);
        assert!((g.dr - 0.01).abs() < 1e-12);
        let (r_peak, g_peak) = g.peak_in(0.5, 3.0).expect("debe haber un pico");
        assert!((r_peak - d).abs() < 0.03, "pico en r={r_peak}, esperado ~{d}");
        assert!(g_peak > 1_000.0, "un par concentrado debe disparar g(r): {g_peak}");
    }

    #[test]
    fn g_r_de_gas_aleatorio_es_aproximadamente_uno() {
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
            "gas ideal → g≈1, se obtuvo {mean}"
        );
    }

    #[test]
    fn clusters_detecta_dimeros_y_monomeros() {
        // Dos dímeros de H a distancia de enlace (2.2 < 1.5·σ_H = 2.4) y un
        // par de monómeros aislados.
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
    fn clusters_vacio_y_aislados() {
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
