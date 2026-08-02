//! `Acceleration`: second derivative of the position (net force over mass).

use crate::ecs::{Component, ComponentId};
use crate::math::Vec3;
use serde::{Deserialize, Serialize};

/// Net acceleration of an entity, recomputed every tick by the force system.
/// It is the intermediate state of the Verlet integrator: the acceleration of
/// the previous tick is read for the velocity half step, and the force system
/// replaces it with the current one.
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
