//! `MovementSystem` — Euler integration of position.
//!
//! Reads `Position` and `Velocity` and advances `x += v·dt`. It is used as a
//! **fallback** when forces are disabled (`enable_forces = false`): with
//! forces enabled the engine uses velocity Verlet ([`super::integrate`]).

use crate::scheduler::{Access, System, SystemContext};

pub struct MovementSystem;

impl System for MovementSystem {
    fn name(&self) -> &'static str {
        "movement"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<crate::components::Velocity>()
            .writes::<crate::components::Position>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let dt = ctx.dt;
        ctx.world
            .par_for_each2_mut::<crate::components::Position, crate::components::Velocity>(
                |_e, pos, vel| {
                    pos.x += vel.x * dt;
                    pos.y += vel.y * dt;
                    pos.z += vel.z * dt;
                },
            );
    }
}
