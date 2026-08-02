//! Integrador de **velocity Verlet** (split kick–drift–kick).
//!
//! Cada tick se ejecutan dos medios impulsos de velocidad alrededor del drift
//! de posición y del cálculo de fuerzas:
//!
//! ```text
//! v(t + dt/2) = v(t) + (dt/2)·a(t)          → VelocityHalfKick
//! x(t + dt)   = x(t) + dt·v(t + dt/2)       → PositionDrift
//! a(t + dt)   = fuerzas en x(t + dt)        → ForceSystem
//! v(t + dt)   = v(t + dt/2) + (dt/2)·a(t+dt) → VelocityHalfKick
//! ```
//!
//! Es un integrador **simpéctico**: conserva la energía de un sistema
//! conservativo con error acotado O(dt²) y no deriva con el tiempo. Es la
//! integración estándar de la dinámica molecular.

use crate::components::{Acceleration, Position, Velocity};
use crate::scheduler::{Access, System, SystemContext};

/// Medio impulso: `v += (dt/2)·a`.
///
/// Se registra **dos veces** en el schedule: la primera usa la aceleración del
/// tick anterior (leída antes de que `ForceSystem` la recalcule) y la segunda
/// la aceleración recién calculada.
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

/// Drift de posición: `x += v·dt` (con `v` ya en el medio paso).
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
