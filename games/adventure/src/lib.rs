//! Standalone, GPU-independent adventure simulation and inspection.
pub mod browser;
pub mod game;

#[cfg(feature = "movement-acceptance")]
pub mod acceptance;

#[cfg(all(target_arch = "wasm32", feature = "player"))]
pub mod browser_player;
pub mod player;

#[cfg(feature = "player")]
pub mod capture;
