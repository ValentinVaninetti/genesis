//! Definición de componentes y su registro global de tipos.

use crate::ecs::archetype::{Column, ColumnImpl};
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Identificador numérico estable de un tipo de componente.
///
/// Los ids **nunca** deben reasignarse ni reutilizarse: son la clave de
/// durabilidad entre el código, el scheduler y los archivos guardados.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComponentId(pub u16);

impl ComponentId {
    /// Id de un componente a partir de su tipo (vía `Component::ID`).
    pub fn of<T: Component>() -> Self {
        T::ID
    }
}

/// Un componente es un fragmento puro de datos.
///
/// Contrato: `Send + Sync + 'static` (para paralelismo y durabilidad) y un
/// `ComponentId` explícito y estable. No se exige `Clone` ni `Default`: el
/// motor mueve los valores entre arquetipos sin copiarlos.
pub trait Component: std::any::Any + Send + Sync + 'static {
    /// Identificador estable del componente.
    const ID: ComponentId;
}

/// Información global de un tipo de componente registrado.
struct ComponentInfo {
    type_id: TypeId,
    /// Fábrica de la columna tipada (SoA) para ese componente.
    new_column: fn() -> Box<dyn Column>,
}

static REGISTRY: OnceLock<Mutex<HashMap<u16, ComponentInfo>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<u16, ComponentInfo>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Recupera el registro tolerando un lock envenenado (p. ej. por un test que
/// paniqueó tras adquirirlo). Como el registro solo se *agrega*, tolerarlo es
/// seguro.
fn registry_lock() -> std::sync::MutexGuard<'static, HashMap<u16, ComponentInfo>> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// Registra un tipo de componente en la tabla global.
///
/// Debe llamarse una sola vez por tipo, normalmente desde
/// `components::register_all()` al arrancar la simulación.
pub fn register<T: Component>() {
    let type_id = TypeId::of::<T>();
    let mut map = registry_lock();
    if let Some(info) = map.get(&T::ID.0) {
        assert_eq!(
            info.type_id, type_id,
            "ComponentId {:?} ya registrado con un tipo distinto",
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

/// Verifica que el id dado corresponda al tipo esperado.
#[allow(dead_code)]
pub(crate) fn assert_type<T: Component>(id: ComponentId) {
    let map = registry().lock().unwrap();
    if let Some(info) = map.get(&id.0) {
        assert_eq!(
            info.type_id,
            TypeId::of::<T>(),
            "el ComponentId {:?} no corresponde a este tipo",
            id
        );
    } else {
        panic!("ComponentId {:?} no registrado", id);
    }
}

/// Crea una columna vacía tipada para el componente con el id dado.
///
/// Libera el lock antes de construir la columna (y antes de poder panickear),
/// para no envenenar el registro.
pub(crate) fn new_column(id: ComponentId) -> Box<dyn Column> {
    let factory = registry_lock().get(&id.0).map(|i| i.new_column);
    match factory {
        Some(f) => f(),
        None => panic!("ComponentId {:?} no registrado", id),
    }
}

/// Tipo asociado a un id (usado para diagnósticos y serialización).
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
    fn registro_y_fabrica() {
        register::<A>();
        let col = new_column(A::ID);
        assert_eq!(col.len(), 0);
    }

    #[test]
    #[should_panic]
    fn id_sin_registrar_panics() {
        let _ = new_column(ComponentId(12345));
    }
}
