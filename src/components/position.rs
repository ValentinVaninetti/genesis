//! `Position`: ubicación de una entidad en el espacio continuo.

use crate::ecs::{Component, ComponentId};
use crate::math::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Position(pub Vec3);

impl Component for Position {
    const ID: ComponentId = ComponentId(1);
}

impl Position {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self(Vec3::new(x, y, z))
    }
}

impl std::ops::Deref for Position {
    type Target = Vec3;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Position {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec3> for Position {
    fn from(v: Vec3) -> Self {
        Self(v)
    }
}
