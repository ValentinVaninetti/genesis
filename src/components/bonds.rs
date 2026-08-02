//! `Bonds`: links between entities.
//!
//! Architectural placeholder. Bonds **emerge** from the laws; this component
//! is only the data that future bond systems will fill.

use crate::ecs::{Component, ComponentId, EntityId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Bonds {
    /// Entities bonded to this one.
    pub neighbors: Vec<EntityId>,
}

impl Component for Bonds {
    const ID: ComponentId = ComponentId(8);
}
