//! Real loopback BRP against the demo's gameplay and remote plugins, without a GPU.
#![cfg(feature = "remote")]

use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use bevy::{prelude::*, time::TimeUpdateStrategy};
use serde_json::{json, Value};
use titan_doom::{remote::DoomRemotePlugin, GameplayPlugin, Level, FIXED_HZ};
use ureq::Agent;

const TIMEOUT: Duration = Duration::from_secs(20);

// The HTTP plugin owns a detached listener. Isolate it in a child so dropping
// the harness also releases the socket, even on assertion failure.
struct Server {
    child: Child,
    client: Agent,
    url: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn new() -> Self {
        // RemoteHttpPlugin accepts a port, not a reserved listener; there is a
        // small reservation gap. Readiness below is bounded and checks the child.
        let port = TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "headless_server", "--ignored", "--nocapture"])
            .env("DOOM_TEST_BRP_PORT", port.to_string())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut server = Self {
            child,
            client: Agent::new_with_config(
                Agent::config_builder()
                    .proxy(None)
                    .timeout_global(Some(TIMEOUT))
                    .build(),
            ),
            url: format!("http://127.0.0.1:{port}"),
        };
        let deadline = Instant::now() + TIMEOUT;
        loop {
            assert!(server.child.try_wait().unwrap().is_none(), "server exited");
            if server.request("rpc.discover", json!({})).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline, "BRP did not become ready");
            thread::sleep(Duration::from_millis(10));
        }
        server
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, ureq::Error> {
        self.client
            .post(&self.url)
            .send_json(json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))?
            .body_mut()
            .read_json()
    }

    fn call(&self, method: &str, params: Value) -> Value {
        let response = self.request(method, params).unwrap();
        assert_eq!(response["id"], 1, "{response}");
        assert!(response.get("error").is_none(), "{method}: {response}");
        response.get("result").unwrap().clone()
    }

    fn resource(&self, name: &str) -> Value {
        self.call("world.get_resources", json!({"resource": name}))["value"].clone()
    }

    fn actions(&self, value: Value) {
        self.call(
            "world.insert_resources",
            json!({"resource": "titan_doom::GameplayActions", "value": value}),
        );
    }

    fn step(&self, frames: u32, dt_secs: f32) {
        self.call("titan.step", json!({"frames": frames, "dt_secs": dt_secs}));
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let status = self.call("titan.status", json!({}));
            if status["pending_steps"] == 0 {
                assert_eq!(status["paused"], true);
                return;
            }
            assert!(Instant::now() < deadline, "step did not finish: {status}");
            thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
#[ignore = "child-process fixture; started by the BRP integration test"]
fn headless_server() {
    let port = std::env::var("DOOM_TEST_BRP_PORT")
        .expect("fixture requires a port")
        .parse()
        .unwrap();
    let mut app = App::new();
    app.add_plugins(
        MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(
            Duration::from_millis(1),
        )),
    )
    .insert_resource(Level::demo())
    .insert_resource(Time::<Fixed>::from_hz(FIXED_HZ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / FIXED_HZ,
    )))
    .add_plugins((GameplayPlugin, DoomRemotePlugin { port }));
    app.run();
}

#[test]
fn brp_actions_pause_and_fixed_steps() {
    let server = Server::new();
    let initial = server.resource("titan_doom::PlayerState");
    assert_eq!(initial["tick"], 0, "launch must start paused");
    assert_eq!(server.call("titan.status", json!({}))["paused"], true);

    // Nested object and event reflection must serialize over BRP, not just
    // appear in the type registry. Restart supplies a nonempty event list.
    let combat = server.resource("titan_doom::combat::CombatState");
    assert_eq!(combat["objects"].as_array().unwrap().len(), 10);
    assert_eq!(combat["phase"], "Playing");
    server.actions(json!({
        "movement": [0.0, 0.0], "look_delta": [0.0, 0.0],
        "fire": false, "interact": false, "restart": true
    }));
    server.step(1, 1.0 / 60.0);
    assert_eq!(server.resource("titan_doom::PlayerState")["tick"], 0);
    assert_eq!(
        server.resource("titan_doom::combat::CombatState")["events"][0]["outcome"],
        "Restarted"
    );

    server.actions(json!({
        "movement": [0.0, 1.0], "look_delta": [0.0, 0.0],
        "fire": false, "interact": false, "restart": false
    }));
    // Default Titan dt is a rounded f32 1/60: each frame runs one fixed tick.
    server.step(6, 1.0 / 60.0);
    let moved = server.resource("titan_doom::PlayerState");
    assert_eq!(moved["tick"], 6);
    let before = initial["position"].as_array().unwrap();
    let after = moved["position"].as_array().unwrap();
    let distance = ((after[0].as_f64().unwrap() - before[0].as_f64().unwrap()).powi(2)
        + (after[1].as_f64().unwrap() - before[1].as_f64().unwrap()).powi(2))
    .sqrt();
    assert!((distance - 0.3).abs() < 0.001, "{initial} -> {moved}");

    // Paused frames still service BRP but neither ticks nor held actions run.
    let status = server.call("titan.pause", json!({}));
    let frame = status["frame"].as_u64().unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while server.call("titan.status", json!({}))["frame"]
        .as_u64()
        .unwrap()
        <= frame + 3
    {
        assert!(Instant::now() < deadline);
    }
    assert_eq!(server.resource("titan_doom::PlayerState"), moved);

    // A Titan step is an app frame, not a gameplay tick: two half-dt frames
    // run one tick, and one double-dt frame runs two. Fixed overstep persists.
    server.step(2, 1.0 / 120.0);
    assert_eq!(server.resource("titan_doom::PlayerState")["tick"], 7);
    server.step(1, 1.0 / 30.0);
    assert_eq!(server.resource("titan_doom::PlayerState")["tick"], 9);

    server.actions(json!({
        "movement": [0.0, 0.0], "look_delta": [0.0, 0.0],
        "fire": false, "interact": false, "restart": false
    }));
    assert_eq!(server.call("titan.resume", json!({}))["paused"], false);
    let deadline = Instant::now() + TIMEOUT;
    while server.resource("titan_doom::PlayerState")["tick"]
        .as_u64()
        .unwrap()
        <= 9
    {
        assert!(Instant::now() < deadline, "resume did not advance gameplay");
    }
    assert_eq!(server.call("titan.pause", json!({}))["paused"], true);
}
