use std::fmt;

use super::{Component, Entity, World};

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
    pub(crate) world: &'world mut World,
}

impl Commands<'_> {
    /// Reserves an entity and queues its activation.
    pub fn spawn(&mut self) -> Entity {
        let entity = self.world.reserve_entity();
        self.world.push_command(Activate::new(entity));
        entity
    }

    /// Reserves an entity and queues one initial component.
    pub fn spawn_with<T: Component>(&mut self, component: T) -> Entity {
        let entity = self.spawn();
        self.insert(entity, component);
        entity
    }

    /// Queues insertion or replacement of a component.
    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) -> &mut Self {
        self.world.push_command(Insert { entity, component });
        self
    }

    /// Queues an entity for despawning.
    pub fn despawn(&mut self, entity: Entity) -> &mut Self {
        self.world.push_command(Despawn { entity });
        self
    }
}
