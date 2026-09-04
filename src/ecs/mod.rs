//! Entity-component-system primitives.

mod entity;
mod storage;
mod world;

pub use entity::Entity;
pub use world::{Component, InsertError, World};
