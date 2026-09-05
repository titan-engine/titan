//! Titan is a Rust-first game engine designed for agent-assisted development.
//!
//! The engine is at an early, experimental stage. Its first foundation is a
//! small custom ECS with explicit, deterministic behavior.

extern crate self as titan;

pub mod app;
pub mod ecs;
pub mod input;
pub mod inspection;
pub mod render;
pub mod replay;
pub mod system;
pub mod time;
pub mod ui;

pub use app::{App, AppError, FixedUpdate, Plugin, ScheduleLabel, Startup, Update};
pub use ecs::{
    Bundle, Commands, Component, ComponentMetadata, DeferredCommandError, DeferredOperation,
    Entity, FindNameError, InsertError, Name, QueryAccessError, World,
};
pub use inspection::Inspect;
pub use time::FixedTime;
pub use titan_macros::{Component, Inspect};

pub use ecs::{AccessMode, AccessTarget, Query, QueryData, Res, ResMut, SystemAccess, SystemError};
pub use system::{ApplyDeferred, ExecutorPolicy, IntoSystem, SystemMetadata};
