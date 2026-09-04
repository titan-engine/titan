//! Entity-component-system primitives.

pub(crate) mod access;
mod allocator;
mod builtin;
mod bundle;
mod command;
mod entity;
mod params;
mod query;
mod storage;
mod world;

pub use builtin::{FindNameError, Name};
pub use bundle::Bundle;
pub use command::{Commands, DeferredCommandError, DeferredOperation};
pub use entity::Entity;
pub use world::{Component, ComponentMetadata, InsertError, QueryAccessError, World};

pub use access::{AccessMode, AccessTarget, SystemAccess, SystemError};
pub use params::{Res, ResMut, SystemParam};
pub use query::{Query, QueryData};
