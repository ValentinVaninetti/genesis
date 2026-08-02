//! `ThermostatSystem` — coupling to a thermal bath (velocity rescaling).
//!
//! An NVE universe (forces without thermostat) conserves energy, so a cold
//! gas **self-heats** during condensation and cannot sit at a chosen
//! temperature. This system is an **instrument**, not a law: it only rescales
//! velocities to drive the equipartition temperature toward
//! `physics.thermostat_temperature` (Berendsen weak coupling). It is opt-in
//! via `[systems].enable_thermostat` and does not feed any physical law.

use crate::components::{Mass, Velocity};
use crate::config::Config;
use crate::scheduler::{Access, System, SystemContext};

/// Berendsen thermostat.
///
/// Rescales every velocity by `λ` with `λ² = 1 + (1/τ)·(T_target/T − 1)`,
/// where `τ` is the relaxation time in ticks. This drives `dT/dt = (T_target − T)/τ`
/// without violently interrupting the dynamics.
pub struct ThermostatSystem {
    /// Relaxation time in ticks.
    tau: f64,
    /// Target equipartition temperature (kelvin).
    target: f64,
    /// Thermal constant (equipartition scale).
    k: f64,
}

impl ThermostatSystem {
    pub fn new(cfg: &Config) -> Self {
        Self {
            tau: cfg.physics.thermostat_tau.max(1.0),
            target: cfg.physics.thermostat_temperature,
            k: cfg.physics.thermal_constant,
        }
    }
}

impl System for ThermostatSystem {
    fn name(&self) -> &'static str {
        "thermostat"
    }

    fn access(&self) -> Access {
        Access::default().reads::<Mass>().writes::<Velocity>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        if self.target <= 0.0 {
            return;
        }
        // Current kinetic energy and particle count.
        let mut kinetic = 0.0;
        let mut count = 0usize;
        ctx.world.for_each2::<Velocity, Mass>(|_, v, m| {
            kinetic += 0.5 * m.0 * v.0.length_squared();
            count += 1;
        });
        if count == 0 {
            return;
        }
        // Equipartition temperature (same formula as StatsSystem).
        let t = (2.0 / 3.0) * (kinetic / count as f64) / self.k;
        if !t.is_finite() || t <= 0.0 {
            return;
        }
        let lambda2 = 1.0 + (1.0 / self.tau) * (self.target / t - 1.0);
        let lambda = lambda2.max(0.0).sqrt();
        ctx.world.par_for_each1_mut::<Velocity>(|_, v| {
            v.0 *= lambda;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::universe::Universe;

    fn hot_config(target: f64) -> Config {
        let mut cfg = Config::default_config();
        cfg.universe.initial_atoms = 512; // 8³ exact cubic lattice
        cfg.universe.size = Vec3::new(24.0, 24.0, 24.0);
        cfg.physics.initial_temperature = 300.0; // hot start
        cfg.physics.thermal_constant = 0.01;
        cfg.physics.thermostat_temperature = target;
        cfg.physics.thermostat_tau = 10.0;
        cfg.systems.enable_forces = true;
        cfg.systems.enable_collisions = false;
        cfg.systems.enable_thermostat = true;
        cfg
    }

    #[test]
    fn cools_a_hot_system_toward_target() {
        let mut u = Universe::new(hot_config(60.0));
        u.run_ticks(2500);
        let t = u.stats.snapshot.temperature_avg;
        assert!(
            (t - 60.0).abs() < 10.0,
            "thermostat should cool toward 60 K, got {t}"
        );
    }

    #[test]
    fn heats_a_cold_system_toward_target() {
        let mut cfg = hot_config(180.0);
        cfg.physics.initial_temperature = 20.0;
        let mut u = Universe::new(cfg);
        u.run_ticks(2500);
        let t = u.stats.snapshot.temperature_avg;
        assert!(
            (t - 180.0).abs() < 10.0,
            "thermostat should heat toward 180 K, got {t}"
        );
    }
}
