use std::any::Any;

use super::Entity;
use super::world::{Component, ComponentMetadata};

pub(crate) trait ErasedStorage: Send + Sync {
    fn remove_entity(&mut self, entity: Entity);
    fn contains(&self, entity: Entity) -> bool;
    fn type_name(&self) -> &'static str;
    fn metadata(&self) -> ComponentMetadata;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
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
