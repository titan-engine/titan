//! Real `TitanRemotePlugin` + HTTP screenshot integration, without a GPU.
//!
//! The fake `RenderApp` only establishes renderer availability. Synthetic image
//! readback triggers the real `ScreenshotCaptured` observer, PNG encoder, atomic
//! publication, token/status handlers, and MCP screenshot tool. The child
//! process owns the HTTP listener so the fixture can reap it even on panic.

#![cfg(feature = "remote-render")]

use std::{
    io::Cursor,
    net::{Ipv4Addr, TcpListener},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use base64::Engine as _;
use bevy_app::{App, SubApp, TaskPoolPlugin, Update};
use bevy_asset::RenderAssetUsages;
use bevy_diagnostic::FrameCountPlugin;
use bevy_ecs::prelude::*;
use bevy_image::Image;
use bevy_remote::{
    builtin_methods::BRP_SPAWN_ENTITY_METHOD, http::RemoteHttpPlugin, BrpResult,
    RemoteMethodSystemId, RemoteMethods, RemotePlugin,
};
use bevy_render::{
    render_resource::{Extent3d, TextureDimension, TextureFormat},
    view::screenshot::{Screenshot, ScreenshotCaptured},
    RenderApp, RenderScheduleOrder,
};
use bevy_time::TimePlugin;
use bevy_window::{PrimaryWindow, Window};
use serde_json::{json, Value};
use titan_mcp::{client::Client, tools};
use titan_remote::TitanRemotePlugin;

const FIXTURE_PORT: &str = "TITAN_MCP_SCREENSHOT_TEST_PORT";
const WAIT: Duration = Duration::from_secs(10);
const SCREENSHOT: &str = "titan.screenshot";
const STATUS: &str = "titan.screenshot_status";

/// Instrumentation forwards to the actual plugin systems, never a mock handler.
#[derive(Resource)]
struct Handlers {
    screenshot: RemoteMethodSystemId,
    status: RemoteMethodSystemId,
    spawn: RemoteMethodSystemId,
}

#[derive(Resource, Default)]
struct Trace(Vec<Value>);

fn forward(world: &mut World, method: &str, params: Option<Value>) -> BrpResult {
    let handlers = world.resource::<Handlers>();
    let handler = match method {
        SCREENSHOT => handlers.screenshot,
        STATUS => handlers.status,
        BRP_SPAWN_ENTITY_METHOD => handlers.spawn,
        _ => panic!("unexpected instrumented method {method}"),
    };
    let RemoteMethodSystemId::Instant(handler) = handler else {
        panic!("Titan screenshot methods must be instant token/status handlers");
    };
    let result = world.run_system_with(handler, params.clone()).unwrap();
    let outcome = match &result {
        Ok(value) => json!({ "result": value }),
        Err(error) => json!({ "error": error.message }),
    };
    world.resource_mut::<Trace>().0.push(json!({
        "method": method, "params": params, "outcome": outcome,
    }));
    result
}

fn screenshot(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    forward(world, SCREENSHOT, params)
}

fn status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    forward(world, STATUS, params)
}

fn spawn_probe(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    forward(world, BRP_SPAWN_ENTITY_METHOD, params)
}

fn trace(In(_): In<Option<Value>>, trace: Res<Trace>) -> BrpResult {
    Ok(json!(trace.0))
}

fn synthetic_readback(world: &mut World) {
    // Ensure at least one real pending status response crosses HTTP before
    // producing readback; otherwise a very fast fixture might skip that state.
    let has_pending_status = world
        .resource::<Trace>()
        .0
        .iter()
        .any(|call| call["method"] == STATUS && call["outcome"]["result"]["pending"] == true);
    if !has_pending_status {
        return;
    }
    let mut query = world.query_filtered::<Entity, With<Screenshot>>();
    let entities: Vec<_> = query.iter(world).collect();
    for entity in entities {
        let image = Image::new_fill(
            Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[10, 20, 30, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        // Titan's real observer encodes and atomically publishes this image.
        world.trigger(ScreenshotCaptured { entity, image });
        // Upstream's renderer despawns the captured entity on the next frame.
        world.despawn(entity);
    }
}

#[test]
#[ignore = "child-process fixture, launched by the integration test"]
fn remote_screenshot_fixture_process() {
    let Ok(port) = std::env::var(FIXTURE_PORT) else {
        return;
    };
    let mut app = App::new();
    let mut render_app = SubApp::new();
    // RemotePlugin's optional render-world support needs this even though no
    // render schedules or GPU resources are driven by our pretend renderer.
    render_app.init_resource::<RenderScheduleOrder>();
    app.insert_sub_app(RenderApp, render_app);
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        FrameCountPlugin,
        RemotePlugin::default().with_method_main("test.screenshot_trace", trace),
        RemoteHttpPlugin::default()
            .with_address(Ipv4Addr::LOCALHOST)
            .with_port(port.parse().unwrap()),
        TitanRemotePlugin,
    ))
    .init_resource::<Trace>()
    .add_systems(Update, synthetic_readback);
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    app.finish();
    app.cleanup();

    let methods = app.world().resource::<RemoteMethods>();
    let handlers = Handlers {
        screenshot: *methods.get(SCREENSHOT).unwrap(),
        status: *methods.get(STATUS).unwrap(),
        spawn: *methods.get(BRP_SPAWN_ENTITY_METHOD).unwrap(),
    };
    app.insert_resource(handlers);
    let screenshot = app.register_system(screenshot);
    let status = app.register_system(status);
    let spawn = app.register_system(spawn_probe);
    let mut methods = app.world_mut().resource_mut::<RemoteMethods>();
    methods.insert(SCREENSHOT, RemoteMethodSystemId::Instant(screenshot));
    methods.insert(STATUS, RemoteMethodSystemId::Instant(status));
    methods.insert(
        BRP_SPAWN_ENTITY_METHOD,
        RemoteMethodSystemId::Instant(spawn),
    );
    loop {
        app.update();
        thread::sleep(Duration::from_millis(2));
    }
}

struct Fixture {
    child: Child,
    client: Client,
}

impl Fixture {
    fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = Client::new(&format!("http://127.0.0.1:{port}")).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "remote_screenshot_fixture_process",
                "--nocapture",
            ])
            .env(FIXTURE_PORT, port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        // RemoteHttpPlugin binds itself, so release the port reservation just
        // before launching. Readiness errors include a competing-bind failure.
        drop(listener);
        let child = command.spawn().unwrap();
        let mut fixture = Self { child, client };
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = fixture.child.try_wait().unwrap() {
                panic!("screenshot fixture exited before readiness: {status}");
            }
            match fixture
                .client
                .call_with_deadline("rpc.discover", None, deadline)
            {
                Ok(_) => return fixture,
                Err(error) => assert!(
                    Instant::now() < deadline,
                    "screenshot fixture did not become ready: {error}"
                ),
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn real_remote_capacity_rejection_is_not_bypassed_by_raw_capture() {
    // No status polls means synthetic readback never completes. All 64 jobs
    // remain pending in the real plugin; destinations/staging files are ours.
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::start();
    for index in 0..64 {
        let path = directory.path().join(format!("pending-{index}.png"));
        let result = fixture
            .client
            .call(SCREENSHOT, Some(json!({"path":path})))
            .unwrap();
        assert!(result["token"].is_u64(), "{result}");
    }
    let error = tools::call(&fixture.client, "screenshot", json!({"timeout_secs":5})).unwrap_err();
    assert!(error.contains("too many screenshots"), "{error}");
    let trace = fixture.client.call("test.screenshot_trace", None).unwrap();
    let calls = trace.as_array().unwrap();
    assert_eq!(calls.len(), 65);
    assert!(
        calls.iter().all(|call| call["method"] == SCREENSHOT),
        "{trace}"
    );
    let rejected_path = calls.last().unwrap()["params"]["path"].as_str().unwrap();
    assert!(!Path::new(rejected_path).parent().unwrap().exists());
}

#[test]
fn real_remote_screenshot_uses_tokens_atomic_publication_and_temp_cleanup() {
    let fixture = Fixture::start();
    let result = tools::call(&fixture.client, "screenshot", json!({ "timeout_secs": 5 })).unwrap();
    assert_eq!(result["isError"], false);
    let content = result["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["mimeType"], "image/png");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content[0]["data"].as_str().unwrap())
        .unwrap();
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    reader.finish().unwrap();
    assert_eq!((info.width, info.height), (2, 1));
    assert_eq!(info.color_type, png::ColorType::Rgb);
    assert_eq!(&pixels[..info.buffer_size()], &[10, 20, 30, 10, 20, 30]);

    let trace = fixture.client.call("test.screenshot_trace", None).unwrap();
    let calls = trace.as_array().unwrap();
    assert!(
        calls.len() >= 3,
        "request, pending poll, completed poll: {trace}"
    );
    assert_eq!(calls[0]["method"], SCREENSHOT);
    let token = calls[0]["outcome"]["result"]["token"].as_u64().unwrap();
    let path = calls[0]["params"]["path"].as_str().unwrap();
    assert_eq!(calls[1]["outcome"]["result"], json!({ "pending": true }));
    for poll in &calls[1..] {
        assert_eq!(poll["method"], STATUS);
        assert_eq!(poll["params"]["token"], token);
    }
    assert_eq!(
        calls.last().unwrap()["outcome"]["result"],
        json!({ "pending": false, "path": path })
    );
    assert!(!Path::new(path).exists(), "published PNG was not removed");
    assert!(
        !Path::new(path).parent().unwrap().exists(),
        "private directory/staging files were not removed"
    );
}
