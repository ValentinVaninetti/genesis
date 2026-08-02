//! Component definition and their global type registry.

use crate::ecs::archetype::{Column, ColumnImpl};
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Stable numeric identifier of a component type.
///
/// Ids **never** must be reassigned or reused: they are the durability key
/// between the code, the scheduler and saved files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComponentId(pub u16);

impl ComponentId {
    /// Id of a component from its type (via `Component::ID`).
    pub fn of<T: Component>() -> Self {
        T::ID
    }
}

/// A component is a pure fragment of data.
///
/// Contract: `Send + Sync + 'static` (for parallelism and durability) and an
/// explicit, stable `ComponentId`. Neither `Clone` nor `Default` are
/// required: the engine moves values between archetypes without copying them.
pub trait Component: std::any::Any + Send + Sync + 'static {
    /// Stable identifier of the component.
    const ID: ComponentId;
}

/// Global information of a registered component type.
struct ComponentInfo {
    type_id: TypeId,
    /// Factory of the typed (SoA) column for that component.
    new_column: fn() -> Box<dyn Column>,
}

static REGISTRY: OnceLock<Mutex<HashMap<u16, ComponentInfo>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<u16, ComponentInfo>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Retrieves the registry tolerating a poisoned lock (e.g. by a test that
/// panicked after acquiring it). Since the registry is only *appended to*,
/// tolerating it is safe.
fn registry_lock() -> std::sync::MutexGuard<'static, HashMap<u16, ComponentInfo>> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// Registers a component type in the global table.
///
/// Must be called once per type, usually from `components::register_all()`
/// when the simulation starts.
pub fn register<T: Component>() {
    let type_id = TypeId::of::<T>();
    let mut map = registry_lock();
    if let Some(info) = map.get(&T::ID.0) {
        assert_eq!(
            info.type_id, type_id,
            "ComponentId {:?} already registered with a different type",
            T::ID
        );
    } else {
        map.insert(
            T::ID.0,
            ComponentInfo {
                type_id,
                new_column: || Box::new(ColumnImpl::<T>::default()),
            },
        );
    }
}

/// Verifies that the given id corresponds to the expected type.
#[allow(dead_code)]
pub(crate) fn assert_type<T: Component>(id: ComponentId) {
    let map = registry().lock().unwrap();
    if let Some(info) = map.get(&id.0) {
        assert_eq!(
            info.type_id,
            TypeId::of::<T>(),
            "ComponentId {:?} does not match this type",
            id
        );
    } else {
        panic!("ComponentId {:?} not registered", id);
    }
}

/// Creates an empty typed column for the component with the given id.
///
/// Releases the lock before building the column (and before it could panic),
/// so the registry is not poisoned.
pub(crate) fn new_column(id: ComponentId) -> Box<dyn Column> {
    let factory = registry_lock().get(&id.0).map(|i| i.new_column);
    match factory {
        Some(f) => f(),
        None => panic!("ComponentId {:?} not registered", id),
    }
}

/// Type associated to an id (used for diagnostics and serialization).
#[allow(dead_code)]
pub(crate) fn type_of(id: ComponentId) -> Option<TypeId> {
    registry_lock().get(&id.0).map(|i| i.type_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct A;
    impl Component for A {
        const ID: ComponentId = ComponentId(99);
    }

    #[test]
    fn registration_and_factory() {
        register::<A>();
        let col = new_column(A::ID);
        assert_eq!(col.len(), 0);
    }

    #[test]
    #[should_panic]
    fn unregistered_id_panics() {
        let _ = new_column(ComponentId(12345));
    }
}
