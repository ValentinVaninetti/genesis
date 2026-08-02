//! `CollisionSystem` — first real physical law: elastic collisions.
//!
//! Detection (broadphase): uniform spatial grid with minimum image over the
//! torus. Interaction: pure elastic impulse (`e = 1`) computed against the
//! pre-collision velocities. Energy is neither stored nor adjusted: it is a
//! consequence of the velocities, and the impulse conserves it.
//!
//! It is a **local**, decoupled law: it knows nothing about chemistry,
//! temperature or life. It only sees mass, position and velocity.

use crate::components::{Mass, Position, Velocity};
use crate::config::Config;
use crate::math::Vec3;
use crate::physics::collision::elastic_pair;
use crate::physics::grid::{Particle, SpatialGrid};
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::CollisionCounter;

/// Collision system. The spatial grid is reused between ticks (it is fully
/// rebuilt in each `run`), avoiding per-tick allocations.
pub struct CollisionSystem {
    grid: SpatialGrid,
}

impl CollisionSystem {
    pub fn new(cfg: &Config) -> Self {
        // Cells of at least the collision diameter: thus two particles in
        // contact can only live in the same cell or in neighbor cells.
        let min_cell = 2.0 * cfg.physics.particle_radius;
        Self {
            grid: SpatialGrid::new(cfg.universe.size, min_cell),
        }
    }
}

impl System for CollisionSystem {
    fn name(&self) -> &'static str {
        "collisions"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Position>()
            .reads::<Velocity>()
            .reads::<Mass>()
            .writes::<Velocity>()
            .resource_read::<Config>()
            .resource_write::<CollisionCounter>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let Some(cfg) = ctx.resources.get::<Config>() else {
            return;
        };
        let radius = cfg.physics.particle_radius;
        let capacity = ctx.world.entity_capacity();

        // Phase 1: collect the particles (position, velocity, mass).
        let mut particles: Vec<Particle> = Vec::with_capacity(ctx.world.len());
        ctx.world.for_each3::<Position, Velocity, Mass>(|e, pos, vel, mass| {
            particles.push(Particle {
                index: e.index(),
                pos: pos.0,
                vel: vel.0,
                mass: mass.0,
            });
        });
        if particles.is_empty() {
            return;
        }

        // Phase 2: broadphase — spatial grid + candidate pairs.
        self.grid.build(&particles);
        let mut pairs = Vec::new();
        self.grid.neighbors(&particles, 2.0 * radius, &mut pairs);

        // Phase 3: impulses of each pair against the pre-collision velocities.
        // Simultaneous accelerations accumulate (standard approximation of
        // molecular dynamics); momentum and energy are conserved per pair.
        let mut dv = vec![Vec3::ZERO; capacity];
        let mut collisions: u64 = 0;
        for &pair in &pairs {
            let a = &particles[pair.a];
            let b = &particles[pair.b];
            let (dva, dvb) = elastic_pair(a.mass, a.vel, b.mass, b.vel, pair.normal);
            if dva != Vec3::ZERO {
                dv[a.index as usize] += dva;
                dv[b.index as usize] += dvb;
                collisions += 1;
            }
        }

        // Phase 4: apply the impulses in parallel.
        if collisions > 0 {
            ctx.world.par_for_each1_mut::<Velocity>(|e, vel| {
                vel.0 += dv[e.index() as usize];
            });
        }

        if let Some(c) = ctx.resources.get_mut::<CollisionCounter>() {
            c.0 += collisions;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{AtomType, Charge};
    use crate::config::Config;
    use crate::ecs::World;
    use crate::universe::Universe;

    fn kinetic(w: &World) -> f64 {
        let mut e = 0.0;
        w.for_each2::<Velocity, Mass>(|_, v, m| e += 0.5 * m.0 * v.0.length_squared());
        e
    }

    fn momentum(w: &World) -> Vec3 {
        let mut p = Vec3::ZERO;
        w.for_each2::<Velocity, Mass>(|_, v, m| p += v.0 * m.0);
        p
    }

    fn spawn_atom(
        u: &mut Universe,
        pos: Vec3,
        vel: Vec3,
        mass: f64,
    ) -> crate::ecs::EntityId {
        let e = u.world.spawn();
        u.world.insert::<Position>(e, Position(pos));
        u.world.insert::<Velocity>(e, Velocity(vel));
        u.world.insert::<Mass>(e, Mass(mass));
        u.world.insert::<AtomType>(e, AtomType::Hydrogen);
        u.world.insert::<Charge>(e, Charge(0.0));
        e
    }

    #[test]
    fn two_atoms_swap_velocities() {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = 0;
        cfg.universe.size = Vec3::new(32.0, 32.0, 32.0);
        cfg.physics.particle_radius = 0.5;
        cfg.systems.enable_collisions = true;
        cfg.systems.enable_forces = false;

        let mut u = Universe::new(cfg);
        let left = spawn_atom(&mut u, Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), 1.0);
        let right = spawn_atom(&mut u, Vec3::new(1.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), 1.0);

        // After the collision, the velocities swap exactly.
        let mut swapped = false;
        for _ in 0..2000 {
            u.tick();
            let v_left = u.world.get::<Velocity>(left).unwrap().0;
            let v_right = u.world.get::<Velocity>(right).unwrap().0;
            if v_left.x < 0.0 && v_right.x > 0.0 {
                swapped = true;
                break;
            }
        }
        assert!(swapped, "the velocities were not swapped");
    }

    #[test]
    fn collisions_conserve_energy_and_momentum() {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = 300;
        cfg.universe.size = Vec3::new(32.0, 32.0, 32.0);
        cfg.physics.particle_radius = 0.4;
        cfg.physics.thermal_constant = 0.01;
        cfg.systems.enable_collisions = true;
        cfg.systems.enable_forces = false;

        let mut u = Universe::new(cfg);
        let e0 = kinetic(&u.world);
        let p0 = momentum(&u.world);
        assert!(e0 > 0.0);

        u.run_ticks(5000);

        let e1 = kinetic(&u.world);
        let p1 = momentum(&u.world);
        let rel = (e1 - e0).abs() / e0;
        let dp = (p0 - p1).length();
        assert!(rel < 1e-4, "relative energy drift: {rel:.3e}");
        assert!(dp < 1e-9 * p0.length().max(1.0), "momentum drift: {dp:.3e}");
        assert!(
            u.stats.snapshot.collisions > 0,
            "there were no collisions in the simulation"
        );
    }
}
