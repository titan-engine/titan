//! Opt-in, local development control of the demo over BRP.

use std::net::Ipv4Addr;

use bevy::{
    prelude::*,
    remote::{http::RemoteHttpPlugin, RemotePlugin},
};
use titan_remote::TitanRemotePlugin;

/// Serves BRP on IPv4 loopback with Titan time control and optional screenshots.
///
/// Install after the app's time and frame-count plugins (included in both
/// `DefaultPlugins` and `MinimalPlugins`). Gameplay starts paused so launch
/// readiness does not consume simulation ticks before the agent takes control.
/// With `render`, Titan's screenshot methods capture the primary window.
/// BRP is unauthenticated: use only with trusted local development clients.
pub struct DoomRemotePlugin {
    /// Loopback TCP port; the windowed launcher defaults to 15702.
    pub port: u16,
}

impl Plugin for DoomRemotePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RemotePlugin::default(),
            RemoteHttpPlugin::default()
                .with_address(Ipv4Addr::LOCALHOST)
                .with_port(self.port),
            TitanRemotePlugin,
        ));
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
    }
}
