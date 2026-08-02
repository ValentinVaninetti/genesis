//! `ForceSystem` — fuerzas intermoleculares de Lennard-Jones.
//!
//! Es la **única** ley que crea interacción entre partículas más allá del
//! impulso de colisión. Calcula, con el mismo grid espacial que las
//! colisiones, la fuerza LJ de cada par dentro del cutoff y la acumula como
//! aceleración en el componente `Acceleration` (que el integrador de Verlet
//! consume). También acumula la energía potencial total en el recurso
//! `PotentialEnergy` para que las estadísticas reporten `E = K + V`.
//!
//! No sabe nada de especies, enlaces ni reacciones: solo masa, posición y tipo
//! atómico (que aporta σ y ε). Toda la "química" emerge de aquí.

use crate::components::{Acceleration, AtomType, Mass, Position};
use crate::config::Config;
use crate::math::Vec3;
use crate::physics::forces::{LjTable, LJ_CUTOFF_FACTOR};
use crate::physics::grid::{min_image, Particle, SpatialGrid};
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::PotentialEnergy;

fn element_index(t: AtomType) -> usize {
    match t {
        AtomType::Hydrogen => 0,
        AtomType::Helium => 1,
        AtomType::Carbon => 2,
        AtomType::Nitrogen => 3,
        AtomType::Oxygen => 4,
        AtomType::Sodium => 5,
    }
}

/// Sistema de fuerzas. El grid se reutiliza entre ticks y se reconstruye por
/// completo en cada `run` (mismo patrón que `CollisionSystem`).
pub struct ForceSystem {
    grid: SpatialGrid,
    lj: LjTable,
    rc: f64,
}

impl ForceSystem {
    pub fn new(cfg: &Config) -> Self {
        let lj = LjTable::new(cfg.physics.thermal_constant, LJ_CUTOFF_FACTOR);
        let rc = lj.rc();
        // Celdas de al menos el cutoff: dos partículas que interactúan solo
        // pueden vivir en la misma celda o en celdas vecinas.
        let grid = SpatialGrid::new(cfg.universe.size, rc);
        Self { grid, lj, rc }
    }
}

impl System for ForceSystem {
    fn name(&self) -> &'static str {
        "forces"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Position>()
            .reads::<Mass>()
            .reads::<AtomType>()
            .writes::<Acceleration>()
            .resource_read::<Config>()
            .resource_write::<PotentialEnergy>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let Some(cfg) = ctx.resources.get::<Config>() else {
            return;
        };
        let world_size = cfg.universe.size;
        let rc2 = self.rc * self.rc;
        let capacity = ctx.world.entity_capacity();

        // Fase 1: recolectar las partículas (posición, masa, elemento).
        let mut particles: Vec<Particle> = Vec::with_capacity(ctx.world.len());
        let mut types: Vec<u32> = Vec::with_capacity(ctx.world.len());
        ctx.world.for_each3::<Position, Mass, AtomType>(|e, pos, mass, at| {
            particles.push(Particle {
                index: e.index(),
                pos: pos.0,
                vel: Vec3::ZERO,
                mass: mass.0,
            });
            types.push(element_index(*at) as u32);
        });
        if particles.is_empty() {
            return;
        }

        // Fase 2: broadphase — pares dentro del cutoff.
        self.grid.build(&particles);
        let mut pairs = Vec::new();
        self.grid.neighbors(&particles, self.rc, &mut pairs);

        // Fase 3: acumular fuerzas (a = F/m) y energía potencial por par.
        // La normal del par apunta de `b` hacia `a`; la fuerza sobre `a` va a
        // lo largo de ella, y sobre `b` es exactamente la opuesta (3ª ley de
        // Newton), por lo que el momento total se conserva.
        let mut acc = vec![Vec3::ZERO; capacity];
        let mut potential = 0.0;
        for &pair in &pairs {
            let a = &particles[pair.a];
            let b = &particles[pair.b];
            let delta = min_image(a.pos - b.pos, world_size);
            let d2 = delta.length_squared();
            if d2 >= rc2 {
                continue;
            }
            let d = d2.sqrt();
            if d <= f64::EPSILON {
                continue;
            }
            let normal = delta * (1.0 / d);
            let p = self.lj.pair_indexed(types[pair.a] as usize, types[pair.b] as usize);
            let (f, v) = self.lj.force_switched(p, d, normal);
            acc[pair.a] += f / a.mass;
            acc[pair.b] -= f / b.mass;
            potential += v;
        }

        // Fase 4: aplicar las aceleraciones en paralelo.
        ctx.world.par_for_each1_mut::<Acceleration>(|e, a| {
            a.0 = acc[e.index() as usize];
        });

        if let Some(pe) = ctx.resources.get_mut::<PotentialEnergy>() {
            pe.0 = potential;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::universe::Universe;

    fn momentum(w: &crate::ecs::World) -> Vec3 {
        let mut p = Vec3::ZERO;
        w.for_each2::<crate::components::Velocity, Mass>(|_, v, m| p += v.0 * m.0);
        p
    }

    #[test]
    fn energia_y_momento_se_conservan_con_fuerzas() {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = 512; // 8³: red cúbica exacta
        cfg.universe.size = Vec3::new(24.0, 24.0, 24.0);
        cfg.physics.initial_temperature = 100.0;
        cfg.physics.thermal_constant = 0.01;
        cfg.systems.enable_forces = true;
        cfg.systems.enable_collisions = false;

        let mut u = Universe::new(cfg);
        let p0 = momentum(&u.world);
        let p0n = p0.length();

        // Relajación inicial: la red fría se reorganiza; la energía total ya se
        // conserva desde el primer tick (Verlet + fuerzas internas).
        u.run_ticks(300);

        let e_ref = u.stats.snapshot.energy_total;
        assert!(e_ref > 0.0, "energía total no positiva: {e_ref}");
        assert!(
            u.stats.snapshot.energy_potential < 0.0,
            "esperada atracción neta (V < 0), se obtuvo {}",
            u.stats.snapshot.energy_potential
        );

        u.run_ticks(2000);

        let e1 = u.stats.snapshot.energy_total;
        let rel = (e1 - e_ref).abs() / e_ref.abs();
        let dp = (momentum(&u.world) - p0).length();
        assert!(rel < 1e-3, "deriva relativa de energía: {rel:.3e}");
        assert!(dp < 1e-6 * p0n.max(1.0), "deriva de momento: {dp:.3e}");
    }
}
