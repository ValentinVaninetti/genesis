//! Estadísticas de la simulación.
//!
//! Un `StatsCollector` agrega, cada tick, un `StatsSnapshot` y mantiene un
//! historial acotado. El muestreo lo hace un sistema (el último del schedule),
//! de modo que la recolección de métricas es parte del universo y no un efecto
//! lateral del bucle principal.

use serde::{Deserialize, Serialize};

/// Una fotografía de métricas en un instante del tiempo de simulación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsSnapshot {
    pub tick: u64,
    pub time: f64,
    pub entities: usize,
    /// Energía cinética total (derivada de `Velocity` y `Mass`).
    pub energy_total: f64,
    pub energy_avg: f64,
    /// Energía potencial total del tick (Lennard-Jones, acumulada por fuerzas).
    pub energy_potential: f64,
    /// Temperatura derivada por equipartición: `(2/3)·⟨K⟩/k`.
    pub temperature_avg: f64,
    /// Rapidez media de las partículas.
    pub mean_speed: f64,
    pub density: f64,
    pub collisions: u64,
    pub systems_run: u64,
    pub fps: f64,
    pub memory_bytes: usize,
}

/// Colector de métricas con historial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsCollector {
    pub snapshot: StatsSnapshot,
    /// Total de sistemas ejecutados desde el inicio.
    pub systems_run: u64,
    /// FPS reales medidos por el bucle principal (no es tiempo de simulación).
    pub fps: f64,
    history: Vec<StatsSnapshot>,
    history_cap: usize,
}

impl StatsCollector {
    pub fn new(history_cap: usize) -> Self {
        Self {
            snapshot: StatsSnapshot {
                tick: 0,
                time: 0.0,
                entities: 0,
                energy_total: 0.0,
                energy_avg: 0.0,
                energy_potential: 0.0,
                temperature_avg: 0.0,
                mean_speed: 0.0,
                density: 0.0,
                collisions: 0,
                systems_run: 0,
                fps: 0.0,
                memory_bytes: 0,
            },
            systems_run: 0,
            fps: 0.0,
            history: Vec::with_capacity(history_cap),
            history_cap,
        }
    }

    /// Registra una métrica nueva en el historial.
    pub fn record(&mut self, snapshot: StatsSnapshot) {
        if self.history.len() == self.history_cap {
            self.history.remove(0);
        }
        self.snapshot = snapshot.clone();
        self.history.push(snapshot);
    }

    /// Última métrica registrada.
    pub fn snapshot(&self) -> &StatsSnapshot {
        &self.snapshot
    }

    /// Métricas históricas (acotadas).
    pub fn history(&self) -> &[StatsSnapshot] {
        &self.history
    }
}

/// Recurso global: contador de colisiones.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CollisionCounter(pub u64);

/// Recurso global: energía potencial total del tick actual (Lennard-Jones),
/// acumulada por el sistema de fuerzas. No es estado persistente: se recalcula
/// por completo en cada tick.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PotentialEnergy(pub f64);

/// Histograma de rapideces (`|v|`) observado en un instante.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocityHistogram {
    /// Límite superior del rango.
    pub max_speed: f64,
    /// Ancho de cada bin.
    pub bin_width: f64,
    /// Conteos por bin.
    pub bins: Vec<u64>,
    /// Total de muestras.
    pub samples: u64,
    /// Muestras fuera de rango (`≥ max_speed`).
    pub overflow: u64,
}

/// Construye un histograma de rapidez a partir del `World`.
pub fn velocity_histogram(
    world: &crate::ecs::World,
    max_speed: f64,
    bins: usize,
) -> VelocityHistogram {
    let bins = bins.clamp(1, 512);
    let max_speed = max_speed.max(1e-9);
    let bin_width = max_speed / bins as f64;
    let mut counts = vec![0u64; bins];
    let mut samples = 0u64;
    let mut overflow = 0u64;
    world.for_each1::<crate::components::Velocity>(|_, v| {
        let s = v.0.length();
        samples += 1;
        if s >= max_speed {
            overflow += 1;
        } else {
            let i = (s / bin_width) as usize;
            counts[i.min(bins - 1)] += 1;
        }
    });
    VelocityHistogram {
        max_speed,
        bin_width,
        bins: counts,
        samples,
        overflow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historial_acotado() {
        let mut c = StatsCollector::new(3);
        for i in 0..10 {
            let mut s = c.snapshot.clone();
            s.tick = i;
            c.record(s);
        }
        assert_eq!(c.history().len(), 3);
        assert_eq!(c.snapshot.tick, 9);
    }
}
