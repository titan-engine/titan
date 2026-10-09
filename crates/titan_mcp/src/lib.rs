//! An MCP stdio sidecar for local Titan games exposing the Bevy Remote Protocol.
//!
//! The game needs `RemotePlugin` and `RemoteHttpPlugin`. Optional `TitanRemotePlugin`
//! methods enable time control and file-based screenshots. See the crate README for
//! configuration, tools, and the localhost-only security model.

pub mod client;
pub mod protocol;
pub mod screenshot;
pub mod tools;
