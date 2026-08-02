//! `Bonds`: enlaces entre entidades.
//!
//! Placeholder arquitectónico. Los enlaces **emergen** de las leyes; este
//! componente es solo el dato que los sistemas de enlace futuros llenarán.

use crate::ecs::{Component, ComponentId, EntityId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Bonds {
    /// Entidades enlazadas a esta.
    pub neighbors: Vec<EntityId>,
}

impl Component for Bonds {
    const ID: ComponentId = ComponentId(8);
}
