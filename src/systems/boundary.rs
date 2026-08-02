//! `BoundarySystem` — architecture demo.
//!
//! Wraps positions inside the volume of the universe (periodic boundary
//! condition). Demonstrates reading a **resource** (`Config`) from a system.

use crate::config::Config;
use crate::scheduler::{Access, System, SystemContext};

pub struct BoundarySystem;

impl System for BoundarySystem {
    fn name(&self) -> &'static str {
        "boundary"
    }

    fn access(&self) -> Access {
        Access::default()
            .writes::<crate::components::Position>()
            .resource_read::<Config>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let Some(cfg) = ctx.resources.get::<Config>() else {
            return;
        };
        let half = cfg.universe.size.scale(0.5);

        ctx.world
            .par_for_each1_mut::<crate::components::Position>(|_e, pos| {
                let w = |v: f64, h: f64| v.rem_euclid(2.0 * h) - h;
                pos.x = w(pos.x, half.x);
                pos.y = w(pos.y, half.y);
                pos.z = w(pos.z, half.z);
            });
    }
}
