use std::any::TypeId;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use super::entity::Entity;
use super::storage::{ComponentStorage, ErasedStorage};

/// A type that can be attached to an entity.
///
/// A derive macro will replace manual implementations as reflection support is
/// introduced. Keeping this trait explicit avoids making every Rust type an
/// inspectable Titan component by accident.
pub trait Component: Send + Sync + 'static {}

#[derive(Clone, Copy, Debug)]
struct EntitySlot {
    generation: u32,
    alive: bool,
}

/// The error returned when a component is inserted using a stale entity handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertError {
    entity: Entity,
}

impl InsertError {
    /// Returns the entity handle that was not alive.
    pub const fn entity(self) -> Entity {
        self.entity
    }
}

impl fmt::Display for InsertError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "entity {:?} is not alive", self.entity)
    }
}

impl Error for InsertError {}

/// Owns entities and all component storage for one ECS world.
///
/// Component iteration follows sparse-set dense order. That order is
/// deterministic for the same sequence of spawn, insert, remove, and despawn
/// operations. Removing a component uses swap removal and can therefore change
/// the position of the final component in later iterations.
#[derive(Default)]
pub struct World {
    entities: Vec<EntitySlot>,
    free_entities: Vec<u32>,
    live_entity_count: usize,
    components: HashMap<TypeId, Box<dyn ErasedStorage>>,
    resources: HashMap<TypeId, Box<dyn std::any::Any + Send + Sync>>,
}

impl World {
    /// Creates an empty world.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a new entity without components.
    pub fn spawn(&mut self) -> Entity {
        if let Some(index) = self.free_entities.pop() {
            let slot = &mut self.entities[index as usize];
            debug_assert!(!slot.alive);
            slot.alive = true;
            self.live_entity_count += 1;
            return Entity::new(index, slot.generation);
        }

        let index = u32::try_from(self.entities.len()).expect("entity capacity exceeded");
        self.entities.push(EntitySlot {
            generation: 0,
            alive: true,
        });
        self.live_entity_count += 1;
        Entity::new(index, 0)
    }

    /// Despawns an entity and removes all of its components.
    ///
    /// Returns `false` when the handle is stale or already despawned.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }

        for storage in self.components.values_mut() {
            storage.remove_entity(entity);
        }

        let slot = &mut self.entities[entity.index() as usize];
        slot.alive = false;
        self.live_entity_count -= 1;

        if let Some(next_generation) = slot.generation.checked_add(1) {
            slot.generation = next_generation;
            self.free_entities.push(entity.index());
        }

        true
    }

    /// Returns whether an entity handle currently refers to a live entity.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities
            .get(entity.index() as usize)
            .is_some_and(|slot| slot.alive && slot.generation == entity.generation())
    }

    /// Returns the number of live entities.
    pub const fn entity_count(&self) -> usize {
        self.live_entity_count
    }

    /// Inserts or replaces a unique world resource.
    pub fn insert_resource<T: Send + Sync + 'static>(&mut self, resource: T) -> Option<T> {
        self.resources
            .insert(TypeId::of::<T>(), Box::new(resource))
            .map(|previous| {
                *previous
                    .downcast::<T>()
                    .expect("resource TypeId must map to its resource type")
            })
    }

    /// Returns a shared reference to a world resource.
    pub fn resource<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.resources.get(&TypeId::of::<T>())?.downcast_ref()
    }

    /// Returns an exclusive reference to a world resource.
    pub fn resource_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.resources.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    /// Removes and returns a world resource.
    pub fn remove_resource<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        let resource = self.resources.remove(&TypeId::of::<T>())?;
        Some(
            *resource
                .downcast::<T>()
                .expect("resource TypeId must map to its resource type"),
        )
    }

    /// Inserts or replaces a component.
    ///
    /// The previous value is returned when this component type was already
    /// attached to the entity.
    pub fn insert<T: Component>(
        &mut self,
        entity: Entity,
        component: T,
    ) -> Result<Option<T>, InsertError> {
        if !self.is_alive(entity) {
            return Err(InsertError { entity });
        }

        Ok(self.storage_mut_or_insert::<T>().insert(entity, component))
    }

    /// Returns a shared reference to an entity's component.
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        self.storage::<T>()?.get(entity)
    }

    /// Returns an exclusive reference to an entity's component.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        self.storage_mut::<T>()?.get_mut(entity)
    }

    /// Removes and returns an entity's component.
    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        self.storage_mut::<T>()?.remove(entity)
    }

    /// Iterates over every entity carrying a component of type `T`.
    pub fn iter<T: Component>(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.storage::<T>()
            .into_iter()
            .flat_map(ComponentStorage::iter)
    }

    /// Mutably iterates over every entity carrying a component of type `T`.
    pub fn iter_mut<T: Component>(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.storage_mut::<T>()
            .into_iter()
            .flat_map(ComponentStorage::iter_mut)
    }

    fn storage<T: Component>(&self) -> Option<&ComponentStorage<T>> {
        self.components
            .get(&TypeId::of::<T>())?
            .as_any()
            .downcast_ref()
    }

    fn storage_mut<T: Component>(&mut self) -> Option<&mut ComponentStorage<T>> {
        self.components
            .get_mut(&TypeId::of::<T>())?
            .as_any_mut()
            .downcast_mut()
    }

    fn storage_mut_or_insert<T: Component>(&mut self) -> &mut ComponentStorage<T> {
        self.components
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(ComponentStorage::<T>::default()))
            .as_any_mut()
            .downcast_mut()
            .expect("component TypeId must map to its component storage")
    }
}

#[cfg(test)]
mod tests {
    use super::{Component, World};

    #[derive(Debug, PartialEq)]
    struct Health(u32);
    impl Component for Health {}

    #[test]
    fn component_lifecycle() {
        let mut world = World::new();
        let entity = world.spawn();

        assert_eq!(world.insert(entity, Health(100)), Ok(None));
        assert_eq!(world.get::<Health>(entity), Some(&Health(100)));
        assert_eq!(world.insert(entity, Health(80)), Ok(Some(Health(100))));
        assert_eq!(world.remove::<Health>(entity), Some(Health(80)));
        assert_eq!(world.get::<Health>(entity), None);
    }

    #[test]
    fn despawn_invalidates_the_handle_and_removes_components() {
        let mut world = World::new();
        let old_entity = world.spawn();
        world.insert(old_entity, Health(100)).unwrap();

        assert!(world.despawn(old_entity));
        assert!(!world.is_alive(old_entity));
        assert_eq!(world.get::<Health>(old_entity), None);
        assert!(!world.despawn(old_entity));

        let new_entity = world.spawn();
        assert_eq!(new_entity.index(), old_entity.index());
        assert_ne!(new_entity.generation(), old_entity.generation());
        assert_eq!(
            world.insert(old_entity, Health(5)).unwrap_err().entity(),
            old_entity
        );
    }

    #[test]
    fn iteration_is_repeatable_for_the_same_operations() {
        let values = || {
            let mut world = World::new();
            let first = world.spawn();
            let second = world.spawn();
            let third = world.spawn();
            world.insert(first, Health(1)).unwrap();
            world.insert(second, Health(2)).unwrap();
            world.insert(third, Health(3)).unwrap();
            world.remove::<Health>(second);
            world
                .iter::<Health>()
                .map(|(_, value)| value.0)
                .collect::<Vec<_>>()
        };

        assert_eq!(values(), values());
        assert_eq!(values(), vec![1, 3]);
    }

    #[test]
    fn mutable_iteration_updates_components() {
        let mut world = World::new();
        for value in 0..3 {
            let entity = world.spawn();
            world.insert(entity, Health(value)).unwrap();
        }

        for (_, health) in world.iter_mut::<Health>() {
            health.0 += 10;
        }

        assert_eq!(
            world
                .iter::<Health>()
                .map(|(_, health)| health.0)
                .collect::<Vec<_>>(),
            vec![10, 11, 12]
        );
    }
}
