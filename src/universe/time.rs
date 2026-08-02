//! Clock of the universe.
//!
//! Distinguishes between *simulation time* (`t`) and *tick* (discrete step).
//! `dt` is fixed and lives in the configuration: unlike game engines, a
//! deterministic physical simulation does not need a variable dt.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Time {
    /// Elapsed simulation time.
    pub t: f64,
    /// Current tick number.
    pub tick: u64,
    /// Time delta per tick (constant).
    pub dt: f64,
}

impl Time {
    pub fn new(dt: f64) -> Self {
        Self { t: 0.0, tick: 0, dt }
    }

    /// Advances one tick.
    pub fn advance(&mut self) {
        self.t += self.dt;
        self.tick += 1;
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::new(1.0 / 60.0)
    }
}
