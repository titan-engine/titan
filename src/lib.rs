//! Titan is a Rust-first game engine designed for agent-assisted development.
//!
//! The engine is at an early, experimental stage. Its first foundation is a
//! small custom ECS with explicit, deterministic behavior.

extern crate self as titan;

pub mod app;
pub mod ecs;
pub mod time;

pub use app::{App, FixedUpdate, Plugin, ScheduleLabel, Startup, Update};
pub use ecs::{Component, ComponentMetadata, Entity, InsertError, World};
pub use time::FixedTime;
pub use titan_macros::Component;
