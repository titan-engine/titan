//! Drives `titan_remote` over a real `RemoteHttpPlugin` socket, with no window or GPU.
//!
//! The app is updated manually so frame counts are exact. Each request is sent by a client thread
//! and then waited for in the BRP mailbox ([`BrpReceiver`]), so the request is processed by the
//! very next `App::update` and never by a frame the test did not intend.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use bevy_app::{App, TaskPoolPlugin};
use bevy_diagnostic::{FrameCount, FrameCountPlugin};
use bevy_remote::{error_codes, http::RemoteHttpPlugin, BrpReceiver, RemotePlugin};
use bevy_time::{Time, TimePlugin, TimeUpdateStrategy, Virtual};
use serde_json::{json, Value};
use titan_remote::TitanRemotePlugin;

/// Upper bound for any single wait, so a regression fails instead of hanging CI.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The clock delta the harness installs before a test touches stepping.
const BASE_DELTA: Duration = Duration::from_millis(20);

/// A JSON-RPC error as returned over HTTP.
#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

/// An app serving BRP over HTTP on a private loopback port.
struct Harness {
    app: App,
    addr: SocketAddr,
    next_id: u64,
}

impl Harness {
    /// A headless app with BRP over HTTP and the Titan methods.
    fn new() -> Self {
        Self::with_plugins(|_| {})
    }

    /// Like [`Harness::new`], but lets the test adjust the app before plugins are finished.
    fn with_plugins(configure: impl FnOnce(&mut App)) -> Self {
        let port = free_port();
        let mut app = App::new();
        configure(&mut app);
        app.add_plugins((
            TaskPoolPlugin::default(),
            TimePlugin,
            FrameCountPlugin,
            RemotePlugin::default(),
            RemoteHttpPlugin::default().with_port(port),
            TitanRemotePlugin,
        ));
        app.finish();
        app.cleanup();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(BASE_DELTA));
        // Runs the startup schedules, which create the mailbox and start the HTTP server.
        app.update();
        Self {
            app,
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            next_id: 1,
        }
    }

    /// Sends one JSON-RPC request over HTTP and returns the full response envelope.
    ///
    /// Exactly one app update runs, and it is the one that processes this request.
    fn envelope(&mut self, method: &str, params: Option<Value>) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if let Some(params) = params {
            request["params"] = params;
        }

        let addr = self.addr;
        let (sender, receiver) = mpsc::channel();
        let client = thread::spawn(move || {
            let _ = sender.send(http_post(addr, &request.to_string()));
        });

        // Updating before the request is queued would let it land in a later frame.
        let deadline = Instant::now() + TIMEOUT;
        while self.app.world().resource::<BrpReceiver>().is_empty() {
            if let Ok(early) = receiver.try_recv() {
                panic!("`{method}` got a response before it was processed: {early}");
            }
            assert!(
                Instant::now() < deadline,
                "`{method}` never reached the BRP mailbox"
            );
            tick_task_pools();
            thread::sleep(Duration::from_millis(1));
        }
        self.app.update();
        assert!(
            self.app.world().resource::<BrpReceiver>().is_empty(),
            "`{method}` left requests in the mailbox"
        );

        // The server task writes the response only once the task pools are ticked again.
        let deadline = Instant::now() + TIMEOUT;
        let body = loop {
            match receiver.try_recv() {
                Ok(body) => break body,
                Err(error) => {
                    assert!(
                        error == mpsc::TryRecvError::Empty && Instant::now() < deadline,
                        "`{method}` got no HTTP response: {error}"
                    );
                    tick_task_pools();
                    thread::sleep(Duration::from_millis(1));
                }
            }
        };
        client.join().unwrap();
        let envelope: Value = serde_json::from_str(&body)
            .unwrap_or_else(|e| panic!("`{method}` response is not JSON ({e}): {body}"));
        assert_eq!(envelope["jsonrpc"], "2.0", "{envelope}");
        assert_eq!(envelope["id"], id, "{envelope}");
        envelope
    }

    /// Sends a request and splits the response into its `result` or `error`.
    fn call(&mut self, method: &str, params: Option<Value>) -> Result<Value, RpcError> {
        let envelope = self.envelope(method, params);
        match (envelope.get("result"), envelope.get("error")) {
            (Some(result), None) => Ok(result.clone()),
            (None, Some(error)) => Err(RpcError {
                code: error["code"].as_i64().unwrap(),
                message: error["message"].as_str().unwrap().to_owned(),
            }),
            _ => panic!("response needs exactly one of `result` and `error`: {envelope}"),
        }
    }

    /// Like [`Harness::call`], for requests that must succeed.
    fn ok(&mut self, method: &str, params: Option<Value>) -> Value {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("`{method}` failed: {e:?}"))
    }

    /// Runs `count` frames that carry no request.
    fn update(&mut self, count: u32) {
        for _ in 0..count {
            self.app.update();
        }
    }

    fn frame(&self) -> u32 {
        self.app.world().resource::<FrameCount>().0
    }

    fn elapsed(&self) -> Duration {
        self.app.world().resource::<Time<Virtual>>().elapsed()
    }

    fn paused(&self) -> bool {
        self.app.world().resource::<Time<Virtual>>().is_paused()
    }
}

/// Runs pending server tasks without advancing a frame.
///
/// Without `bevy_tasks/multi_threaded` (the dev-dependency does not enable it), spawned tasks only
/// run when the pools are ticked on the thread that spawned them, which `App::update` would do at
/// the cost of a frame. With it, this is a harmless no-op.
fn tick_task_pools() {
    bevy::tasks::tick_global_task_pools_on_main_thread();
}

/// An unused loopback port. The OS does not hand the same ephemeral port out twice in a row, so
/// tests running in parallel do not collide in practice.
fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A minimal HTTP/1.1 client: one `POST /` with a JSON body, returning the response body.
///
/// Connecting is retried because the server binds on a background task after the first update.
fn http_post(addr: SocketAddr, body: &str) -> String {
    let deadline = Instant::now() + TIMEOUT;
    let mut stream = loop {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(250)) {
            Ok(stream) => break stream,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "could not connect to {addr}: {error}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }
    };
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    write!(
        stream,
        "POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let response = String::from_utf8(response).unwrap();
    let (head, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed HTTP response: {response:?}"));
    let head = head.to_ascii_lowercase();
    assert!(
        head.starts_with("http/1.1 200"),
        "unexpected response: {head}"
    );
    assert!(
        head.contains("content-type: application/json"),
        "unexpected response: {head}"
    );
    if head.contains("transfer-encoding: chunked") {
        decode_chunked(body)
    } else {
        body.to_owned()
    }
}

/// Decodes an HTTP/1.1 chunked body, without trailers or chunk extensions.
fn decode_chunked(mut rest: &str) -> String {
    let mut decoded = String::new();
    loop {
        let (size, after) = rest
            .split_once("\r\n")
            .unwrap_or_else(|| panic!("malformed chunk header: {rest:?}"));
        let size = usize::from_str_radix(size.trim(), 16)
            .unwrap_or_else(|e| panic!("bad chunk size {size:?}: {e}"));
        if size == 0 {
            return decoded;
        }
        decoded.push_str(&after[..size]);
        rest = after[size..]
            .strip_prefix("\r\n")
            .unwrap_or_else(|| panic!("chunk is not terminated: {rest:?}"));
    }
}

fn status(paused: bool, frame: u32, pending_steps: u32) -> Value {
    json!({ "paused": paused, "frame": frame, "pending_steps": pending_steps })
}

#[test]
fn status_reports_the_frame_that_processed_the_request() {
    let mut harness = Harness::new();
    for params in [None, Some(Value::Null), Some(json!({}))] {
        // `frame` is read after the request frame's `Last`, so it is the frame count after that update.
        let result = harness.ok("titan.status", params);
        assert_eq!(result, status(false, harness.frame(), 0));
    }
    let frame = harness.frame();
    harness.update(3);
    assert_eq!(
        harness.ok("titan.status", None),
        status(false, frame + 4, 0)
    );
}

#[test]
fn pause_freezes_virtual_time_and_resume_restarts_it() {
    let mut harness = Harness::new();
    harness.ok("titan.status", None);

    let elapsed = harness.elapsed();
    let result = harness.ok("titan.pause", None);
    assert_eq!(result["paused"], true);
    assert_eq!(result["pending_steps"], 0);
    assert!(harness.paused());

    // The pause request's own frame already ran with a live clock; later frames do not.
    let frozen = harness.elapsed();
    assert!(frozen >= elapsed);
    let frame = harness.frame();
    harness.update(5);
    assert_eq!(harness.elapsed(), frozen);
    assert_eq!(harness.frame(), frame + 5, "frames continue while paused");
    assert_eq!(harness.ok("titan.status", None), status(true, frame + 6, 0));
    assert_eq!(harness.elapsed(), frozen);

    // Pausing twice is harmless.
    assert_eq!(harness.ok("titan.pause", None)["paused"], true);

    let result = harness.ok("titan.resume", None);
    assert_eq!(result["paused"], false);
    assert!(!harness.paused());
    harness.update(1);
    assert_eq!(harness.elapsed() - frozen, BASE_DELTA);
    assert_eq!(harness.ok("titan.resume", None)["paused"], false);
}

#[test]
fn step_advances_exactly_n_frames_and_pauses() {
    let mut harness = Harness::new();
    let dt_secs = 0.025_f32;
    let dt = Duration::from_secs_f32(dt_secs);

    // Non-default clock configuration that stepping must put back afterwards.
    {
        let mut time = harness.app.world_mut().resource_mut::<Time<Virtual>>();
        time.set_relative_speed_f64(0.25);
        time.set_max_delta(Duration::from_millis(1));
    }
    harness.ok("titan.pause", None);

    let frame = harness.frame();
    let elapsed = harness.elapsed();
    let result = harness.ok(
        "titan.step",
        Some(json!({ "frames": 10, "dt_secs": dt_secs })),
    );
    // The step request's frame was not part of the step.
    assert_eq!(harness.frame(), frame + 1);
    assert_eq!(result, json!({ "target_frame": frame + 1 + 10 }));
    assert_eq!(harness.elapsed(), elapsed);

    let start = harness.frame();
    for n in 1..=10 {
        harness.update(1);
        assert_eq!(harness.frame(), start + n);
        assert_eq!(
            harness.elapsed() - elapsed,
            dt * n,
            "after stepped frame {n}"
        );
    }
    assert_eq!(harness.frame(), result["target_frame"]);

    // Done: paused, nothing pending, and further frames do not move the clock.
    assert!(harness.paused());
    harness.update(2);
    assert_eq!(harness.elapsed() - elapsed, dt * 10);
    let after = harness.ok("titan.status", None);
    assert_eq!(after, status(true, harness.frame(), 0));
    assert_eq!(harness.elapsed() - elapsed, dt * 10);

    let time = harness.app.world().resource::<Time<Virtual>>();
    assert_eq!(time.relative_speed_f64(), 0.25);
    assert_eq!(time.max_delta(), Duration::from_millis(1));
    assert!(matches!(
        harness.app.world().resource::<TimeUpdateStrategy>(),
        TimeUpdateStrategy::ManualDuration(d) if *d == BASE_DELTA
    ));
}

#[test]
fn step_defaults_to_one_sixtieth_of_a_second() {
    let mut harness = Harness::new();
    let elapsed = harness.elapsed();
    let result = harness.ok("titan.step", Some(json!({ "frames": 2 })));
    assert_eq!(result["target_frame"], harness.frame() + 2);
    // The request frame itself ran at the harness' base delta, before the step began.
    let before = harness.elapsed();
    assert!(before >= elapsed);
    harness.update(2);
    assert_eq!(
        harness.elapsed() - before,
        Duration::from_secs_f32(1.0 / 60.0) * 2
    );
    assert!(harness.paused());
}

#[test]
fn polling_status_counts_down_over_http() {
    let mut harness = Harness::new();
    harness.ok("titan.pause", None);
    let elapsed = harness.elapsed();
    let target = harness.ok("titan.step", Some(json!({ "frames": 4, "dt_secs": 0.01 })))
        ["target_frame"]
        .as_u64()
        .unwrap();
    assert_eq!(
        harness.ok("titan.status", None)["pending_steps"],
        3,
        "a polling request's own frame is a stepped frame"
    );
    assert_eq!(harness.ok("titan.status", None)["pending_steps"], 2);
    assert_eq!(harness.ok("titan.status", None)["pending_steps"], 1);
    let done = harness.ok("titan.status", None);
    assert_eq!(done, status(true, target as u32, 0));
    assert_eq!(harness.elapsed() - elapsed, Duration::from_millis(40));
}

#[test]
fn a_second_step_is_rejected_until_the_first_is_cancelled() {
    let mut harness = Harness::new();
    harness.ok("titan.step", Some(json!({ "frames": 5 })));
    let error = harness
        .call("titan.step", Some(json!({ "frames": 1 })))
        .unwrap_err();
    assert_eq!(error.code, i64::from(titan_remote::STEP_IN_PROGRESS));
    assert!(error.message.contains("pending"), "{}", error.message);

    // Pausing cancels the step; the next step is accepted.
    assert_eq!(harness.ok("titan.pause", None)["pending_steps"], 0);
    harness.ok("titan.step", Some(json!({ "frames": 1 })));
    // Resuming cancels it too and leaves the clock running.
    let result = harness.ok("titan.resume", None);
    assert_eq!(result["pending_steps"], 0);
    assert_eq!(result["paused"], false);
}

#[test]
fn zero_frame_step_pauses_immediately() {
    let mut harness = Harness::new();
    let frame = harness.frame();
    let result = harness.ok("titan.step", Some(json!({ "frames": 0 })));
    assert_eq!(result["target_frame"], frame + 1);
    assert!(harness.paused());
}

#[test]
fn invalid_params_return_invalid_params_and_change_nothing() {
    let mut harness = Harness::new();
    let invalid_step_params = [
        None,
        Some(json!({})),
        Some(json!([1, 2])),
        Some(json!([1, 0.1])),
        Some(json!({ "frames": -1 })),
        Some(json!({ "frames": 1.5 })),
        Some(json!({ "frames": "3" })),
        Some(json!({ "frames": 4_294_967_296_u64 })),
        Some(json!({ "frames": 1, "dt_secs": 0 })),
        Some(json!({ "frames": 1, "dt_secs": -0.1 })),
        Some(json!({ "frames": 1, "dt_secs": 2 })),
        Some(json!({ "frames": 1, "dt_secs": 1e-30 })),
        Some(json!({ "frames": 1, "dt_secs": "fast" })),
        Some(json!({ "frames": 1, "unexpected": true })),
    ];
    for params in invalid_step_params {
        let error = harness.call("titan.step", params.clone()).unwrap_err();
        assert_eq!(
            error.code,
            i64::from(error_codes::INVALID_PARAMS),
            "titan.step with {params:?}: {}",
            error.message
        );
    }
    for method in ["titan.status", "titan.pause", "titan.resume"] {
        for params in [json!({ "unexpected": true }), json!([]), json!(1)] {
            let error = harness.call(method, Some(params.clone())).unwrap_err();
            assert_eq!(
                error.code,
                i64::from(error_codes::INVALID_PARAMS),
                "{method} with {params}: {}",
                error.message
            );
        }
    }

    // Nothing was changed by the rejected calls.
    assert!(!harness.paused());
    let result = harness.ok("titan.status", None);
    assert_eq!(result, status(false, harness.frame(), 0));
}

#[test]
fn unknown_titan_method_is_not_found() {
    let mut harness = Harness::new();
    let error = harness.call("titan.nope", None).unwrap_err();
    assert_eq!(error.code, i64::from(error_codes::METHOD_NOT_FOUND));
}

#[test]
fn builtin_methods_still_work_beside_titan_methods() {
    let mut harness = Harness::new();

    let discover = harness.ok("rpc.discover", None);
    let names: Vec<&str> = discover["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|method| method["name"].as_str().unwrap())
        .collect();
    for expected in [
        "rpc.discover",
        "world.query",
        "world.list_components",
        "titan.status",
        "titan.pause",
        "titan.resume",
        "titan.step",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} is missing from {names:?}"
        );
    }

    // A built-in handler runs as well, not only its registration.
    let components = harness.ok("world.list_components", None);
    assert!(components.is_array(), "{components}");

    // And Titan time control still works after the built-ins have been used.
    assert_eq!(harness.ok("titan.pause", None)["paused"], true);
}

/// Without a renderer the screenshot methods report why, instead of hanging or panicking.
#[cfg(feature = "render")]
mod screenshot {
    use bevy_app::SubApp;
    use bevy_render::RenderApp;

    use super::*;

    #[test]
    fn methods_are_registered_with_the_render_feature() {
        let mut harness = Harness::new();
        let discover = harness.ok("rpc.discover", None);
        let names: Vec<&str> = discover["methods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|method| method["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"titan.screenshot"), "{names:?}");
        assert!(names.contains(&"titan.screenshot_status"), "{names:?}");
    }

    #[test]
    fn screenshot_without_a_renderer_is_an_error() {
        let mut harness = Harness::new();
        for params in [None, Some(json!({})), Some(json!({ "path": "shot.png" }))] {
            let error = harness.call("titan.screenshot", params).unwrap_err();
            assert_eq!(error.code, i64::from(error_codes::INTERNAL_ERROR));
            assert!(error.message.contains("renderer"), "{}", error.message);
        }
    }

    #[test]
    fn screenshot_without_a_primary_window_is_an_error() {
        // An empty render sub-app makes the renderer look present, so only the window is missing.
        let mut harness = Harness::with_plugins(|app| {
            let mut render = SubApp::new();
            // Workspace feature unification can enable bevy_remote/bevy_render.
            // Its plugin needs this resource even for our non-rendering stand-in.
            render.init_resource::<bevy_render::RenderScheduleOrder>();
            app.insert_sub_app(RenderApp, render);
        });
        let error = harness.call("titan.screenshot", None).unwrap_err();
        assert_eq!(error.code, i64::from(error_codes::INTERNAL_ERROR));
        assert!(
            error.message.contains("primary window"),
            "{}",
            error.message
        );
    }

    #[test]
    fn screenshot_params_are_validated_before_anything_else() {
        let mut harness = Harness::new();
        for params in [
            json!({ "unexpected": true }),
            json!({ "path": 5 }),
            json!("shot.png"),
            json!([]),
            json!(["shot.png"]),
        ] {
            let error = harness.call("titan.screenshot", Some(params)).unwrap_err();
            assert_eq!(error.code, i64::from(error_codes::INVALID_PARAMS));
        }
        for params in [
            None,
            Some(json!({})),
            Some(json!({ "token": "1" })),
            Some(json!({ "token": 1, "unexpected": true })),
            // Well-formed, but no such job.
            Some(json!({ "token": 12345 })),
        ] {
            let error = harness.call("titan.screenshot_status", params).unwrap_err();
            assert_eq!(error.code, i64::from(error_codes::INVALID_PARAMS));
        }
    }
}
