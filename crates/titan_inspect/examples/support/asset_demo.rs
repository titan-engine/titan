//! Shared headless asset fixture for the example and BRP acceptance tests.
use std::{
    path::Path,
    time::{Duration, Instant},
};

use bevy_app::{App, Startup, TaskPoolPlugin};
use bevy_asset::{
    io::{
        memory::{Dir, MemoryAssetReader},
        AssetSourceBuilder,
    },
    Asset, AssetApp, AssetLoader, AssetPlugin, AssetServer, Handle, LoadContext,
};
use bevy_ecs::prelude::*;
use bevy_reflect::TypePath;
use bevy_remote::{BrpMessage, BrpResult, BrpSender, RemotePlugin};
use serde_json::Value;
use titan_inspect::InspectPlugin;

// Intentionally not Reflect: inspecting metadata must not require asset contents.
#[derive(Asset, TypePath)]
pub struct DemoAsset {
    #[dependency]
    pub children: Vec<Handle<DemoAsset>>,
}

#[derive(TypePath)]
struct DemoLoader;
impl AssetLoader for DemoLoader {
    type Asset = DemoAsset;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn bevy_asset::io::Reader,
        _: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let paths = String::from_utf8(bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        Ok(DemoAsset {
            children: paths
                .lines()
                .filter(|line| !line.is_empty())
                .map(|path| context.load(path.to_owned()))
                .collect(),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["demo"]
    }
}

#[derive(Resource)]
pub struct DemoHandles(pub Vec<Handle<DemoAsset>>);

fn load_at_startup(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(DemoHandles(
        ["valid.demo", "missing.demo", "parent.demo", "scene.demo"]
            .map(|path| server.load(path))
            .to_vec(),
    ));
}

pub fn demo_app() -> App {
    let mut app = App::new();
    let root = Dir::default();
    root.insert_asset_text(Path::new("valid.demo"), "");
    root.insert_asset_text(Path::new("parent.demo"), "missing.demo");
    root.insert_asset_text(Path::new("scene.demo"), "parent.demo");
    app.register_asset_source(
        bevy_asset::io::AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: root.clone() })),
    );
    app.add_plugins((
        TaskPoolPlugin::default(),
        InspectPlugin,
        AssetPlugin::default(),
        RemotePlugin::default(),
    ))
    .init_asset::<DemoAsset>()
    .register_asset_loader(DemoLoader)
    .add_systems(Startup, load_at_startup);
    app.finish();
    app.cleanup();
    app
}

pub fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.update();
        if let Some(handles) = app.world().get_resource::<DemoHandles>() {
            let server = app.world().resource::<AssetServer>();
            if handles.0.iter().all(|handle| {
                server
                    .get_load_states(handle.id())
                    .is_some_and(|(own, _, recursive)| {
                        (own.is_loaded() || own.is_failed())
                            && (recursive.is_loaded() || recursive.is_failed() || own.is_failed())
                    })
            }) {
                break;
            }
        }
        assert!(Instant::now() < deadline, "asset tasks did not settle");
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub fn call(app: &mut App, method: &str, params: Option<Value>) -> BrpResult {
    let (sender, receiver) = async_channel::bounded(1);
    app.world()
        .resource::<BrpSender>()
        .try_send(BrpMessage {
            method: method.to_owned(),
            params,
            sender,
        })
        .unwrap();
    app.update();
    receiver.try_recv().expect("BRP dispatcher response")
}
