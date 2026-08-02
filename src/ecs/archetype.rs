//! Arquetipos: agrupación de entidades con idéntico set de componentes.
//!
//! Un `Archetype` almacena, en arrays paralelos (SoA), una columna `Vec<T>`
//! por cada componente del set. Todas las filas están alineadas: la fila `i`
//! es la misma entidad en todas las columnas.

use crate::ecs::component::ComponentId;
use crate::ecs::entity::EntityId;
use crate::ecs::Component;
use std::any::{Any, TypeId};

/// Identificador de arquetipo dentro del `World`.
pub type ArchetypeId = u32;

/// Columna tipada genérica (trait object).
///
/// El contrato es mínimo: inspección, bytes en memoria, y el movimiento de una
/// fila desde otra columna (equivalente a `swap_remove` + `push`). Todo el
/// downcasting se valida contra `TypeId`/`ComponentId` en el `World`.
pub trait Column: Any + Send + Sync {
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Cantidad de filas.
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes aproximados ocupados por el buffer (capacidad real).
    fn bytes(&self) -> usize;

    /// Mueve el valor de la fila `src_row` de `src` al final de esta columna.
    ///
    /// Semántica `swap_remove`: la fila `src_row` se elimina de `src` moviendo
    /// el último elemento a su posición. Todos los `ColumnImpl` de un mismo
    /// arquetipo deben procesar la misma fila para mantener el alineado.
    fn push_row(&mut self, src: &mut dyn Column, src_row: usize);

    /// Elimina la fila `row` (semántica `swap_remove`).
    fn swap_remove(&mut self, row: usize);

    /// Vacía la columna conservando la capacidad.
    fn clear(&mut self);
}

/// Implementación concreta de columna: un `Vec<T>` plano.
pub struct ColumnImpl<T> {
    pub(crate) data: Vec<T>,
}

impl<T> Default for ColumnImpl<T> {
    fn default() -> Self {
        Self { data: Vec::new() }
    }
}

impl<T> Column for ColumnImpl<T>
where
    T: Component,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn bytes(&self) -> usize {
        self.data.capacity().saturating_mul(std::mem::size_of::<T>())
    }

    fn push_row(&mut self, src: &mut dyn Column, src_row: usize) {
        let src = src
            .as_any_mut()
            .downcast_mut::<ColumnImpl<T>>()
            .expect("push_row: tipo de columna no coincide");
        let value = src.data.swap_remove(src_row);
        self.data.push(value);
    }

    fn swap_remove(&mut self, row: usize) {
        self.data.swap_remove(row);
    }

    fn clear(&mut self) {
        self.data.clear();
    }
}

/// Un grupo de entidades con el mismo set de componentes.
pub struct Archetype {
    pub(crate) id: ArchetypeId,
    /// Set de componentes ordenado ascendentemente (clave de unicidad).
    pub(crate) components: Vec<ComponentId>,
    /// Columnas SoA, paralelas a `components`.
    pub(crate) columns: Vec<Box<dyn Column>>,
    /// Entidades, paralelas a las filas.
    pub(crate) entities: Vec<EntityId>,
}

impl Archetype {
    pub(crate) fn new(id: ArchetypeId, components: Vec<ComponentId>, columns: Vec<Box<dyn Column>>) -> Self {
        debug_assert_eq!(components.len(), columns.len());
        Self {
            id,
            components,
            columns,
            entities: Vec::new(),
        }
    }

    pub fn id(&self) -> ArchetypeId {
        self.id
    }

    /// Ids de los componentes de este arquetipo (ordenados).
    pub fn component_ids(&self) -> &[ComponentId] {
        &self.components
    }

    /// Cantidad de entidades.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Posición del componente en el set (búsqueda binaria).
    pub fn position_of(&self, id: ComponentId) -> Option<usize> {
        self.components.binary_search(&id).ok()
    }

    pub(crate) fn column_mut(&mut self, id: ComponentId) -> &mut dyn Column {
        let p = self
            .position_of(id)
            .unwrap_or_else(|| panic!("columna {:?} ausente en arquetipo {}", id, self.id));
        self.columns[p].as_mut()
    }

    /// Elimina la fila `row` de la lista de entidades (`swap_remove`).
    ///
    /// Devuelve la entidad desplazada a `row`, si la hubo. Nota: las columnas
    /// de datos deben ya haberse eliminado de forma coherente por el llamador.
    pub(crate) fn remove_row_swap(&mut self, row: usize) -> Option<EntityId> {
        let last = self.entities.len() - 1;
        let displaced = (last != row).then(|| self.entities[last]);
        self.entities.swap_remove(row);
        displaced
    }

    /// Bytes aproximados del arquetipo (columnas + metadatos).
    pub fn memory_bytes(&self) -> usize {
        let mut total = self
            .entities
            .capacity()
            .saturating_mul(std::mem::size_of::<EntityId>());
        total += self
            .components
            .capacity()
            .saturating_mul(std::mem::size_of::<ComponentId>());
        total += self.columns.len().saturating_mul(std::mem::size_of::<Box<dyn Column>>());
        for col in &self.columns {
            total += col.bytes();
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::component::Component;

    struct A;
    impl Component for A {
        const ID: ComponentId = ComponentId(1);
    }

    #[allow(dead_code)]
    struct B;
    impl Component for B {
        const ID: ComponentId = ComponentId(2);
    }

    #[test]
    fn push_row_mueve_y_elimina() {
        let mut a: ColumnImpl<A> = ColumnImpl {
            data: vec![A, A, A],
        };
        let mut b: ColumnImpl<A> = ColumnImpl { data: Vec::new() };

        // mover fila 1 de a -> b (swap_remove: el último va a la posición 1)
        b.push_row(&mut a, 1);

        assert_eq!(a.len(), 2);
        assert_eq!(b.len(), 1);
        assert_eq!(a.data.len(), 2);
    }
}
