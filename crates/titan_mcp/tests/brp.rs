//! Real, headless BRP integration tests. No renderer, window backend, or GPU is needed.
//!
//! The HTTP plugin detaches its listener without exposing a shutdown handle. Running
//! each fixture in a child process lets the guard reap both the app and its listener,
//! including when an assertion panics. Titan fixtures use the real `TitanRemotePlugin`.

use std::{
    io::Write,
    net::{Ipv4Addr, TcpListener},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use bevy_app::{App, Last, TaskPoolPlugin, Update};
use bevy_diagnostic::{update_frame_count, FrameCount, FrameCountPlugin};
use bevy_ecs::{
    prelude::*,
    reflect::{ReflectComponent, ReflectResource},
};
use bevy_input::{
    keyboard::{KeyCode, KeyboardInput},
    mouse::{MouseButton, MouseButtonInput},
    ButtonInput, ButtonState, InputPlugin,
};
use bevy_reflect::{Reflect, TypePath};
use bevy_remote::{http::RemoteHttpPlugin, BrpReceiver, BrpResult, RemotePlugin};
use bevy_time::{Time, TimePlugin, TimeUpdateStrategy, Virtual};
use bevy_window::{CursorMoved, PrimaryWindow, Window, WindowEvent};
use serde_json::{json, Value};
use titan_mcp::{client::Client, tools};
use titan_remote::TitanRemotePlugin;

const FIXTURE_PORT: &str = "TITAN_MCP_TEST_BRP_PORT";
const FIXTURE_TITAN: &str = "TITAN_MCP_TEST_TITAN";
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
    raw_cursor_moves: u32,
    raw_cursor_x: f32,
    raw_cursor_y: f32,
    raw_cursor_window: u64,
    frame: u32,
    cursor_frame: u32,
    press_frames: Vec<[u32; 2]>,
    press_positions: Vec<Option<[f32; 2]>>,
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
    mut cursor: MessageReader<CursorMoved>,
    windows: Query<&Window>,
    mut state: ResMut<MouseState>,
) {
    state.frame += 1;
    for event in cursor.read() {
        state.raw_cursor_moves += 1;
        state.raw_cursor_x = event.position.x;
        state.raw_cursor_y = event.position.y;
        state.raw_cursor_window = event.window.to_bits();
        state.cursor_frame = state.frame;
    }
    for event in raw.read() {
        if event.state == ButtonState::Pressed {
            let frames = [state.cursor_frame, state.frame];
            state.press_frames.push(frames);
            state.press_positions.push(
                windows
                    .get(event.window)
                    .ok()
                    .and_then(Window::physical_cursor_position)
                    .map(|position| position.to_array()),
            );
        }
        state.raw.push(event.into());
    }
    state.held = input.pressed(MouseButton::Left) || input.pressed(MouseButton::Right);
    for button in [MouseButton::Left, MouseButton::Right] {
        state.presses += u32::from(input.just_pressed(button));
        state.releases += u32::from(input.just_released(button));
    }
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

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct TimeTrace {
    elapsed_ns: u64,
    last_delta_ns: u64,
    last_unpaused_delta_ns: u64,
    unpaused_updates: u64,
    updates: u64,
    first_poll_frame: u32,
    finish_frame: u32,
}

fn record_time(time: Res<Time<Virtual>>, mut trace: ResMut<TimeTrace>) {
    trace.elapsed_ns = time.elapsed().as_nanos().try_into().unwrap();
    trace.last_delta_ns = time.delta().as_nanos().try_into().unwrap();
    trace.updates += 1;
    if !time.is_paused() {
        trace.unpaused_updates += 1;
        trace.last_unpaused_delta_ns = trace.last_delta_ns;
    }
}

#[derive(Resource, Default)]
struct WrapProbe {
    armed: bool,
    first_poll_queued: bool,
    finish_hidden: bool,
}

#[derive(Resource, Default)]
struct InputFrameGate(bool);

fn arm_input_wrap(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    assert!(world.resource::<Time<Virtual>>().is_paused());
    let frame = params.unwrap()["frame"]
        .as_u64()
        .unwrap()
        .try_into()
        .unwrap();
    world.resource_mut::<FrameCount>().0 = frame;
    world.resource_mut::<InputFrameGate>().0 = true;
    Ok(json!({"frame": frame}))
}

// Only the counter's initial position is synthetic; Titan's clock, step state,
// target calculation, and status handlers are unchanged.
fn arm_wrap(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    assert!(world.resource::<Time<Virtual>>().is_paused());
    world.resource_mut::<FrameCount>().0 = u32::MAX - 2;
    *world.resource_mut::<WrapProbe>() = WrapProbe {
        armed: true,
        ..Default::default()
    };
    Ok(json!({"frame": u32::MAX - 2}))
}

fn hold_wrap_start(
    probe: Res<WrapProbe>,
    strategy: Res<TimeUpdateStrategy>,
    mut frame: ResMut<FrameCount>,
) {
    if probe.armed && !probe.first_poll_queued && matches!(*strategy, TimeUpdateStrategy::Automatic)
    {
        frame.0 = u32::MAX - 2;
    }
}

fn queue_first_step_poll(
    strategy: Res<TimeUpdateStrategy>,
    receiver: Option<Res<BrpReceiver>>,
    mut probe: ResMut<WrapProbe>,
) {
    if !probe.armed
        || probe.first_poll_queued
        || !matches!(*strategy, TimeUpdateStrategy::ManualDuration(_))
    {
        return;
    }
    // The step request has already been processed. Hold the first stepped update
    // until MCP's first real titan.status poll is queued, so it sees pending=1 at
    // MAX-1 rather than accidentally seeing completion first.
    let receiver = receiver.expect("first step poll needs the BRP mailbox");
    let deadline = Instant::now() + WAIT;
    while receiver.is_empty() {
        assert!(
            Instant::now() < deadline,
            "MCP never queued its first step poll"
        );
        bevy_tasks::tick_global_task_pools_on_main_thread();
        thread::sleep(Duration::from_millis(1));
    }
    probe.first_poll_queued = true;
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
        .register_type::<CursorMoved>()
        .add_message::<WindowEvent>()
        .add_message::<CursorMoved>()
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
    let mut secondary = Window::default();
    secondary.resolution.set_scale_factor(1.5);
    app.world_mut().spawn(secondary);
    let mut remote = RemotePlugin::default();
    if std::env::var_os(FIXTURE_TITAN).is_some() {
        app.add_plugins((TimePlugin, FrameCountPlugin, TitanRemotePlugin))
            .register_type::<TimeTrace>()
            .init_resource::<TimeTrace>()
            .init_resource::<WrapProbe>()
            .init_resource::<InputFrameGate>()
            .add_systems(Update, (record_time, queue_first_step_poll))
            .add_systems(Last, hold_wrap_start.after(update_frame_count));
        remote = remote
            .with_method_main("test.arm_wrap", arm_wrap)
            .with_method_main("test.arm_input_wrap", arm_input_wrap);
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
        if app
            .world()
            .get_resource::<InputFrameGate>()
            .is_some_and(|gate| gate.0)
        {
            // Exactly one real update per HTTP request lets input barriers start
            // at MAX and poll across zero, without relying on wall-clock timing.
            let deadline = Instant::now() + WAIT;
            while app.world().resource::<BrpReceiver>().is_empty() {
                assert!(
                    Instant::now() < deadline,
                    "input test never queued its next request"
                );
                bevy_tasks::tick_global_task_pools_on_main_thread();
                thread::sleep(Duration::from_millis(1));
            }
        }
        app.update();
        let hide_finish = app
            .world()
            .get_resource::<WrapProbe>()
            .is_some_and(|probe| probe.first_poll_queued && !probe.finish_hidden);
        if hide_finish {
            assert_eq!(app.world().resource::<FrameCount>().0, u32::MAX - 1);
            app.world_mut().resource_mut::<TimeTrace>().first_poll_frame = u32::MAX - 1;
            // Do not let any request observe the final stepped frame. Removing
            // the mailbox for exactly one update makes this deterministic even
            // if HTTP delivery or MCP's 20 ms poll sleep is unusually slow.
            // Requests remain queued in the real receiver and are processed on
            // a later paused update, after the real FrameCount wraps to zero.
            let receiver = app.world_mut().remove_resource::<BrpReceiver>().unwrap();
            app.update();
            assert_eq!(app.world().resource::<FrameCount>().0, u32::MAX);
            assert!(app.world().resource::<Time<Virtual>>().is_paused());
            app.world_mut().resource_mut::<TimeTrace>().finish_frame = u32::MAX;
            app.world_mut().insert_resource(receiver);
            app.world_mut().resource_mut::<WrapProbe>().finish_hidden = true;
        }
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

    fn time_trace(&self) -> Value {
        self.tool("get_resource", json!({"resource": "TimeTrace"}))["value"].clone()
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
    fixture.tool(
        "set_component",
        json!({"entity":window,"component":"Window","path":"resolution.scale_factor","value":1.25}),
    );
    fixture.tool("set_component", json!({"entity":window,"component":"Window","path":"resolution.scale_factor_override","value":2.0}));
    let secondary = fixture
        .query(json!({"components":[],"with":["Window"],"without":["PrimaryWindow"]}))[0]["entity"]
        .clone();
    let state = || fixture.tool("get_resource", json!({"resource":"MouseState"}))["value"].clone();
    for (index, (button, target, x, y, scale)) in [
        ("Left", window.clone(), 8.5, 12.25, 2.0),
        ("Right", secondary, 10.25, 16.5, 1.5),
    ]
    .into_iter()
    .enumerate()
    {
        let mut args = json!({"x":x,"y":y});
        if button == "Right" {
            args["button"] = json!(button);
            args["window"] = target.clone();
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
                && actual["raw_cursor_moves"] == expected
                && actual["held"] == false
        });
        let actual = state();
        let records = json!([
            {"button":button,"state":"Pressed","window":target},
            {"button":button,"state":"Released","window":target}
        ]);
        for channel in ["raw", "aggregate"] {
            assert_eq!(
                json!(actual[channel].as_array().unwrap()[index * 2..].to_vec()),
                records
            );
        }
        assert_eq!(actual["cursor_moves"], expected);
        assert_eq!(actual["cursor_x"], x);
        assert_eq!(actual["cursor_y"], y);
        assert_eq!(actual["raw_cursor_x"], x);
        assert_eq!(actual["raw_cursor_y"], y);
        assert_eq!(actual["raw_cursor_window"], target);
        // These positions were read by the raw button consumer on press,
        // exactly as legacy UI reads Window::physical_cursor_position().
        assert_eq!(
            actual["press_positions"][index],
            json!([x * scale, y * scale])
        );
        let frames = &actual["press_frames"][index];
        assert!(frames[0].as_u64().unwrap() > 0);
        assert!(frames[0].as_u64().unwrap() < frames[1].as_u64().unwrap());
    }
    // Clicking the explicit secondary window must not move the primary cursor.
    let primary = fixture.tool(
        "get_components",
        json!({"entity":window,"components":["Window"],"strict":true}),
    );
    assert_eq!(
        primary[Window::type_path()]["internal"]["physical_cursor_position"],
        json!([17.0, 24.5])
    );
}

#[test]
fn click_rejects_invalid_scale_before_mutating_cursor_or_sending_input() {
    let fixture = Fixture::start(false);
    let window = fixture.query(json!({"components": [], "with": ["Window", "PrimaryWindow"]}))[0]
        ["entity"]
        .clone();
    for (scale, x, expected) in [
        (0.0, 10.0, "positive"),
        (-2.0, 10.0, "positive"),
        (1e38, 10.0, "representable"),
    ] {
        fixture.tool("set_component", json!({"entity":window,"component":"Window","path":"resolution.scale_factor_override","value":scale}));
        let error = tools::call(&fixture.client, "click", json!({"x":x,"y":0})).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let state = fixture.tool("get_resource", json!({"resource":"MouseState"}))["value"].clone();
    assert_eq!(state["presses"], 0);
    assert_eq!(state["releases"], 0);
    assert_eq!(state["raw"], json!([]));
    assert_eq!(state["aggregate"], json!([]));
    assert_eq!(state["raw_cursor_moves"], 0);
    assert_eq!(state["cursor_moves"], 0);
    let component = fixture.tool(
        "get_components",
        json!({"entity":window,"components":["Window"],"strict":true}),
    );
    assert_eq!(
        component[Window::type_path()]["internal"]["physical_cursor_position"],
        Value::Null
    );
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

fn paused_frame(status: &Value) -> u32 {
    assert_eq!(status["paused"], true, "{status}");
    assert_eq!(status["pending_steps"], 0, "{status}");
    status["frame"].as_u64().unwrap().try_into().unwrap()
}

#[test]
fn titan_pause_freezes_real_virtual_time_but_not_frames_and_resume_advances() {
    let fixture = Fixture::start(true);
    let discovery = fixture.client.call("rpc.discover", None).unwrap();
    for name in ["titan.pause", "titan.resume", "titan.step", "titan.status"] {
        assert!(discovery["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|method| method["name"] == name));
    }
    let frame = paused_frame(&fixture.tool("pause", json!({})));
    let frozen = fixture.time_trace();
    eventually(|| {
        fixture.time_trace()["updates"].as_u64().unwrap() >= frozen["updates"].as_u64().unwrap() + 5
    });
    let status = fixture.tool("game_status", json!({}));
    assert_eq!(status["reachable"], true);
    assert_eq!(status["titan_remote_available"], true);
    assert!(paused_frame(&status["status"]).wrapping_sub(frame) >= 5);
    let still_frozen = fixture.time_trace();
    assert_eq!(still_frozen["elapsed_ns"], frozen["elapsed_ns"]);
    assert_eq!(still_frozen["unpaused_updates"], frozen["unpaused_updates"]);
    assert_eq!(still_frozen["last_delta_ns"], 0);

    let resumed = fixture.tool("resume", json!({}));
    assert_eq!(resumed["paused"], false);
    assert_eq!(resumed["pending_steps"], 0);
    eventually(|| {
        fixture.time_trace()["elapsed_ns"].as_u64().unwrap()
            > frozen["elapsed_ns"].as_u64().unwrap()
    });
    assert!(
        fixture.time_trace()["unpaused_updates"].as_u64().unwrap()
            > frozen["unpaused_updates"].as_u64().unwrap()
    );
}

#[test]
fn titan_step_runs_exact_unpaused_updates_and_real_deltas_including_defaults() {
    let fixture = Fixture::start(true);
    fixture.tool("pause", json!({}));
    for (args, frames, dt_secs) in [
        (json!({"frames": 3, "dt_secs": 0.025}), 3, 0.025_f32),
        (json!({"frames": 2}), 2, 1.0 / 60.0),
        (json!({"frames": 1, "dt_secs": 1.0}), 1, 1.0),
        (json!({"frames": 2, "dt_secs": 1e-9}), 2, 1e-9),
    ] {
        let before = fixture.time_trace();
        let completed = fixture.tool("step", args);
        paused_frame(&completed);
        let after = fixture.time_trace();
        let dt = Duration::from_secs_f32(dt_secs);
        assert_eq!(
            after["unpaused_updates"].as_u64().unwrap()
                - before["unpaused_updates"].as_u64().unwrap(),
            u64::from(frames)
        );
        assert_eq!(
            after["elapsed_ns"].as_u64().unwrap() - before["elapsed_ns"].as_u64().unwrap(),
            u64::try_from((dt * frames).as_nanos()).unwrap()
        );
        assert_eq!(after["last_unpaused_delta_ns"], json!(dt.as_nanos() as u64));
        // Keep polling after completion: app frames continue but Time<Virtual>
        // and the count of unpaused Update invocations must stay frozen.
        for _ in 0..3 {
            paused_frame(&fixture.client.call("titan.status", None).unwrap());
        }
        let later = fixture.time_trace();
        assert!(later["updates"].as_u64().unwrap() > after["updates"].as_u64().unwrap());
        assert_eq!(later["elapsed_ns"], after["elapsed_ns"]);
        assert_eq!(later["unpaused_updates"], after["unpaused_updates"]);
        assert_eq!(later["last_delta_ns"], 0);
    }
}

#[test]
fn titan_step_completes_when_first_completed_status_is_after_u32_wrap() {
    let fixture = Fixture::start(true);
    fixture.tool("pause", json!({}));
    fixture.client.call("test.arm_wrap", None).unwrap();
    let before = fixture.time_trace();
    let completed = fixture.tool("step", json!({"frames": 2, "dt_secs": 0.025}));
    let frame = paused_frame(&completed);
    let after = fixture.time_trace();
    assert_eq!(after["first_poll_frame"], u32::MAX - 1);
    assert_eq!(after["finish_frame"], u32::MAX);
    // The real target was MAX and completion happened at MAX, but no HTTP
    // request could observe that frame. Numeric frame>=target never succeeds
    // for the first completed status (or until another entire u32 cycle).
    assert!(
        frame < u32::MAX - 2,
        "status must be past the rollover: {completed}"
    );
    assert_eq!(
        after["unpaused_updates"].as_u64().unwrap() - before["unpaused_updates"].as_u64().unwrap(),
        2
    );
    assert_eq!(
        after["elapsed_ns"].as_u64().unwrap() - before["elapsed_ns"].as_u64().unwrap(),
        (Duration::from_secs_f32(0.025) * 2).as_nanos() as u64
    );
}

#[test]
fn titan_input_barriers_work_while_real_virtual_time_is_paused() {
    let fixture = Fixture::start(true);
    let frame = paused_frame(&fixture.tool("pause", json!({})));
    let frozen = fixture.time_trace();
    fixture.tool("send_key", json!({"key": "KeyW", "action": "tap"}));
    let key = fixture.tool("get_resource", json!({"resource": "KeyState"}))["value"].clone();
    // No eventual fallback: real titan.status frame barriers must separate
    // press/release. This resource query observes the release's next PreUpdate.
    assert_eq!(key, json!({"held": false, "presses": 1, "releases": 1}));

    fixture.tool("click", json!({"x": 8.5, "y": 12.25}));
    let mouse = fixture.tool("get_resource", json!({"resource": "MouseState"}))["value"].clone();
    assert_eq!(mouse["held"], false);
    assert_eq!(mouse["presses"], 1);
    assert_eq!(mouse["releases"], 1);
    assert_eq!(mouse["raw"].as_array().unwrap().len(), 2);
    assert_eq!(mouse["aggregate"], mouse["raw"]);
    assert_eq!(mouse["raw_cursor_moves"], 1);
    assert_eq!(mouse["cursor_moves"], 1);
    assert_eq!(mouse["press_positions"], json!([[8.5, 12.25]]));
    assert!(
        mouse["press_frames"][0][0].as_u64().unwrap()
            < mouse["press_frames"][0][1].as_u64().unwrap()
    );
    let status = fixture.tool("game_status", json!({}));
    assert_eq!(status["titan_remote_available"], true);
    assert!(paused_frame(&status["status"]).wrapping_sub(frame) >= 4);
    let after = fixture.time_trace();
    assert_eq!(after["elapsed_ns"], frozen["elapsed_ns"]);
    assert_eq!(after["unpaused_updates"], frozen["unpaused_updates"]);
}

#[test]
fn titan_send_key_frame_barrier_crosses_u32_wrap_while_paused() {
    let fixture = Fixture::start(true);
    fixture.tool("pause", json!({}));
    let window = fixture.query(json!({"components": [], "with": ["Window", "PrimaryWindow"]}))[0]
        ["entity"]
        .clone();
    let frozen = fixture.time_trace();
    // Explicit window avoids resolution requests. Discovery, press, then the
    // barrier's first status each run one real update: the first status is MAX.
    fixture
        .client
        .call("test.arm_input_wrap", Some(json!({"frame": u32::MAX - 3})))
        .unwrap();
    fixture.tool(
        "send_key",
        json!({"key": "KeyW", "action": "tap", "window": window}),
    );
    let key = fixture.tool("get_resource", json!({"resource": "KeyState"}))["value"].clone();
    assert_eq!(key, json!({"held": false, "presses": 1, "releases": 1}));
    // Polls at zero and one satisfy wrapping_sub(MAX) >= 2. Ordinary or
    // saturating subtraction instead times out, even though input is updating.
    assert_eq!(
        paused_frame(&fixture.client.call("titan.status", None).unwrap()),
        5
    );
    let after = fixture.time_trace();
    assert_eq!(after["elapsed_ns"], frozen["elapsed_ns"]);
    assert_eq!(after["unpaused_updates"], frozen["unpaused_updates"]);
}

#[test]
fn titan_click_cursor_frame_barrier_crosses_u32_wrap_while_paused() {
    let fixture = Fixture::start(true);
    fixture.tool("pause", json!({}));
    let window = fixture.query(json!({"components": [], "with": ["Window", "PrimaryWindow"]}))[0]
        ["entity"]
        .clone();
    let frozen = fixture.time_trace();
    // Discovery, get/mutate Window, both cursor messages, then the first status:
    // six queued requests put the cursor-to-press barrier's first status at MAX.
    fixture
        .client
        .call("test.arm_input_wrap", Some(json!({"frame": u32::MAX - 6})))
        .unwrap();
    fixture.tool("click", json!({"x": 8.5, "y": 12.25, "window": window}));
    let mouse = fixture.tool("get_resource", json!({"resource": "MouseState"}))["value"].clone();
    assert_eq!(mouse["held"], false);
    assert_eq!(mouse["presses"], 1);
    assert_eq!(mouse["releases"], 1);
    assert_eq!(mouse["raw"].as_array().unwrap().len(), 2);
    assert_eq!(mouse["aggregate"], mouse["raw"]);
    assert_eq!(mouse["press_positions"], json!([[8.5, 12.25]]));
    assert!(
        mouse["press_frames"][0][0].as_u64().unwrap()
            < mouse["press_frames"][0][1].as_u64().unwrap()
    );
    assert_eq!(
        paused_frame(&fixture.client.call("titan.status", None).unwrap()),
        11
    );
    let after = fixture.time_trace();
    assert_eq!(after["elapsed_ns"], frozen["elapsed_ns"]);
    assert_eq!(after["unpaused_updates"], frozen["unpaused_updates"]);
}
