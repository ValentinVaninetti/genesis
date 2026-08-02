//! **Velocity Verlet** integrator (kick–drift–kick split).
//!
//! Every tick two half kicks of velocity run around the position drift and
//! the force computation:
//!
//! ```text
//! v(t + dt/2) = v(t) + (dt/2)·a(t)          → VelocityHalfKick
//! x(t + dt)   = x(t) + dt·v(t + dt/2)       → PositionDrift
//! a(t + dt)   = forces at x(t + dt)        → ForceSystem
//! v(t + dt)   = v(t + dt/2) + (dt/2)·a(t+dt) → VelocityHalfKick
//! ```
//!
//! It is a **symplectic** integrator: it conserves the energy of a
//! conservative system with bounded O(dt²) error and does not drift over
//! time. It is the standard integration of molecular dynamics.

use crate::components::{Acceleration, Position, Velocity};
use crate::scheduler::{Access, System, SystemContext};

/// Half kick: `v += (dt/2)·a`.
///
/// It is registered **twice** in the schedule: the first time it uses the
/// acceleration of the previous tick (read before `ForceSystem` recomputes
/// it) and the second time the freshly computed acceleration.
pub struct VelocityHalfKick;

impl System for VelocityHalfKick {
    fn name(&self) -> &'static str {
        "velocity_half_kick"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Acceleration>()
            .writes::<Velocity>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let half_dt = 0.5 * ctx.dt;
        ctx.world
            .par_for_each2_mut::<Velocity, Acceleration>(|_e, vel, acc| {
                vel.0 += acc.0 * half_dt;
            });
    }
}

/// Position drift: `x += v·dt` (with `v` already in the half step).
pub struct PositionDrift;

impl System for PositionDrift {
    fn name(&self) -> &'static str {
        "position_drift"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Velocity>()
            .writes::<Position>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let dt = ctx.dt;
        ctx.world
            .par_for_each2_mut::<Position, Velocity>(|_e, pos, vel| {
                pos.0 += vel.0 * dt;
            });
    }
}
