//! Produce two recoverable errors as one report with count 2, without a renderer.

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use titan_diagnostics::{DiagnosticsLayer, DiagnosticsPlugin, DiagnosticsState};
use tracing_subscriber::prelude::*;

fn load_level() -> Result {
    tracing::warn!(asset = "levels/demo.ron", "level file is missing");
    Err(BevyError::error("cannot load levels/demo.ron"))
}

fn main() {
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(DiagnosticsLayer))
        .expect("install subscriber");
    let mut app = App::new();
    app.add_plugins(DiagnosticsPlugin {
        directory: "diagnostics/headless".into(),
        app_name: "headless-example".into(),
        app_version: Some(env!("CARGO_PKG_VERSION").into()),
        ..Default::default()
    });
    app.add_systems(Update, load_level);
    app.update();
    app.update();
    assert!(app
        .world()
        .resource::<DiagnosticsState>()
        .last_write_error()
        .is_none());
}
