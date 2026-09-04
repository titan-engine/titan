use std::fmt;

use super::{Bundle, Component, Entity, World};

/// The kind of structural operation that failed during deferred application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeferredOperation {
    Spawn,
    Insert,
    Despawn,
}

/// A structured failure produced while applying a deferred ECS command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeferredCommandError {
    entity: Entity,
    operation: DeferredOperation,
}

impl DeferredCommandError {
    pub(crate) const fn new(entity: Entity, operation: DeferredOperation) -> Self {
        Self { entity, operation }
    }

    /// Returns the entity targeted by the failed command.
    pub const fn entity(self) -> Entity {
        self.entity
    }

    /// Returns the operation that failed.
    pub const fn operation(self) -> DeferredOperation {
        self.operation
    }
}

impl fmt::Display for DeferredCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "deferred {:?} failed for entity {:?}",
            self.operation, self.entity
        )
    }
}

impl std::error::Error for DeferredCommandError {}

pub(crate) trait DeferredCommand: Send {
    fn apply(self: Box<Self>, world: &mut World) -> Result<(), DeferredCommandError>;
}

pub(crate) struct Activate {
    entity: Entity,
}

impl Activate {
    pub(crate) const fn new(entity: Entity) -> Self {
        Self { entity }
    }
}

impl DeferredCommand for Activate {
    fn apply(self: Box<Self>, world: &mut World) -> Result<(), DeferredCommandError> {
        world
            .activate_reserved(self.entity)
            .then_some(())
            .ok_or_else(|| DeferredCommandError::new(self.entity, DeferredOperation::Spawn))
    }
}

struct Insert<T> {
    entity: Entity,
    component: T,
}

impl<T: Component> DeferredCommand for Insert<T> {
    fn apply(self: Box<Self>, world: &mut World) -> Result<(), DeferredCommandError> {
        world
            .insert(self.entity, self.component)
            .map(|_| ())
            .map_err(|_| DeferredCommandError::new(self.entity, DeferredOperation::Insert))
    }
}

struct InsertBundle<B> {
    entity: Entity,
    bundle: B,
}

impl<B: Bundle> DeferredCommand for InsertBundle<B> {
    fn apply(self: Box<Self>, world: &mut World) -> Result<(), DeferredCommandError> {
        world
            .insert_bundle(self.entity, self.bundle)
            .map_err(|_| DeferredCommandError::new(self.entity, DeferredOperation::Insert))
    }
}

struct Despawn {
    entity: Entity,
}

impl DeferredCommand for Despawn {
    fn apply(self: Box<Self>, world: &mut World) -> Result<(), DeferredCommandError> {
        world
            .despawn(self.entity)
            .then_some(())
            .ok_or_else(|| DeferredCommandError::new(self.entity, DeferredOperation::Despawn))
    }
}

/// Queues structural world changes for the next synchronization point.
///
/// A reserved entity ID is returned by [`spawn`](Self::spawn) immediately, but
/// the entity and its components are not visible until the commands are
/// applied at the end of the schedule or by [`World::apply_deferred`].
pub struct Commands<'world> {
    pub(crate) allocator: &'world mut super::allocator::EntityAllocator,
    pub(crate) deferred: &'world mut Vec<Box<dyn DeferredCommand>>,
}

impl Commands<'_> {
    /// Reserves an entity and queues its activation.
    pub fn spawn(&mut self) -> Entity {
        let entity = self.allocator.reserve_entity();
        self.deferred.push(Box::new(Activate::new(entity)));
        entity
    }

    /// Reserves an entity and queues its initial component bundle.
    pub fn spawn_with<B: Bundle>(&mut self, bundle: B) -> Entity {
        let entity = self.spawn();
        self.insert_bundle(entity, bundle);
        entity
    }

    /// Queues insertion or replacement of a component.
    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) -> &mut Self {
        self.deferred.push(Box::new(Insert { entity, component }));
        self
    }

    /// Queues a bundle as one insertion operation.
    ///
    /// Components are inserted in tuple order at the synchronization point.
    /// An invalid entity produces one error without inserting any components.
    pub fn insert_bundle<B: Bundle>(&mut self, entity: Entity, bundle: B) -> &mut Self {
        self.deferred
            .push(Box::new(InsertBundle { entity, bundle }));
        self
    }

    /// Queues an entity for despawning.
    pub fn despawn(&mut self, entity: Entity) -> &mut Self {
        self.deferred.push(Box::new(Despawn { entity }));
        self
    }
}
