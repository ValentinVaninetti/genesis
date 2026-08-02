//! # GENESIS
//!
//! Motor de simulación de un universo cuyas leyes son pocas, simples y
//! generales. El objetivo **no** es simular vida ni química: es programar
//! únicamente las reglas fundamentales y observar si la complejidad emerge
//! espontáneamente.
//!
//! Módulos principales:
//! - [`ecs`]: ECS propio, orientado a datos, basado en arquetipos.
//! - [`universe`]: fachada (`Universe`), reloj y recursos globales.
//! - [`scheduler`]: orden explícito de los sistemas + análisis de etapas.
//! - [`components`]: catálogo de componentes (posición, velocidad, energía…).
//! - [`systems`]: leyes (por ahora, demos de arquitectura).
//! - [`config`]: toda la física en TOML, fuera del código.
//! - [`serialization`]: guardar y retomar universos completos.
//! - [`stats`]: métricas por tick con historial.
//! - [`math`]: `Vec3` y primitivas geométricas.
//! - [`physics`] / [`chemistry`]: reservados para leyes futuras.
//!
//! # Modelo mental
//!
//! Todo lo que existe es una **entidad** (inicialmente solo átomos). Las
//! entidades solo tienen **datos** (`Component`). Las **leyes** son `System`s
//! independientes, ordenadas explícitamente por el scheduler. Nada de química,
//! nada de evolución: solo reglas. Si algo parecido a la vida aparece, será
//! una consecuencia, nunca una característica.

pub mod chemistry;
pub mod components;
pub mod config;
pub mod ecs;
pub mod math;
pub mod physics;
pub mod rng;
pub mod scheduler;
pub mod serialization;
pub mod stats;
pub mod systems;
pub mod universe;
