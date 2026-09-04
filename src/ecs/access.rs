use super::{Commands, World, storage::ErasedStorage};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessMode {
    Read,
    Write,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessTarget {
    Component,
    Resource,
    Commands,
    World,
}
/// Declared access used for validation; execution remains sequential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemAccess {
    pub target: AccessTarget,
    pub mode: AccessMode,
    pub type_name: &'static str,
    pub(crate) type_id: TypeId,
}
impl SystemAccess {
    pub(crate) fn typed<T: 'static>(target: AccessTarget, mode: AccessMode) -> Self {
        Self {
            target,
            mode,
            type_name: std::any::type_name::<T>(),
            type_id: TypeId::of::<T>(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemError {
    ConflictingAccess {
        target: AccessTarget,
        type_name: &'static str,
    },
    MissingResource {
        type_name: &'static str,
    },
}
impl fmt::Display for SystemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingAccess { target, type_name } => {
                write!(f, "conflicting {target:?} access to {type_name}")
            }
            Self::MissingResource { type_name } => {
                write!(f, "required resource {type_name} is missing")
            }
        }
    }
}
impl std::error::Error for SystemError {}
pub(crate) fn validate(accesses: &[SystemAccess]) -> Result<(), SystemError> {
    for (index, access) in accesses.iter().enumerate() {
        for other in &accesses[..index] {
            if (access.target == AccessTarget::World || other.target == AccessTarget::World)
                || (access.target == other.target
                    && access.type_id == other.type_id
                    && (access.mode == AccessMode::Write || other.mode == AccessMode::Write))
            {
                return Err(SystemError::ConflictingAccess {
                    target: access.target,
                    type_name: access.type_name,
                });
            }
        }
    }
    Ok(())
}
/// Internal safe borrow preparation shared by the sealed parameter implementations.
#[doc(hidden)]
pub struct SystemContext<'w> {
    pub(crate) shared_components: HashMap<TypeId, &'w dyn ErasedStorage>,
    pub(crate) mutable_components: HashMap<TypeId, &'w mut dyn ErasedStorage>,
    pub(crate) shared_resources: HashMap<TypeId, &'w (dyn Any + Send + Sync)>,
    pub(crate) mutable_resources: HashMap<TypeId, &'w mut (dyn Any + Send + Sync)>,
    pub(crate) commands: Option<Commands<'w>>,
}
impl<'w> SystemContext<'w> {
    pub(crate) fn prepare(
        world: &'w mut World,
        accesses: &[SystemAccess],
    ) -> Result<Self, SystemError> {
        validate(accesses)?;
        // Check every required resource before handing out any mutable access.
        for access in accesses {
            if access.target == AccessTarget::Resource
                && !world.resources.contains_key(&access.type_id)
            {
                return Err(SystemError::MissingResource {
                    type_name: access.type_name,
                });
            }
        }
        let mode = |target, id| {
            accesses
                .iter()
                .find(|access| access.target == target && access.type_id == id)
                .map(|access| access.mode)
        };
        let mut context = Self {
            shared_components: HashMap::new(),
            mutable_components: HashMap::new(),
            shared_resources: HashMap::new(),
            mutable_resources: HashMap::new(),
            commands: None,
        };
        for (id, storage) in &mut world.components {
            match mode(AccessTarget::Component, *id) {
                Some(AccessMode::Read) => {
                    context.shared_components.insert(*id, &**storage);
                }
                Some(AccessMode::Write) => {
                    context.mutable_components.insert(*id, &mut **storage);
                }
                None => {}
            }
        }
        for (id, resource) in &mut world.resources {
            match mode(AccessTarget::Resource, *id) {
                Some(AccessMode::Read) => {
                    context.shared_resources.insert(*id, &**resource);
                }
                Some(AccessMode::Write) => {
                    context.mutable_resources.insert(*id, &mut **resource);
                }
                None => {}
            }
        }
        if accesses
            .iter()
            .any(|access| access.target == AccessTarget::Commands)
        {
            context.commands = Some(Commands {
                allocator: &mut world.allocator,
                deferred: &mut world.deferred,
            });
        }
        Ok(context)
    }
}
