//! Reloj del universo.
//!
//! Distingue entre *tiempo de simulación* (`t`) y *tick* (paso discreto).
//! El `dt` es fijo y vive en la configuración: a diferencia de los motores de
//! juego, una simulación física determinista no necesita dt variable.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Time {
    /// Tiempo de simulación transcurrido.
    pub t: f64,
    /// Número de tick actual.
    pub tick: u64,
    /// Delta de tiempo por tick (constante).
    pub dt: f64,
}

impl Time {
    pub fn new(dt: f64) -> Self {
        Self { t: 0.0, tick: 0, dt }
    }

    /// Avanza un tick.
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
