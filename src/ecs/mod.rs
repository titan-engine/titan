//! Entity-component-system primitives.

mod builtin;
mod command;
mod entity;
mod storage;
mod world;

pub use builtin::{FindNameError, Name};
pub use command::{Commands, DeferredCommandError, DeferredOperation};
pub use entity::Entity;
pub use world::{Component, ComponentMetadata, InsertError, QueryAccessError, World};
