//! `StatsSystem` — observación del universo.
//!
//! Se ejecuta **último** en el schedule y agrega una `StatsSnapshot` por tick.
//! Solo lee: la energía y la temperatura se **derivan** de las velocidades
//! (cinética + equipartición), del potencial acumulado por las fuerzas y del
//! contador de colisiones, en lugar de almacenarse como estado.

use crate::components::{Mass, Velocity};
use crate::config::Config;
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::{CollisionCounter, PotentialEnergy, StatsSnapshot};

pub struct StatsSystem;

impl System for StatsSystem {
    fn name(&self) -> &'static str {
        "stats"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Velocity>()
            .reads::<Mass>()
            .resource_read::<Config>()
            .resource_read::<CollisionCounter>()
            .resource_read::<PotentialEnergy>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        let cfg = ctx.resources.get::<Config>();
        let collisions = ctx
            .resources
            .get::<CollisionCounter>()
            .map(|c| c.0)
            .unwrap_or(0);
        let energy_potential = ctx
            .resources
            .get::<PotentialEnergy>()
            .map(|pe| pe.0)
            .unwrap_or(0.0);

        let mut kinetic = 0.0;
        let mut speed_sum = 0.0;
        let mut count = 0usize;
        ctx.world.for_each2::<Velocity, Mass>(|_, v, m| {
            kinetic += 0.5 * m.0 * v.0.length_squared();
            speed_sum += v.0.length();
            count += 1;
        });

        // Equipartición: con 3 grados traslacionales, T = (2/3)·⟨K⟩/k.
        let k_eff = cfg.map(|c| c.physics.thermal_constant).unwrap_or(0.0);
        let energy_avg = if count > 0 {
            kinetic / count as f64
        } else {
            0.0
        };
        let temperature_avg = if k_eff > 0.0 {
            (2.0 / 3.0) * energy_avg / k_eff
        } else {
            0.0
        };

        let volume = cfg.map(|c| c.universe.size.x * c.universe.size.y * c.universe.size.z);
        let density = match volume {
            Some(v) if v > 0.0 => ctx.world.len() as f64 / v,
            _ => 0.0,
        };

        let snapshot = StatsSnapshot {
            tick: ctx.time.tick,
            time: ctx.time.t,
            entities: ctx.world.len(),
            // Energía total: cinética + potencial (conservada por Verlet).
            energy_total: kinetic + energy_potential,
            energy_avg,
            energy_potential,
            temperature_avg,
            mean_speed: if count > 0 {
                speed_sum / count as f64
            } else {
                0.0
            },
            density,
            collisions,
            systems_run: ctx.stats.systems_run,
            fps: ctx.stats.fps,
            memory_bytes: ctx.world.memory_bytes(),
        };
        ctx.stats.record(snapshot);
    }
}
