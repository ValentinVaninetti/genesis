//! Uniform spatial grid (broadphase) with periodic boundary conditions.
//!
//! Detects pairs of particles within collision distance without checking all
//! combinations: each particle is inserted into a cell and only adjacent
//! cells are compared (Manhattan-1 neighborhood). The universe is a torus:
//! distances use the **minimum image** and neighbor cells wrap around the
//! edges.
//!
//! Each candidate pair of particles is reported **exactly once**, even on
//! degenerate single-cell axes, thanks to the canonical direction.

use crate::math::Vec3;

/// Compact particle for the grid.
#[derive(Debug, Clone, Copy)]
pub struct Particle {
    /// Entity index (to apply the results in the `World`).
    pub index: u32,
    pub pos: Vec3,
    pub vel: Vec3,
    pub mass: f64,
}

/// Pair of candidate collision particles, with the unit normal (minimum
/// image) pointing from particle `b` towards `a`.
#[derive(Debug, Clone, Copy)]
pub struct Pair {
    pub a: usize,
    pub b: usize,
    pub normal: Vec3,
}

/// Empty cell / end of linked list.
const EMPTY: u32 = u32::MAX;

/// The 27 directions of the Manhattan-1 neighborhood.
const ALL_OFFSETS: [(i32, i32, i32); 27] = [
    (-1, -1, -1), (-1, -1, 0), (-1, -1, 1),
    (-1, 0, -1), (-1, 0, 0), (-1, 0, 1),
    (-1, 1, -1), (-1, 1, 0), (-1, 1, 1),
    (0, -1, -1), (0, -1, 0), (0, -1, 1),
    (0, 0, -1), (0, 0, 0), (0, 0, 1),
    (0, 1, -1), (0, 1, 0), (0, 1, 1),
    (1, -1, -1), (1, -1, 0), (1, -1, 1),
    (1, 0, -1), (1, 0, 0), (1, 0, 1),
    (1, 1, -1), (1, 1, 0), (1, 1, 1),
];

/// Uniform spatial index, rebuildable per tick.
pub struct SpatialGrid {
    dims: (u32, u32, u32),
    cell: Vec3,
    world_size: Vec3,
    /// Head of the linked list per cell.
    heads: Vec<u32>,
    /// Cells with particles in the current build (for partial reset).
    touched: Vec<u32>,
    /// Next particle per slot.
    next: Vec<u32>,
    chain: Vec<u32>,
    nchain: Vec<u32>,
}

impl SpatialGrid {
    /// Creates a grid covering `world_size` with cells of at least `min_cell`
    /// per axis (the real cells may be larger, never smaller).
    pub fn new(world_size: Vec3, min_cell: f64) -> Self {
        let ncell = |size: f64| ((size / min_cell).floor()).max(1.0) as u32;
        let (nx, ny, nz) = (ncell(world_size.x), ncell(world_size.y), ncell(world_size.z));
        let cell = Vec3::new(
            world_size.x / nx as f64,
            world_size.y / ny as f64,
            world_size.z / nz as f64,
        );
        let total = (nx as usize) * (ny as usize) * (nz as usize);
        Self {
            dims: (nx, ny, nz),
            cell,
            world_size,
            heads: vec![EMPTY; total],
            touched: Vec::new(),
            next: Vec::new(),
            chain: Vec::new(),
            nchain: Vec::new(),
        }
    }

    /// Cells per axis.
    pub fn dims(&self) -> (u32, u32, u32) {
        self.dims
    }

    /// Cells with particles in the last build.
    pub fn touched_cells(&self) -> usize {
        self.touched.len()
    }

    /// Rebuilds the spatial index for `particles`.
    pub fn build(&mut self, particles: &[Particle]) {
        for &ci in &self.touched {
            self.heads[ci as usize] = EMPTY;
        }
        self.touched.clear();
        self.next.resize(particles.len(), EMPTY);
        for (slot, p) in particles.iter().enumerate() {
            let ci = self.cell_index(self.cell_of(p.pos));
            if self.heads[ci] == EMPTY {
                self.touched.push(ci as u32);
            }
            self.next[slot] = self.heads[ci];
            self.heads[ci] = slot as u32;
        }
    }

    /// Detects the pairs of particles at distance smaller than `cutoff`.
    ///
    /// Two particles are candidates if their distance on the torus is `<
    /// cutoff`. Each pair is reported exactly once.
    pub fn neighbors(&mut self, particles: &[Particle], cutoff: f64, out: &mut Vec<Pair>) {
        let cd2 = cutoff * cutoff;
        out.clear();
        for &ci in &self.touched {
            Self::gather(&self.next, self.heads[ci as usize], &mut self.chain);
            let (cx, cy, cz) = self.decode(ci);

            // Pairs within the same cell.
            for i in 0..self.chain.len() {
                for j in (i + 1)..self.chain.len() {
                    self.check_pair(particles, self.chain[i], self.chain[j], cd2, out);
                }
            }

            // Pairs with neighbor cells (each pair, only once).
            for &(dx, dy, dz) in &ALL_OFFSETS {
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }
                let nc = self.neighbor(cx, cy, cz, dx, dy, dz);
                if nc == ci as usize || self.heads[nc] == EMPTY {
                    continue;
                }
                if (dx, dy, dz) != self.canonical_offset((dx, dy, dz)) {
                    continue;
                }
                Self::gather(&self.next, self.heads[nc], &mut self.nchain);
                for &sa in &self.chain {
                    for &sb in &self.nchain {
                        self.check_pair(particles, sa, sb, cd2, out);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    /// Walks the linked list from `head` dumping it into `buf`.
    fn gather(next: &[u32], head: u32, buf: &mut Vec<u32>) {
        buf.clear();
        let mut cur = head;
        while cur != EMPTY {
            buf.push(cur);
            cur = next[cur as usize];
        }
    }

    fn check_pair(
        &self,
        particles: &[Particle],
        sa: u32,
        sb: u32,
        cd2: f64,
        out: &mut Vec<Pair>,
    ) {
        let a = &particles[sa as usize];
        let b = &particles[sb as usize];
        let delta = min_image(a.pos - b.pos, self.world_size);
        let d2 = delta.length_squared();
        if d2 >= cd2 {
            return;
        }
        let d = d2.sqrt();
        if d <= f64::EPSILON {
            return; // exact overlap: undefined normal
        }
        out.push(Pair {
            a: sa as usize,
            b: sb as usize,
            normal: delta * (1.0 / d),
        });
    }

    /// Cell (ix, iy, iz) of a position, with periodic wrapping.
    fn cell_of(&self, pos: Vec3) -> (u32, u32, u32) {
        let axis = |v: f64, period: f64, c: f64, n: u32| {
            let u = (v + 0.5 * period).rem_euclid(period);
            ((u / c).floor() as u32).min(n - 1)
        };
        (
            axis(pos.x, self.world_size.x, self.cell.x, self.dims.0),
            axis(pos.y, self.world_size.y, self.cell.y, self.dims.1),
            axis(pos.z, self.world_size.z, self.cell.z, self.dims.2),
        )
    }

    fn cell_index(&self, (ix, iy, iz): (u32, u32, u32)) -> usize {
        self.encode(ix, iy, iz)
    }

    fn encode(&self, ix: u32, iy: u32, iz: u32) -> usize {
        (ix + self.dims.0 * (iy + self.dims.1 * iz)) as usize
    }

    fn decode(&self, idx: u32) -> (u32, u32, u32) {
        let (nx, ny, _) = self.dims;
        let nxny = nx * ny;
        let iz = idx / nxny;
        let rem = idx % nxny;
        let iy = rem / nx;
        let ix = rem % nx;
        (ix, iy, iz)
    }

    fn neighbor(&self, cx: u32, cy: u32, cz: u32, dx: i32, dy: i32, dz: i32) -> usize {
        let (nx, ny, nz) = self.dims;
        let ix = (cx as i64 + dx as i64).rem_euclid(nx as i64) as u32;
        let iy = (cy as i64 + dy as i64).rem_euclid(ny as i64) as u32;
        let iz = (cz as i64 + dz as i64).rem_euclid(nz as i64) as u32;
        self.encode(ix, iy, iz)
    }

    /// Canonical version of an offset: degenerate axes (a single cell) are
    /// reduced to 0 and the direction is normalized with the first non-zero
    /// component positive.
    ///
    /// Each distinct cell pair is processed exactly once: for the pair
    /// `{A, B}` there is a single offset going from `A` to `B`, and the grid
    /// only uses it when it matches this canonical form. In degenerate tori
    /// several offsets wrap to the same cell; the canonical one removes the
    /// redundant ones.
    fn canonical_offset(&self, o: (i32, i32, i32)) -> (i32, i32, i32) {
        let dims = [self.dims.0, self.dims.1, self.dims.2];
        let mut d = [o.0, o.1, o.2];
        for k in 0..3 {
            if dims[k] <= 1 {
                d[k] = 0;
            }
        }
        let mut first = 0;
        for v in d {
            if v != 0 {
                first = v;
                break;
            }
        }
        match first {
            1 => (d[0], d[1], d[2]),
            -1 => (-d[0], -d[1], -d[2]),
            _ => (0, 0, 0),
        }
    }
}

/// Minimum image: the difference between two positions, mapped to the range
/// `[-size/2, size/2)` per axis (periodic torus of period `size`).
pub fn min_image(delta: Vec3, size: Vec3) -> Vec3 {
    Vec3::new(
        delta.x - size.x * (delta.x / size.x).round(),
        delta.y - size.y * (delta.y / size.y).round(),
        delta.z - size.z * (delta.z / size.z).round(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    fn brute_pairs(particles: &[Particle], world_size: Vec3, radius: f64) -> Vec<(u32, u32)> {
        let cd2 = (2.0 * radius) * (2.0 * radius);
        let mut out = Vec::new();
        for i in 0..particles.len() {
            for j in (i + 1)..particles.len() {
                let d2 = min_image(particles[i].pos - particles[j].pos, world_size).length_squared();
                if d2 < cd2 {
                    out.push((i as u32, j as u32));
                }
            }
        }
        out.sort_unstable();
        out
    }

    fn grid_pairs(particles: &[Particle], world_size: Vec3, radius: f64) -> Vec<(u32, u32)> {
        let mut grid = SpatialGrid::new(world_size, 2.0 * radius);
        grid.build(particles);
        let mut pairs = Vec::new();
        grid.neighbors(particles, 2.0 * radius, &mut pairs);
        let mut out: Vec<(u32, u32)> = pairs
            .iter()
            .map(|p| (p.a.min(p.b) as u32, p.a.max(p.b) as u32))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn grid_matches_brute_force() {
        let world_size = Vec3::new(64.0, 64.0, 64.0);
        let radius = 1.0;
        let mut rng = Rng::new(1234);
        let particles: Vec<Particle> = (0..400)
            .map(|i| Particle {
                index: i,
                pos: rng.in_box(world_size.scale(0.5)),
                vel: Vec3::ZERO,
                mass: 1.0,
            })
            .collect();
        assert_eq!(grid_pairs(&particles, world_size, radius), brute_pairs(&particles, world_size, radius));
    }

    #[test]
    fn degenerate_torus_does_not_duplicate_or_lose() {
        // Thin z forces nz = 1: the wraps in z fall into the same cell and the
        // distances cross the periodic border.
        let world_size = Vec3::new(16.0, 16.0, 2.0);
        let radius = 0.6;
        assert_eq!(SpatialGrid::new(world_size, 2.0 * radius).dims().2, 1);

        let mut rng = Rng::new(99);
        let particles: Vec<Particle> = (0..300)
            .map(|i| Particle {
                index: i,
                pos: rng.in_box(world_size.scale(0.5)),
                vel: Vec3::ZERO,
                mass: 1.0,
            })
            .collect();
        assert_eq!(grid_pairs(&particles, world_size, radius), brute_pairs(&particles, world_size, radius));
    }

    #[test]
    fn single_cell_universe() {
        // Min_cell larger than the world: everything falls into a single cell.
        let world_size = Vec3::new(3.0, 3.0, 3.0);
        let radius = 5.0;
        let particles: Vec<Particle> = vec![
            Particle { index: 0, pos: Vec3::new(0.0, 0.0, 0.0), vel: Vec3::ZERO, mass: 1.0 },
            Particle { index: 1, pos: Vec3::new(1.0, 0.0, 0.0), vel: Vec3::ZERO, mass: 1.0 },
            Particle { index: 2, pos: Vec3::new(-0.5, 1.5, 0.0), vel: Vec3::ZERO, mass: 1.0 },
        ];
        assert_eq!(grid_pairs(&particles, world_size, radius), brute_pairs(&particles, world_size, radius));
    }

    #[test]
    fn minimum_image_normal_is_correct() {
        // Two particles almost opposite across the periodic border:
        // a ≡ +33 (by wrapping) and b = +31.5, at a distance of 1.5.
        let size = Vec3::new(64.0, 64.0, 64.0);
        let a = Vec3::new(-31.0, 0.0, 0.0);
        let b = Vec3::new(31.5, 0.0, 0.0);
        let delta = min_image(a - b, size);
        assert!((delta.x - 1.5).abs() < 1e-9);
        let d2 = delta.length_squared();
        assert!((d2 - 2.25).abs() < 1e-9);
    }
}
