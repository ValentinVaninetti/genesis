//! `Charge`: net electric charge (in units of elementary charge).

use crate::ecs::{Component, ComponentId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct Charge(pub f64);

impl Component for Charge {
    const ID: ComponentId = ComponentId(4);
}

impl std::ops::Deref for Charge {
    type Target = f64;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Charge {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<f64> for Charge {
    fn from(v: f64) -> Self {
        Self(v)
    }
}
