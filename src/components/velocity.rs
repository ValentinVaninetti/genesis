//! `Velocity`: velocity (vector) of an entity.

use crate::ecs::{Component, ComponentId};
use crate::math::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Velocity(pub Vec3);

impl Component for Velocity {
    const ID: ComponentId = ComponentId(2);
}

impl std::ops::Deref for Velocity {
    type Target = Vec3;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Velocity {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec3> for Velocity {
    fn from(v: Vec3) -> Self {
        Self(v)
    }
}
