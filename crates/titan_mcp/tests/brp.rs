//! Real, headless BRP integration tests. No renderer, window backend, or GPU is needed.
//!
//! The HTTP plugin detaches its listener without exposing a shutdown handle. Running
//! each fixture in a child process lets the guard reap both the app and its listener,
//! including when an assertion panics. Titan methods are contract stubs until #17 lands.

use std::{
    io::Write,
    net::{Ipv4Addr, TcpListener},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use bevy_app::{App, TaskPoolPlugin, Update};
use bevy_ecs::{
    prelude::*,
    reflect::{ReflectComponent, ReflectResource},
};
use bevy_input::{
    keyboard::{KeyCode, KeyboardInput},
    mouse::{MouseButton, MouseButtonInput},
    ButtonInput, InputPlugin,
};
use bevy_reflect::{Reflect, TypePath};
use bevy_remote::{http::RemoteHttpPlugin, BrpResult, RemotePlugin};
use bevy_window::{PrimaryWindow, Window, WindowEvent};
use serde_json::{json, Value};
use titan_mcp::{client::Client, tools};

const FIXTURE_PORT: &str = "TITAN_MCP_TEST_BRP_PORT";
const FIXTURE_TITAN: &str = "TITAN_MCP_TEST_TITAN_STUB";
const WAIT: Duration = Duration::from_secs(10);

#[derive(Component, Reflect)]
#[reflect(Component)]
struct TestPosition {
    x: f32,
    y: f32,
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct TestMarker {
    value: u32,
}

#[derive(Resource, Reflect)]
#[reflect(Resource)]
struct TestSettings {
    speed: f32,
}

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct KeyState {
    held: bool,
    presses: u32,
    releases: u32,
}

#[derive(Reflect)]
struct MouseRecord {
    button: String,
    state: String,
    window: u64,
}

impl From<&MouseButtonInput> for MouseRecord {
    fn from(input: &MouseButtonInput) -> Self {
        Self {
            button: format!("{:?}", input.button),
            state: format!("{:?}", input.state),
            window: input.window.to_bits(),
        }
    }
}

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct MouseState {
    held: bool,
    presses: u32,
    releases: u32,
    raw: Vec<MouseRecord>,
    aggregate: Vec<MouseRecord>,
    cursor_moves: u32,
    cursor_x: f32,
    cursor_y: f32,
}

mod first {
    use bevy_ecs::{prelude::*, reflect::ReflectComponent};
    use bevy_reflect::Reflect;

    #[derive(Component, Reflect)]
    #[reflect(Component)]
    pub struct Ambiguous {
        pub value: u32,
    }
}

mod second {
    use bevy_ecs::{prelude::*, reflect::ReflectComponent};
    use bevy_reflect::Reflect;

    #[derive(Component, Reflect)]
    #[reflect(Component)]
    pub struct Ambiguous {
        pub value: u32,
    }
}

fn record_keys(input: Res<ButtonInput<KeyCode>>, mut state: ResMut<KeyState>) {
    state.held = input.pressed(KeyCode::KeyW);
    state.presses += u32::from(input.just_pressed(KeyCode::KeyW));
    state.releases += u32::from(input.just_released(KeyCode::KeyW));
}

fn record_mouse(
    input: Res<ButtonInput<MouseButton>>,
    mut raw: MessageReader<MouseButtonInput>,
    mut aggregate: MessageReader<WindowEvent>,
    mut state: ResMut<MouseState>,
) {
    state.held = input.pressed(MouseButton::Left) || input.pressed(MouseButton::Right);
    for button in [MouseButton::Left, MouseButton::Right] {
        state.presses += u32::from(input.just_pressed(button));
        state.releases += u32::from(input.just_released(button));
    }
    state.raw.extend(raw.read().map(MouseRecord::from));
    for event in aggregate.read() {
        match event {
            WindowEvent::MouseButtonInput(input) => state.aggregate.push(input.into()),
            WindowEvent::CursorMoved(cursor) => {
                state.cursor_moves += 1;
                state.cursor_x = cursor.position.x;
                state.cursor_y = cursor.position.y;
            }
            _ => {}
        }
    }
}

#[derive(Resource)]
struct StubClock {
    paused: bool,
    frame: u64,
    pending_steps: u32,
    polls_while_pending: u32,
    last_step: Option<Value>,
}

impl StubClock {
    fn status(&self) -> Value {
        json!({"paused": self.paused, "frame": self.frame, "pending_steps": self.pending_steps})
    }
}

fn stub_pause(In(params): In<Option<Value>>, mut clock: ResMut<StubClock>) -> BrpResult {
    assert!(params.is_none() || params == Some(json!({})));
    clock.paused = true;
    Ok(clock.status())
}

fn stub_resume(In(params): In<Option<Value>>, mut clock: ResMut<StubClock>) -> BrpResult {
    assert!(params.is_none() || params == Some(json!({})));
    clock.paused = false;
    Ok(clock.status())
}

fn stub_step(In(params): In<Option<Value>>, mut clock: ResMut<StubClock>) -> BrpResult {
    let params = params.expect("titan.step requires params");
    clock.pending_steps = params["frames"]
        .as_u64()
        .expect("frames must be an integer") as u32;
    clock.paused = false;
    clock.last_step = Some(params);
    Ok(json!({"target_frame": clock.frame + u64::from(clock.pending_steps)}))
}

fn stub_status(In(_): In<Option<Value>>, mut clock: ResMut<StubClock>) -> BrpResult {
    // Deliberately make progress only when polled: returning the target immediately
    // from the tool is not equivalent to waiting for the requested frames to run.
    if clock.pending_steps > 0 {
        clock.polls_while_pending += 1;
        clock.frame += 1;
        clock.pending_steps -= 1;
        clock.paused = clock.pending_steps == 0;
    }
    Ok(clock.status())
}

fn stub_trace(In(_): In<Option<Value>>, clock: Res<StubClock>) -> BrpResult {
    Ok(json!({"polls_while_pending": clock.polls_while_pending, "last_step": clock.last_step}))
}

/// Only invoked by `Fixture::start`, never as an ordinary test.
#[test]
#[ignore = "child-process fixture, launched by the other integration tests"]
fn brp_fixture_process() {
    let Ok(port) = std::env::var(FIXTURE_PORT) else {
        return;
    };
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), InputPlugin))
        .register_type::<TestPosition>()
        .register_type::<TestMarker>()
        .register_type::<TestSettings>()
        .register_type::<KeyState>()
        .register_type::<MouseState>()
        .register_type::<MouseRecord>()
        .register_type::<MouseButtonInput>()
        .register_type::<WindowEvent>()
        .add_message::<WindowEvent>()
        .register_type::<first::Ambiguous>()
        .register_type::<second::Ambiguous>()
        .register_type::<KeyboardInput>()
        .register_type::<Window>()
        .register_type::<PrimaryWindow>()
        .insert_resource(TestSettings { speed: 2.0 })
        .init_resource::<KeyState>()
        .init_resource::<MouseState>()
        .add_systems(Update, (record_keys, record_mouse));
    app.world_mut().spawn(TestPosition { x: 1.0, y: 2.0 });
    app.world_mut()
        .spawn((TestPosition { x: 3.0, y: 4.0 }, TestMarker { value: 7 }));
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    let mut remote = RemotePlugin::default();
    if std::env::var_os(FIXTURE_TITAN).is_some() {
        app.insert_resource(StubClock {
            paused: false,
            frame: 10,
            pending_steps: 0,
            polls_while_pending: 0,
            last_step: None,
        });
        remote = remote
            .with_method_main("titan.pause", stub_pause)
            .with_method_main("titan.resume", stub_resume)
            .with_method_main("titan.step", stub_step)
            .with_method_main("titan.status", stub_status)
            .with_method_main("test.trace", stub_trace);
    }
    app.add_plugins((
        remote,
        RemoteHttpPlugin::default()
            .with_address(Ipv4Addr::LOCALHOST)
            .with_port(port.parse().expect("fixture port")),
    ));
    app.finish();
    app.cleanup();
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
    fn start(titan: bool) -> Self {
        // Use the OS-assigned ephemeral port rather than the default BRP port.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = Client::new(&format!("http://127.0.0.1:{port}")).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", "brp_fixture_process", "--nocapture"])
            .env(FIXTURE_PORT, port.to_string())
            .env_remove(FIXTURE_TITAN)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if titan {
            command.env(FIXTURE_TITAN, "1");
        }
        // RemoteHttpPlugin owns binding; release the reservation immediately
        // before launching the child, keeping the unavoidable race small.
        drop(listener);
        let child = command.spawn().unwrap();
        let mut fixture = Self { child, client };
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = fixture.child.try_wait().unwrap() {
                panic!("headless BRP fixture exited before readiness: {status}");
            }
            match fixture.client.call("rpc.discover", None) {
                Ok(_) => return fixture,
                Err(error) => assert!(
                    Instant::now() < deadline,
                    "headless BRP fixture at port {port} did not become ready: {error}"
                ),
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn tool(&self, name: &str, args: Value) -> Value {
        tools::call(&self.client, name, args)
            .unwrap_or_else(|error| panic!("{name} failed: {error}"))
    }

    fn query(&self, args: Value) -> Vec<Value> {
        self.tool("query_entities", args)
            .as_array()
            .expect("query rows")
            .clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn eventually(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the app state"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn query_mutate_insert_remove_and_filters() {
    let fixture = Fixture::start(false);
    let rows = fixture.query(json!({"components": ["TestPosition"], "without": ["TestMarker"]}));
    assert_eq!(rows.len(), 1);
    let entity = rows[0]["entity"].clone();
    assert_eq!(
        rows[0]["components"][TestPosition::type_path()],
        json!({"x": 1.0, "y": 2.0})
    );

    fixture.tool(
        "set_component",
        json!({"entity": entity, "component": "TestPosition", "path": "x", "value": 9.0}),
    );
    let fetched = fixture.tool(
        "get_components",
        json!({"entity": entity, "components": ["TestPosition"], "strict": true}),
    );
    assert_eq!(fetched[TestPosition::type_path()]["x"], 9.0);

    fixture.tool(
        "insert_components",
        json!({"entity": entity, "components": {"TestMarker": {"value": 42}}}),
    );
    let rows = fixture
        .query(json!({"components": ["TestPosition", "TestMarker"], "with": ["TestMarker"]}));
    assert_eq!(rows.len(), 2);
    let truncated = fixture.tool(
        "query_entities",
        json!({"components": ["TestPosition"], "limit": 1}),
    );
    assert_eq!(truncated["items"].as_array().unwrap().len(), 1);
    assert_eq!(truncated["omitted"], 1);
    assert!(truncated["note"].as_str().unwrap().contains("narrow"));
    let changed = rows.iter().find(|row| row["entity"] == entity).unwrap();
    assert_eq!(changed["components"][TestMarker::type_path()]["value"], 42);
    let listed = fixture.tool("list_components", json!({"entity": entity}));
    assert!(listed
        .as_array()
        .unwrap()
        .contains(&json!(TestMarker::type_path())));

    fixture.tool(
        "remove_components",
        json!({"entity": entity, "components": ["TestMarker"]}),
    );
    let listed = fixture.tool("list_components", json!({"entity": entity}));
    assert!(!listed
        .as_array()
        .unwrap()
        .contains(&json!(TestMarker::type_path())));
    assert_eq!(
        fixture
            .query(json!({"components": ["TestPosition"], "without": ["TestMarker"]}))
            .len(),
        1
    );
}

#[test]
fn spawn_and_despawn_entities() {
    let fixture = Fixture::start(false);
    let spawned = fixture.tool(
        "spawn_entity",
        json!({"components": {"TestPosition": {"x": 8.0, "y": 6.0}, "TestMarker": {"value": 99}}}),
    );
    let entity = spawned["entity"].clone();
    assert!(
        entity.is_number(),
        "spawn should return an entity ID: {spawned}"
    );
    let rows = fixture.query(json!({"components": ["TestPosition", "TestMarker"]}));
    let row = rows.iter().find(|row| row["entity"] == entity).unwrap();
    assert_eq!(row["components"][TestMarker::type_path()]["value"], 99);
    fixture.tool("despawn_entity", json!({"entity": entity}));
    assert!(!fixture
        .query(json!({"components": ["TestPosition"]}))
        .iter()
        .any(|row| row["entity"] == entity));
    let error =
        tools::call(&fixture.client, "despawn_entity", json!({"entity": entity})).unwrap_err();
    assert!(
        error.contains("entity") || error.contains("Entity"),
        "{error}"
    );
}

#[test]
fn resource_crud_and_raw_brp_escape_hatch() {
    let fixture = Fixture::start(false);
    let listed = fixture.tool("list_resources", json!({}));
    assert!(listed
        .as_array()
        .unwrap()
        .contains(&json!(TestSettings::type_path())));
    assert_eq!(
        fixture.tool("get_resource", json!({"resource": "TestSettings"}))["value"]["speed"],
        2.0
    );
    fixture.tool(
        "set_resource",
        json!({"resource": "TestSettings", "path": "speed", "value": 5.0}),
    );
    assert_eq!(
        fixture.tool("get_resource", json!({"resource": "TestSettings"}))["value"]["speed"],
        5.0
    );
    fixture.tool("brp_call", json!({"method": "world.remove_resources", "params": {"resource": TestSettings::type_path()}}));
    // list_resources enumerates registered resource metadata, not live values.
    let error = tools::call(
        &fixture.client,
        "get_resource",
        json!({"resource": "TestSettings"}),
    )
    .unwrap_err();
    assert!(error.contains("TestSettings"), "{error}");
    fixture.tool(
        "set_resource",
        json!({"resource": "TestSettings", "value": {"speed": 6.0}}),
    );
    assert_eq!(
        fixture.tool("get_resource", json!({"resource": "TestSettings"}))["value"]["speed"],
        6.0
    );
    fixture.tool("brp_call", json!({"method": "world.insert_resources", "params": {"resource": TestSettings::type_path(), "value": {"speed": 7.0}}}));
    assert_eq!(
        fixture.tool("get_resource", json!({"resource": "TestSettings"}))["value"]["speed"],
        7.0
    );
}

#[test]
fn send_key_changes_real_button_input() {
    let fixture = Fixture::start(false);
    let state = || fixture.tool("get_resource", json!({"resource": "KeyState"}))["value"].clone();
    assert_eq!(state(), json!({"held": false, "presses": 0, "releases": 0}));
    // Omitting the window exercises resolution of the reflected PrimaryWindow.
    fixture.tool("send_key", json!({"key": "KeyW", "action": "press"}));
    eventually(|| state()["held"] == true);
    assert_eq!(state()["presses"], 1);
    let window = fixture.query(json!({"components": [], "with": ["Window", "PrimaryWindow"]}))[0]
        ["entity"]
        .clone();
    fixture.tool(
        "send_key",
        json!({"key": "KeyW", "action": "release", "window": window}),
    );
    eventually(|| state()["held"] == false && state()["releases"] == 1);
    fixture.tool(
        "send_key",
        json!({"key": "KeyW", "action": "tap", "window": window}),
    );
    eventually(|| state()["presses"] == 2 && state()["releases"] == 2 && state()["held"] == false);
}

#[test]
fn click_updates_button_input_and_both_message_consumers() {
    let fixture = Fixture::start(false);
    let window = fixture.query(json!({"components": [], "with": ["Window", "PrimaryWindow"]}))[0]
        ["entity"]
        .clone();
    let state = || fixture.tool("get_resource", json!({"resource":"MouseState"}))["value"].clone();
    for (index, button) in ["Left", "Right"].into_iter().enumerate() {
        let mut args = json!({"x":8.5,"y":12.25});
        if button == "Right" {
            args["button"] = json!(button);
            args["window"] = window.clone();
        }
        // First click exercises defaults and primary-window resolution.
        fixture.tool("click", args);
        let expected = index + 1;
        eventually(|| {
            let actual = state();
            actual["presses"] == expected
                && actual["releases"] == expected
                && actual["raw"].as_array().unwrap().len() == expected * 2
                && actual["aggregate"].as_array().unwrap().len() == expected * 2
                && actual["held"] == false
        });
        let actual = state();
        let records = json!([
            {"button":button,"state":"Pressed","window":window},
            {"button":button,"state":"Released","window":window}
        ]);
        for channel in ["raw", "aggregate"] {
            assert_eq!(
                json!(actual[channel].as_array().unwrap()[index * 2..].to_vec()),
                records
            );
        }
        assert_eq!(actual["cursor_moves"], expected);
        assert_eq!(actual["cursor_x"], 8.5);
        assert_eq!(actual["cursor_y"], 12.25);
    }
}

#[test]
fn find_types_and_short_name_resolution_report_ambiguity() {
    let fixture = Fixture::start(false);
    let found = fixture.tool("find_types", json!({"query": "TestPosition"}));
    assert!(
        found.to_string().contains(TestPosition::type_path()),
        "{found}"
    );
    assert!(
        !found.to_string().contains(TestMarker::type_path()),
        "search should be filtered: {found}"
    );
    let found = fixture.tool("find_types", json!({"query": "Ambiguous"}));
    assert!(
        found.to_string().contains(first::Ambiguous::type_path()),
        "{found}"
    );
    assert!(
        found.to_string().contains(second::Ambiguous::type_path()),
        "{found}"
    );
    let limited = fixture.tool("find_types", json!({"query": "ambiguous", "limit": 1}));
    assert_eq!(limited["types"].as_array().unwrap().len(), 1);
    assert_eq!(limited["total"], 2);
    assert_eq!(limited["omitted"], 1);
    assert!(!limited["note"].as_str().unwrap().is_empty());
    let empty = fixture.tool("find_types", json!({"query": "DefinitelyNotRegistered"}));
    assert_eq!(empty["types"], json!([]));
    let error = tools::call(
        &fixture.client,
        "query_entities",
        json!({"components": ["Ambiguous"]}),
    )
    .unwrap_err();
    assert!(error.to_lowercase().contains("ambiguous"), "{error}");
    assert!(error.contains(first::Ambiguous::type_path()), "{error}");
    assert!(error.contains(second::Ambiguous::type_path()), "{error}");
    assert!(fixture
        .query(json!({"components": [first::Ambiguous::type_path()]}))
        .is_empty());
    let error = tools::call(
        &fixture.client,
        "query_entities",
        json!({"components": ["DefinitelyNotRegistered"]}),
    )
    .unwrap_err();
    assert!(error.contains("DefinitelyNotRegistered"), "{error}");
    assert!(
        error.contains("register") || error.contains("Reflect"),
        "{error}"
    );
}

#[test]
fn missing_titan_methods_have_actionable_errors() {
    let fixture = Fixture::start(false);
    let status = fixture.tool("game_status", json!({}));
    assert_eq!(status["reachable"], true);
    assert_eq!(status["titan_remote_available"], false);
    assert_eq!(status["status"], Value::Null);
    for (name, args) in [
        ("pause", json!({})),
        ("resume", json!({})),
        ("step", json!({"frames": 1})),
    ] {
        let error = tools::call(&fixture.client, name, args).unwrap_err();
        assert!(error.contains("TitanRemotePlugin"), "{name}: {error}");
        assert!(
            error.contains("titan.") || error.contains(name),
            "{name}: {error}"
        );
    }
}

#[test]
fn binary_stdio_tools_call_queries_the_live_headless_app() {
    let fixture = Fixture::start(false);
    let mut child = Command::new(env!("CARGO_BIN_EXE_titan_mcp"))
        .args(["--url", fixture.client.url()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for request in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {},
            "clientInfo": {"name": "brp-integration-test", "version": "1"}
        }}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "query_entities", "arguments": {"components": ["TestPosition"]}
        }}),
    ] {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 3);
    for (index, response) in responses.iter().enumerate() {
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], index + 1);
        assert!(response.get("error").is_none(), "{response}");
    }
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "titan_mcp");
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert!(responses[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "query_entities"));
    let result = &responses[2]["result"];
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["content"].as_array().unwrap().len(), 1);
    assert_eq!(result["content"][0]["type"], "text");
    let rows: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let mut positions: Vec<Value> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            assert!(row["entity"].is_number(), "{row}");
            row["components"][TestPosition::type_path()].clone()
        })
        .collect();
    positions.sort_by(|a, b| {
        a["x"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["x"].as_f64().unwrap())
    });
    assert_eq!(
        positions,
        [json!({"x": 1.0, "y": 2.0}), json!({"x": 3.0, "y": 4.0})]
    );
}

#[test]
fn titan_pause_resume_and_step_follow_issue_17_contract() {
    let fixture = Fixture::start(true);
    let paused = fixture.tool("pause", json!({}));
    assert_eq!(
        paused,
        json!({"paused": true, "frame": 10, "pending_steps": 0})
    );
    let resumed = fixture.tool("resume", json!({}));
    assert_eq!(
        resumed,
        json!({"paused": false, "frame": 10, "pending_steps": 0})
    );
    fixture.tool("pause", json!({}));
    let stepped = fixture.tool("step", json!({"frames": 3, "dt_secs": 0.025}));
    assert_eq!(stepped["paused"], true);
    assert_eq!(stepped["frame"], 13);
    assert_eq!(stepped["pending_steps"], 0);
    let trace = fixture.client.call("test.trace", None).unwrap();
    assert_eq!(trace["polls_while_pending"], 3);
    assert_eq!(trace["last_step"], json!({"frames": 3, "dt_secs": 0.025}));
    let stepped = fixture.tool("step", json!({"frames": 2}));
    assert_eq!(
        stepped,
        json!({"paused": true, "frame": 15, "pending_steps": 0})
    );
    let trace = fixture.client.call("test.trace", None).unwrap();
    assert_eq!(trace["polls_while_pending"], 5);
    assert_eq!(trace["last_step"], json!({"frames": 2}));
    let status = fixture.tool("game_status", json!({}));
    assert_eq!(status["reachable"], true);
    assert_eq!(status["titan_remote_available"], true);
    assert_eq!(status["status"], stepped);
}
