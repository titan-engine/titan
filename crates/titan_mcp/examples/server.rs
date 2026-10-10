//! A GPU-free BRP game for exercising the MCP launch/rebuild/restart loop.
//! Run with `cargo run -p titan_mcp --example server -- 15702`.

use bevy_app::{App, TaskPoolOptions, TaskPoolPlugin, Update};
use bevy_ecs::{prelude::*, reflect::ReflectResource};
use bevy_reflect::Reflect;
use bevy_remote::{http::RemoteHttpPlugin, RemotePlugin};
use std::{net::Ipv4Addr, thread, time::Duration};

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct Counter {
    ticks: u64,
}

fn tick(mut counter: ResMut<Counter>) {
    counter.ticks += 1;
}

fn main() {
    let port = std::env::args()
        .nth(1)
        .map_or(15702, |value| value.parse().expect("port"));
    run(port);
}

/// Runs the same headless game used by the lifecycle integration tests.
pub fn run(port: u16) {
    let mut app = App::new();
    // A tiny game needs only the minimum pools, even on large CI runners.
    app.add_plugins(TaskPoolPlugin {
        task_pool_options: TaskPoolOptions::with_num_threads(1),
    })
    .init_resource::<Counter>()
    .register_type::<Counter>()
    .add_systems(Update, tick)
    .add_plugins((
        RemotePlugin::default(),
        RemoteHttpPlugin::default()
            .with_address(Ipv4Addr::LOCALHOST)
            .with_port(port),
    ));
    app.finish();
    app.cleanup();
    loop {
        app.update();
        thread::sleep(Duration::from_millis(5));
    }
}
