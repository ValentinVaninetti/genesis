//! Recursos globales de la simulación.
//!
//! Los recursos son valores *singleton* identificados por `TypeId`, como
//! configuración, contadores globales, tablas, etc. Viven separados del
//! `World` para que los sistemas puedan pedir ambos a la vez sin conflictos
//! de borrow y para que el scheduler pueda declarar el acceso a ellos.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Almacén de recursos tipados.
#[derive(Default)]
pub struct Resources {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Resources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserta o reemplaza un recurso.
    pub fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Acceso inmutable a un recurso.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>())?.downcast_ref()
    }

    /// Acceso mutable a un recurso.
    pub fn get_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        self.map.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    /// Remueve un recurso y lo devuelve.
    pub fn remove<T: Any + Send + Sync>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast().ok())
            .map(|b| *b)
    }

    /// ¿Existe el recurso `T`?
    pub fn contains<T: Any + Send + Sync>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Cantidad de recursos registrados.
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
    fn ciclo_de_vida() {
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
