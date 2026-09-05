use std::any::TypeId;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use super::bundle::Bundle;
use super::command::{Commands, DeferredCommand, DeferredCommandError};
use super::entity::Entity;
use super::storage::{ComponentStorage, ErasedStorage};

/// Basic information available for every registered component type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentMetadata {
    /// The fully qualified Rust type name.
    pub type_name: &'static str,
    /// The size of the component in bytes.
    pub size: usize,
    /// The alignment of the component in bytes.
    pub align: usize,
}

/// A type that can be attached to an entity.
///
/// Implement this with `#[derive(Component)]` in normal game code. Value
/// reflection and serialization will remain separate opt-in capabilities.
pub trait Component: Send + Sync + 'static {
    /// Returns the metadata that is always available for this component.
    fn metadata() -> ComponentMetadata
    where
        Self: Sized,
    {
        ComponentMetadata {
            type_name: std::any::type_name::<Self>(),
            size: std::mem::size_of::<Self>(),
            align: std::mem::align_of::<Self>(),
        }
    }
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

/// An invalid combination of component accesses in a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryAccessError {
    type_name: &'static str,
}

impl QueryAccessError {
    /// Returns the component type requested through conflicting accesses.
    pub const fn type_name(self) -> &'static str {
        self.type_name
    }
}

impl fmt::Display for QueryAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "component {} cannot be borrowed mutably and immutably by one query",
            self.type_name
        )
    }
}

impl Error for QueryAccessError {}

/// Owns entities and all component storage for one ECS world.
///
/// Component iteration follows sparse-set dense order. That order is
/// deterministic for the same sequence of spawn, insert, remove, and despawn
/// operations. Removing a component uses swap removal and can therefore change
/// the position of the final component in later iterations.
#[derive(Default)]
pub struct World {
    pub(crate) allocator: super::allocator::EntityAllocator,
    pub(crate) components: HashMap<TypeId, Box<dyn ErasedStorage>>,
    pub(crate) resources: HashMap<TypeId, Box<dyn std::any::Any + Send + Sync>>,
    pub(crate) deferred: Vec<Box<dyn DeferredCommand>>,
}

impl World {
    /// Creates an empty world.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a new entity without components.
    pub fn spawn(&mut self) -> Entity {
        self.allocator.allocate_entity(false)
    }

    /// Allocates an entity and inserts its initial components in tuple order.
    pub fn spawn_with<B: Bundle>(&mut self, bundle: B) -> Entity {
        let entity = self.spawn();
        self.insert_bundle(entity, bundle)
            .expect("a newly spawned entity must be alive");
        entity
    }

    /// Inserts or replaces all components in a bundle.
    ///
    /// Stale or reserved handles are rejected before any components are
    /// inserted, including for an empty bundle. Repeated component types are
    /// applied in tuple order; the final value wins. Replaced values are dropped.
    pub fn insert_bundle<B: Bundle>(
        &mut self,
        entity: Entity,
        bundle: B,
    ) -> Result<(), InsertError> {
        if !self.is_alive(entity) {
            return Err(InsertError { entity });
        }
        bundle.insert_into(self, entity)
    }

    /// Returns a command writer that queues structural changes.
    pub fn commands(&mut self) -> Commands<'_> {
        Commands {
            allocator: &mut self.allocator,
            deferred: &mut self.deferred,
        }
    }

    /// Applies all queued structural changes in insertion order.
    ///
    /// Commands queued while this batch is being applied are deferred until the
    /// next call. All commands are attempted, and failures are returned in
    /// their deterministic application order.
    pub fn apply_deferred(&mut self) -> Vec<DeferredCommandError> {
        let commands = std::mem::take(&mut self.deferred);
        commands
            .into_iter()
            .filter_map(|command| command.apply(self).err())
            .collect()
    }

    pub(crate) fn activate_reserved(&mut self, entity: Entity) -> bool {
        self.allocator.activate_reserved(entity)
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

        self.allocator.release(entity);
        true
    }

    /// Returns whether this generational entity handle is currently alive.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.allocator.is_alive(entity)
    }
    /// Returns the number of live entities, excluding reserved commands.
    pub const fn entity_count(&self) -> usize {
        self.allocator.entity_count()
    }
    /// Iterates over live entities in ascending allocator-index order.
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.allocator.entities()
    }

    /// Reports actual retained ECS vector storage without changing capacities.
    /// See [`super::WorldStorageStats`] for accounting exclusions.
    pub fn storage_stats(&self) -> super::WorldStorageStats {
        let (entity_slots, free_entities) = self.allocator.storage_stats();
        let mut components: Vec<_> = self
            .components
            .values()
            .map(|storage| storage.storage_stats())
            .collect();
        components.sort_by_key(|storage| storage.type_name);
        super::WorldStorageStats {
            entity_slots,
            free_entities,
            components,
        }
    }

    /// Returns sorted metadata for all component types registered by insertion,
    /// including types whose last instance has been removed.
    pub fn component_metadata(&self) -> Vec<ComponentMetadata> {
        let mut metadata: Vec<_> = self
            .components
            .values()
            .map(|storage| storage.metadata())
            .collect();
        metadata.sort_by_key(|metadata| metadata.type_name);
        metadata
    }

    /// Returns sorted Rust type names for components attached to an entity.
    pub fn component_type_names(&self, entity: Entity) -> Vec<&'static str> {
        if !self.is_alive(entity) {
            return Vec::new();
        }
        let mut names: Vec<_> = self
            .components
            .values()
            .filter(|storage| storage.contains(entity))
            .map(|storage| storage.type_name())
            .collect();
        names.sort_unstable();
        names
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

    /// Iterates over entities carrying both component types.
    pub fn iter2<A: Component, B: Component>(&self) -> impl Iterator<Item = (Entity, &A, &B)> {
        let second = self.storage::<B>();
        self.storage::<A>().into_iter().flat_map(move |first| {
            first.iter().filter_map(move |(entity, first_value)| {
                second?
                    .get(entity)
                    .map(|second_value| (entity, first_value, second_value))
            })
        })
    }

    /// Applies a function to entities carrying mutable `A` and shared `B`.
    ///
    /// Returns the number of matching entities. Asking for two forms of access
    /// to the same component type returns an error before any values are
    /// visited.
    pub fn for_each_mut_with<A: Component, B: Component>(
        &mut self,
        mut visit: impl FnMut(Entity, &mut A, &B),
    ) -> Result<usize, QueryAccessError> {
        if TypeId::of::<A>() == TypeId::of::<B>() {
            return Err(QueryAccessError {
                type_name: std::any::type_name::<A>(),
            });
        }

        let [first, second] = self
            .components
            .get_disjoint_mut([&TypeId::of::<A>(), &TypeId::of::<B>()]);
        let (Some(first), Some(second)) = (first, second) else {
            return Ok(0);
        };
        let first = first
            .as_any_mut()
            .downcast_mut::<ComponentStorage<A>>()
            .expect("component TypeId must map to its component storage");
        let second = second
            .as_any()
            .downcast_ref::<ComponentStorage<B>>()
            .expect("component TypeId must map to its component storage");
        let mut visited = 0;
        for (entity, first_value) in first.iter_mut() {
            if let Some(second_value) = second.get(entity) {
                visit(entity, first_value, second_value);
                visited += 1;
            }
        }
        Ok(visited)
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

    struct Damage(u32);
    impl Component for Damage {}

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

    #[test]
    fn deferred_commands_are_invisible_until_applied() {
        let mut world = World::new();
        let entity = {
            let mut commands = world.commands();
            commands.spawn_with(Health(100))
        };

        assert!(!world.is_alive(entity));
        assert_eq!(world.get::<Health>(entity), None);
        assert!(world.apply_deferred().is_empty());
        assert!(world.is_alive(entity));
        assert_eq!(world.get::<Health>(entity), Some(&Health(100)));
    }

    #[test]
    fn deferred_failures_are_structured_and_do_not_stop_later_commands() {
        let mut world = World::new();
        let stale = world.spawn();
        world.despawn(stale);
        let valid = {
            let mut commands = world.commands();
            commands.insert(stale, Health(1));
            commands.spawn_with(Health(2))
        };

        let errors = world.apply_deferred();

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].entity(), stale);
        assert_eq!(errors[0].operation(), crate::ecs::DeferredOperation::Insert);
        assert_eq!(world.get::<Health>(valid), Some(&Health(2)));
    }

    #[test]
    fn two_component_queries_join_and_mutate_matching_entities() {
        let mut world = World::new();
        let complete = world.spawn();
        let incomplete = world.spawn();
        world.insert(complete, Health(100)).unwrap();
        world.insert(complete, Damage(25)).unwrap();
        world.insert(incomplete, Health(50)).unwrap();

        let values: Vec<_> = world
            .iter2::<Health, Damage>()
            .map(|(entity, health, damage)| (entity, health.0, damage.0))
            .collect();
        assert_eq!(values, vec![(complete, 100, 25)]);

        let visited = world
            .for_each_mut_with::<Health, Damage>(|_, health, damage| {
                health.0 -= damage.0;
            })
            .unwrap();
        assert_eq!(visited, 1);
        assert_eq!(world.get::<Health>(complete), Some(&Health(75)));
        assert_eq!(world.get::<Health>(incomplete), Some(&Health(50)));
    }

    #[test]
    fn conflicting_query_access_is_rejected_before_iteration() {
        let mut world = World::new();
        let error = world
            .for_each_mut_with::<Health, Health>(|_, _, _| unreachable!())
            .unwrap_err();

        assert_eq!(error.type_name(), std::any::type_name::<Health>());
    }
}
