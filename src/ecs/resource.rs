//! Global resources of the simulation.
//!
//! Resources are *singleton* values identified by `TypeId`, such as
//! configuration, global counters, tables, etc. They live separated from the
//! `World` so that systems can ask for both at once without borrow conflicts
//! and so the scheduler can declare access to them.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Store of typed resources.
#[derive(Default)]
pub struct Resources {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Resources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a resource.
    pub fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Immutable access to a resource.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>())?.downcast_ref()
    }

    /// Mutable access to a resource.
    pub fn get_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        self.map.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    /// Removes a resource and returns it.
    pub fn remove<T: Any + Send + Sync>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast().ok())
            .map(|b| *b)
    }

    /// Does the resource `T` exist?
    pub fn contains<T: Any + Send + Sync>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Number of registered resources.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle() {
        let mut r = Resources::new();
        assert!(!r.contains::<u32>());
        r.insert(42u32);
        assert_eq!(r.get::<u32>(), Some(&42));
        if let Some(v) = r.get_mut::<u32>() {
            *v += 1;
        }
        assert_eq!(r.remove::<u32>(), Some(43));
        assert!(!r.contains::<u32>());
    }
}
