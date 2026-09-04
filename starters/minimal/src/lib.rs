//! Minimal game definition and browser host.
pub mod browser;
pub mod game;
#[cfg(target_arch = "wasm32")]
mod surface;
