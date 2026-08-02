//! `Acceleration`: segunda derivada de la posición (fuerza neta sobre masa).

use crate::ecs::{Component, ComponentId};
use crate::math::Vec3;
use serde::{Deserialize, Serialize};

/// Aceleración neta de una entidad, recalculada cada tick por el sistema de
/// fuerzas. Es el estado intermedio del integrador de Verlet: la aceleración
/// del tick anterior se lee para el medio paso de velocidad, y el sistema de
/// fuerzas la reemplaza por la del tick actual.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Acceleration(pub Vec3);

impl Component for Acceleration {
    const ID: ComponentId = ComponentId(9);
}

impl From<Vec3> for Acceleration {
    fn from(v: Vec3) -> Self {
        Self(v)
    }
}
