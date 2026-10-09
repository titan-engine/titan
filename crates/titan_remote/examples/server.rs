//! A local BRP server with time control and primary-window screenshots.

use bevy::prelude::*;
use bevy_remote::{http::RemoteHttpPlugin, RemotePlugin};
use titan_remote::TitanRemotePlugin;

fn main() {
    let port = std::env::var("TITAN_REMOTE_PORT")
        .map(|port| {
            port.parse::<u16>()
                .expect("TITAN_REMOTE_PORT must be a port number")
        })
        .unwrap_or(15702);
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            RemotePlugin::default(),
            RemoteHttpPlugin::default().with_port(port),
            TitanRemotePlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, rotate)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn(Sprite::from_color(
        Color::srgb(0.2, 0.7, 0.9),
        Vec2::splat(160.0),
    ));
}

fn rotate(time: Res<Time>, mut sprites: Query<&mut Transform, With<Sprite>>) {
    for mut transform in &mut sprites {
        transform.rotate_z(time.delta_secs());
    }
}
