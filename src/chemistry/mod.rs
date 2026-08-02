//! Química del universo (reservado).
//!
//! **El objetivo explícito del proyecto es que este módulo nunca necesite
//! existir como código específico.** Si la química emerge, será una
//! consecuencia de las leyes de `crate::physics`, no de funciones "crear
//! agua" o "crear enlace".
//!
//! Este módulo existe para dejar el espacio delimitado y documentar la regla.

/// Principios que deben respetar las leyes futuras.
pub mod principles {
    /// Una "reacción" nunca debe programarse como tal: debe ser la suma de
    /// interacciones de bajo nivel (enlace = energía local < umbral, etc.).
    pub const NO_HARDCODED_CHEMISTRY: &str =
        "está prohibido implementar especies químicas explícitamente";
}
