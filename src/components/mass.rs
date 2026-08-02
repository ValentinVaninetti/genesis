//! `Mass`: inertial/gravitational mass of an entity.

use crate::ecs::{Component, ComponentId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct Mass(pub f64);

impl Component for Mass {
    const ID: ComponentId = ComponentId(3);
}

impl std::ops::Deref for Mass {
    type Target = f64;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Mass {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<f64> for Mass {
    fn from(v: f64) -> Self {
        Self(v)
    }
}
