//! Opt-in, local development control of the demo over BRP.

use std::net::Ipv4Addr;

use bevy::{
    prelude::*,
    remote::{http::RemoteHttpPlugin, RemotePlugin},
};
use bevy_render::RenderApp;
use titan_remote::TitanRemotePlugin;

/// Serves BRP on IPv4 loopback with Titan time control and optional screenshots.
///
/// Install after the app's time and frame-count plugins (included in both
/// `DefaultPlugins` and `MinimalPlugins`). Gameplay starts paused so launch
/// readiness does not consume simulation ticks before the agent takes control.
/// Only the gameplay world is served: rendering does not open a second BRP
/// listener. With `render`, Titan's screenshot methods capture the primary window.
/// BRP is unauthenticated: use only with trusted local development clients.
pub struct DoomRemotePlugin {
    /// Loopback TCP port; the windowed launcher defaults to 15702.
    pub port: u16,
}

impl Plugin for DoomRemotePlugin {
    fn build(&self, app: &mut App) {
        // Upstream BRP automatically adds an HTTP listener for RenderApp at
        // the fixed port 15703, with no public transport-disable option. This
        // demo exposes only the gameplay world. Stage BRP registration without
        // the render subapp, then restore it before Titan screenshot setup.
        let render_app = app.remove_sub_app(RenderApp);
        app.add_plugins((
            RemotePlugin::default(),
            RemoteHttpPlugin::default()
                .with_address(Ipv4Addr::LOCALHOST)
                .with_port(self.port),
        ));
        if let Some(render_app) = render_app {
            app.insert_sub_app(RenderApp, render_app);
        }
        app.add_plugins(TitanRemotePlugin);
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::SubApp,
        diagnostic::FrameCount,
        remote::{http::HostPort, RemoteMethods},
    };

    #[test]
    fn only_gameplay_world_gets_a_transport_and_render_app_is_preserved() {
        // A synthetic render subapp exercises plugin setup without a GPU.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut render_app = SubApp::new();
        render_app.world_mut().insert_resource(FrameCount(42));
        app.insert_sub_app(RenderApp, render_app);
        app.add_plugins(DoomRemotePlugin { port: 15703 });
        app.finish();
        assert_eq!(app.world().resource::<HostPort>().0, 15703);
        assert!(app.world().contains_resource::<RemoteMethods>());
        let render_world = app.get_sub_app(RenderApp).unwrap().world();
        assert_eq!(render_world.resource::<FrameCount>().0, 42);
        assert!(!render_world.contains_resource::<HostPort>());
        assert!(!render_world.contains_resource::<RemoteMethods>());
    }
}
