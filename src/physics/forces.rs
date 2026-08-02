//! Potencial de Lennard-Jones: la interacción intermolecular fundamental.
//!
//! Es la única "química" del universo. Cada elemento aporta un `σ` (alcance)
//! y una `ε` (profundidad del pozo de potencial, en kelvin). Para un par de
//! tipos distintos se usan las reglas de mezcla de **Lorentz–Berthelot**:
//!
//! ```text
//! σ_ij = (σ_i + σ_j) / 2        ε_ij = √(ε_i · ε_j)
//! ```
//!
//! De este único potencial emergen la repulsión de corto alcance, la
//! atracción de Van der Waals y, con ella, la condensación, los agregados y
//! cualquier estructura que aparezca. Nada se programa como "molécula".
//!
//! ## Unidades
//!
//! `σ` está en unidades de simulación (la misma escala que posición y masa);
//! `ε` se expresa en kelvin y se convierte a energía de simulación con la
//! constante térmica de la configuración (`k`). Así, `k·T/ε = T/ε_k`: la
//! temperatura crítica de un elemento es ≈ `1.31·ε_k`.

use crate::components::AtomType;
use crate::math::Vec3;

/// Parámetros de Lennard-Jones de un elemento.
#[derive(Debug, Clone, Copy)]
pub struct LjElement {
    /// Alcance del potencial (unidades de simulación).
    pub sigma: f64,
    /// Profundidad del pozo en kelvin.
    pub epsilon_k: f64,
}

/// Tabla de elementos: `σ` en unidades de simulación, `ε` en kelvin.
/// Valores del orden de los reales para gases simples (relativos entre sí).
const ELEMENTS: [LjElement; 6] = [
    LjElement { sigma: 1.6, epsilon_k: 12.0 }, // H
    LjElement { sigma: 1.4, epsilon_k: 8.0 },  // He
    LjElement { sigma: 1.9, epsilon_k: 80.0 }, // C
    LjElement { sigma: 1.7, epsilon_k: 40.0 }, // N
    LjElement { sigma: 1.65, epsilon_k: 55.0 }, // O
    LjElement { sigma: 2.0, epsilon_k: 110.0 }, // Na
];

/// Distancia de corte en múltiplos de `σ`: más allá no hay interacción.
pub const LJ_CUTOFF_FACTOR: f64 = 2.5;

/// Núcleo endurecido: por debajo de `r_min = NUCLEUS · σ` el potencial se
/// evalúa en `r_min` (evita la divergencia `r→0` con `dt` finito).
pub const LJ_NUCLEUS: f64 = 0.5;

fn element_index(t: AtomType) -> usize {
    match t {
        AtomType::Hydrogen => 0,
        AtomType::Helium => 1,
        AtomType::Carbon => 2,
        AtomType::Nitrogen => 3,
        AtomType::Oxygen => 4,
        AtomType::Sodium => 5,
    }
}

/// Parámetros de un par de tipos (`ε` ya en unidades de energía).
#[derive(Debug, Clone, Copy)]
pub struct LjPair {
    pub sigma: f64,
    pub epsilon: f64,
}

/// Tabla de pares (6×6, simétrica) con el cutoff del sistema.
pub struct LjTable {
    pair: [[LjPair; 6]; 6],
    rc: f64,
    r_on: f64,
}

impl LjTable {
    /// Construye la tabla con las reglas de mezcla y prepara el switch suave
    /// entre `r_on` y `rc`.
    pub fn new(thermal_constant: f64, cutoff_factor: f64) -> Self {
        let mut max_sigma = 0.0f64;
        let mut pair = [[LjPair { sigma: 1.0, epsilon: 0.0 }; 6]; 6];
        for i in 0..6 {
            for j in 0..6 {
                let a = ELEMENTS[i];
                let b = ELEMENTS[j];
                let sigma = 0.5 * (a.sigma + b.sigma);
                let epsilon = thermal_constant * (a.epsilon_k * b.epsilon_k).sqrt();
                max_sigma = max_sigma.max(sigma);
                pair[i][j] = LjPair { sigma, epsilon };
            }
        }
        let rc = cutoff_factor.max(1.0) * max_sigma;
        let r_on = 0.9 * rc;
        Self { pair, rc, r_on }
    }

    /// Distancia de corte del sistema.
    pub fn rc(&self) -> f64 {
        self.rc
    }

    /// Parámetros del par `(a, b)`.
    pub fn pair(&self, a: AtomType, b: AtomType) -> LjPair {
        self.pair[element_index(a)][element_index(b)]
    }

    /// Parámetros del par por índice de elemento.
    pub fn pair_indexed(&self, i: usize, j: usize) -> LjPair {
        self.pair[i][j]
    }

    /// Fuerza (sobre `a`, a lo largo de `normal` que apunta de `b` hacia `a`)
    /// y contribución al potencial, con el potencial truncado y *switcheado*
    /// para que tanto la energía como la fuerza tiendan suavemente a 0 en `rc`.
    ///
    /// `r` debe ser `< rc`; por debajo de `LJ_NUCLEUS·σ` se evalúa en ese
    /// punto (núcleo endurecido).
    #[inline]
    pub fn force_switched(&self, p: LjPair, r: f64, normal: Vec3) -> (Vec3, f64) {
        let r = r.max(LJ_NUCLEUS * p.sigma);
        let s = p.sigma / r;
        let s6 = s * s * s * s * s * s;
        let s12 = s6 * s6;

        // Potencial LJ (no desplazado) y magnitud base de la fuerza a lo largo
        // de `normal` (= −dV/dr).
        let v = 4.0 * p.epsilon * (s12 - s6);
        let m = (24.0 * p.epsilon / p.sigma) * s * (2.0 * s12 - s6);

        let (sw, dsw) = self.switch(r);
        // F_ef = −d(V·sw)/dr = m·sw − V·(dsw/dr)
        let f_mag = m * sw - v * dsw;
        (normal * f_mag, v * sw)
    }

    /// Factor de suavizado `[0,1]` y su derivada respecto a `r`.
    ///
    /// Polinomio quíntico (función suave estándar de dinámica molecular):
    /// vale 1 hasta `r_on` y cae a 0 en `rc` con primera y segunda derivada
    /// nulas en ambos extremos.
    #[inline]
    fn switch(&self, r: f64) -> (f64, f64) {
        if r <= self.r_on {
            return (1.0, 0.0);
        }
        let u = (r - self.r_on) / (self.rc - self.r_on);
        let u2 = u * u;
        let u3 = u2 * u;
        let u4 = u3 * u;
        let u5 = u4 * u;
        let sw = 1.0 - 10.0 * u3 + 15.0 * u4 - 6.0 * u5;
        let dsdu = -30.0 * u2 * (1.0 - u) * (1.0 - u);
        (sw, dsdu / (self.rc - self.r_on))
    }
}

/// Fuerza y potencial LJ **sin** switch, para tests de la forma del potencial.
#[inline]
pub fn lj_raw(p: LjPair, r: f64) -> (f64, f64) {
    let s = p.sigma / r;
    let s6 = s * s * s * s * s * s;
    let s12 = s6 * s6;
    let v = 4.0 * p.epsilon * (s12 - s6);
    let m = (24.0 * p.epsilon / p.sigma) * s * (2.0 * s12 - s6);
    (m, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> LjTable {
        LjTable::new(0.01, LJ_CUTOFF_FACTOR)
    }

    #[test]
    fn pozo_de_potencial_en_sigma_por_raiz_sexta_de_2() {
        // El mínimo de V(r) = 4ε[(σ/r)¹² − (σ/r)⁶] está en r = σ·2^(1/6),
        // con V = −ε.
        let t = table();
        let p = t.pair(AtomType::Hydrogen, AtomType::Hydrogen);
        let r_min = p.sigma * 2.0f64.powf(1.0 / 6.0);
        let (_, v) = lj_raw(p, r_min);
        assert!((v + p.epsilon).abs() < 1e-12 * p.epsilon.max(1.0));
        let (m, _) = lj_raw(p, r_min);
        assert!(m.abs() < 1e-9, "la fuerza en el mínimo debe anularse");
    }

    #[test]
    fn fuerza_repulsiva_y_atractiva() {
        // normal = +x (de b hacia a). Dentro de σ la fuerza empuja (repulsión);
        // fuera atrae (m < 0 a lo largo de +x).
        let t = table();
        let p = t.pair(AtomType::Carbon, AtomType::Carbon);
        let (m_rep, _) = lj_raw(p, 0.8 * p.sigma);
        assert!(m_rep > 0.0, "esperada repulsión dentro de σ");
        let (m_att, _) = lj_raw(p, 1.5 * p.sigma);
        assert!(m_att < 0.0, "esperada atracción fuera de σ");
        // El cruce por cero coincide con el mínimo del pozo.
        let (m0, _) = lj_raw(p, p.sigma * 2.0f64.powf(1.0 / 6.0));
        assert!(m0.abs() < 1e-9);
    }

    #[test]
    fn reglas_de_mezcla_simetricas() {
        let t = table();
        let h = t.pair(AtomType::Hydrogen, AtomType::Carbon);
        let c = t.pair(AtomType::Carbon, AtomType::Hydrogen);
        assert_eq!(h.sigma, c.sigma);
        assert_eq!(h.epsilon, c.epsilon);
        // ε_ij entre los extremos: √(ε_H·ε_C).
        let expected = 0.01 * (12.0f64 * 80.0).sqrt();
        assert!((h.epsilon - expected).abs() < 1e-12);
    }

    #[test]
    fn el_switch_se_anula_en_el_corte() {
        let t = table();
        let p = t.pair(AtomType::Oxygen, AtomType::Oxygen);
        let r = t.rc();
        assert!(r > p.sigma);
        let (f, v) = t.force_switched(p, r, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(f, Vec3::ZERO);
        assert_eq!(v, 0.0);
        // Dentro de r_on no hay suavizado.
        let r_in = 0.8 * t.r_on;
        let (f_in, _) = t.force_switched(p, r_in, Vec3::new(1.0, 0.0, 0.0));
        let (m, _) = lj_raw(p, r_in);
        assert!((f_in.x - m).abs() < 1e-12);
    }
}
