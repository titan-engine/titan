use std::any::Any;

use super::Entity;
use super::world::{Component, ComponentMetadata};

pub(crate) trait ErasedStorage: Send + Sync {
    fn remove_entity(&mut self, entity: Entity);
    fn contains(&self, entity: Entity) -> bool;
    fn type_name(&self) -> &'static str;
    fn metadata(&self) -> ComponentMetadata;
    fn storage_stats(&self) -> ComponentStorageStats;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Actual vector lengths and capacities, excluding allocation bookkeeping and
/// heap allocations owned by elements. Zero-sized elements retain zero bytes.
#[derive(Clone, Debug, serde::Serialize)]
pub struct VectorStorageStats {
    pub len: usize,
    pub capacity: usize,
    pub element_size: usize,
    pub capacity_bytes: usize,
}

impl VectorStorageStats {
    pub(crate) fn of<T>(values: &Vec<T>) -> Self {
        Self {
            len: values.len(),
            capacity: values.capacity(),
            element_size: std::mem::size_of::<T>(),
            capacity_bytes: values.capacity() * std::mem::size_of::<T>(),
        }
    }
}

/// Retained sparse-set vector storage for one registered component type.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ComponentStorageStats {
    pub type_name: &'static str,
    pub sparse: VectorStorageStats,
    pub entities: VectorStorageStats,
    pub values: VectorStorageStats,
}

/// Read-only storage accounting, not total world or process memory.
///
/// Excludes vector headers, hash maps, boxed storage headers, resources, deferred
/// commands, allocations owned by component values, and allocator overhead.
/// Capacities and element layouts depend on target and Rust implementation.
#[derive(Clone, Debug, serde::Serialize)]
pub struct WorldStorageStats {
    pub entity_slots: VectorStorageStats,
    pub free_entities: VectorStorageStats,
    pub components: Vec<ComponentStorageStats>,
}

#[derive(Clone, Copy)]
struct SparseEntry {
    dense_index: usize,
    generation: u32,
}

pub(crate) struct ComponentStorage<T> {
    sparse: Vec<Option<SparseEntry>>,
    entities: Vec<Entity>,
    values: Vec<T>,
}

impl<T> Default for ComponentStorage<T> {
    fn default() -> Self {
        Self {
            sparse: Vec::new(),
            entities: Vec::new(),
            values: Vec::new(),
        }
    }
}

impl<T> ComponentStorage<T> {
    pub(crate) fn insert(&mut self, entity: Entity, value: T) -> Option<T> {
        let sparse_index = entity.index() as usize;
        if self.sparse.len() <= sparse_index {
            self.sparse.resize(sparse_index + 1, None);
        }

        if let Some(entry) = self.sparse[sparse_index]
            && entry.generation == entity.generation()
        {
            return Some(std::mem::replace(
                &mut self.values[entry.dense_index],
                value,
            ));
        }

        let dense_index = self.values.len();
        self.entities.push(entity);
        self.values.push(value);
        self.sparse[sparse_index] = Some(SparseEntry {
            dense_index,
            generation: entity.generation(),
        });
        None
    }

    pub(crate) fn get(&self, entity: Entity) -> Option<&T> {
        let entry = self.entry(entity)?;
        self.values.get(entry.dense_index)
    }

    pub(crate) fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        let entry = self.entry(entity)?;
        self.values.get_mut(entry.dense_index)
    }

    pub(crate) fn remove(&mut self, entity: Entity) -> Option<T> {
        let entry = self.entry(entity)?;
        let sparse_index = entity.index() as usize;
        self.sparse[sparse_index] = None;

        self.entities.swap_remove(entry.dense_index);
        let value = self.values.swap_remove(entry.dense_index);

        if let Some(moved_entity) = self.entities.get(entry.dense_index).copied() {
            let moved_entry = self.sparse[moved_entity.index() as usize]
                .as_mut()
                .expect("a dense entity must have a sparse entry");
            moved_entry.dense_index = entry.dense_index;
        }

        Some(value)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.entities.iter().copied().zip(&self.values)
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.entities.iter().copied().zip(&mut self.values)
    }

    fn entry(&self, entity: Entity) -> Option<SparseEntry> {
        let entry = self.sparse.get(entity.index() as usize)?.as_ref()?;
        (entry.generation == entity.generation()).then_some(*entry)
    }
}

impl<T: Component> ErasedStorage for ComponentStorage<T> {
    fn storage_stats(&self) -> ComponentStorageStats {
        ComponentStorageStats {
            type_name: std::any::type_name::<T>(),
            sparse: VectorStorageStats::of(&self.sparse),
            entities: VectorStorageStats::of(&self.entities),
            values: VectorStorageStats::of(&self.values),
        }
    }

    fn remove_entity(&mut self, entity: Entity) {
        self.remove(entity);
    }

    fn contains(&self, entity: Entity) -> bool {
        self.get(entity).is_some()
    }

    fn metadata(&self) -> ComponentMetadata {
        T::metadata()
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::ComponentStorage;
    use crate::ecs::Entity;

    #[test]
    fn vector_stats_account_for_zero_sized_elements() {
        let values = vec![(); 3];
        let stats = super::VectorStorageStats::of(&values);
        assert_eq!(stats.len, 3);
        assert_eq!(stats.element_size, 0);
        assert_eq!(stats.capacity_bytes, 0);
    }

    #[test]
    fn removing_an_item_keeps_the_moved_sparse_entry_valid() {
        let first = Entity::new(0, 0);
        let second = Entity::new(1, 0);
        let mut storage = ComponentStorage::default();

        storage.insert(first, 10_u32);
        storage.insert(second, 20_u32);

        assert_eq!(storage.remove(first), Some(10));
        assert_eq!(storage.get(second), Some(&20));
    }

    #[test]
    fn generation_is_part_of_component_identity() {
        let mut storage = ComponentStorage::default();
        storage.insert(Entity::new(4, 1), 10_u32);

        assert_eq!(storage.get(Entity::new(4, 2)), None);
        assert_eq!(storage.remove(Entity::new(4, 2)), None);
    }
}
