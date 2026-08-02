//! Definición del trait `System` y del contexto de ejecución.
//!
//! Un sistema es una **ley** o **transformación** que se ejecuta cada tick
//! sobre el `World`. Cada sistema declara explícitamente qué componentes lee
//! y qué escribe: es la información que permite al scheduler validar el orden,
//! detectar conflictos y (en el futuro) ejecutar etapas en paralelo.

use crate::ecs::{ComponentId, Resources, World};
use crate::rng::Rng;
use crate::stats::StatsCollector;
use crate::universe::Time;
use std::any::{Any, TypeId};

/// Acceso declarado por un sistema a componentes y recursos.
#[derive(Debug, Clone, Default)]
pub struct Access {
    /// Componentes leídos (no modificados).
    pub reads: Vec<ComponentId>,
    /// Componentes escritos.
    pub writes: Vec<ComponentId>,
    /// Recursos leídos.
    pub resources_read: Vec<TypeId>,
    /// Recursos escritos.
    pub resources_write: Vec<TypeId>,
}

impl Access {
    pub fn reads<C: crate::ecs::Component>(mut self) -> Self {
        self.reads.push(C::ID);
        self
    }

    pub fn writes<C: crate::ecs::Component>(mut self) -> Self {
        self.writes.push(C::ID);
        self
    }

    pub fn resource_read<R: Any + Send + Sync>(mut self) -> Self {
        self.resources_read.push(TypeId::of::<R>());
        self
    }

    pub fn resource_write<R: Any + Send + Sync>(mut self) -> Self {
        self.resources_write.push(TypeId::of::<R>());
        self
    }

    /// Dos accesos entran en conflicto si comparten un recurso o componente
    /// y al menos uno de los dos lo escribe.
    pub fn conflicts_with(&self, other: &Access) -> bool {
        let component_conflict = self
            .writes
            .iter()
            .any(|c| other.reads.contains(c) || other.writes.contains(c))
            || other
                .writes
                .iter()
                .any(|c| self.reads.contains(c) || self.writes.contains(c));
        let resource_conflict = self
            .resources_write
            .iter()
            .any(|c| other.resources_read.contains(c) || other.resources_write.contains(c))
            || other
                .resources_write
                .iter()
                .any(|c| self.resources_read.contains(c) || self.resources_write.contains(c));
        component_conflict || resource_conflict
    }
}

/// Contexto de ejecución de un sistema.
///
/// Da acceso a todo lo que una ley necesita sin acoplarse al `Universe`
/// completo: mundo, recursos, azar, tiempo y estadísticas.
pub struct SystemContext<'a> {
    pub world: &'a mut World,
    pub resources: &'a mut Resources,
    pub rng: &'a mut Rng,
    pub time: &'a Time,
    pub stats: &'a mut StatsCollector,
    /// Delta de tiempo de este tick.
    pub dt: f64,
}

/// Una ley del universo. Debe ser determinista salvo por `ctx.rng`.
pub trait System: Send + Sync {
    /// Nombre corto y estable (para logs y debug).
    fn name(&self) -> &'static str;

    /// Declaración de acceso (por defecto: sin acceso declarado).
    fn access(&self) -> Access {
        Access::default()
    }

    /// Ejecuta la ley.
    fn run(&mut self, ctx: &mut SystemContext<'_>);
}
