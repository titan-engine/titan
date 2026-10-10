//! Transport-level regressions for the three-second input frame barrier.
//! These deliberately stalled HTTP responses complement the real-plugin tests.

extern crate alloc;

use alloc::sync::Arc;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Condvar, Mutex, PoisonError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use bevy_ecs::entity::Entity;
use bevy_remote::builtin_methods::{
    BRP_GET_COMPONENTS_METHOD, BRP_MUTATE_COMPONENTS_METHOD, BRP_WRITE_MESSAGE_METHOD,
    RPC_DISCOVER_METHOD,
};
use serde_json::{json, Value};
use titan_mcp::{client::Client, tools};

const WINDOW: &str = "bevy_window::window::Window";
const KEYBOARD_INPUT: &str = "bevy_input::keyboard::KeyboardInput";
const MOUSE_BUTTON_INPUT: &str = "bevy_input::mouse::MouseButtonInput";
const WINDOW_EVENT: &str = "bevy_window::event::WindowEvent";

#[derive(Clone, Default)]
struct State {
    shutdown: bool,
    status_calls: usize,
    requests: Vec<Value>,
    errors: Vec<String>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

struct Fixture {
    client: Client,
    shared: Arc<Shared>,
    server: Option<JoinHandle<()>>,
}

impl Fixture {
    // Status responses advance one frame per call. A successful barrier uses
    // three calls (baseline, +1, +2), so click's second baseline is call four.
    fn start(stall_status_call: usize) -> Self {
        Self::configure(stall_status_call, None)
    }

    fn configure(stall_status_call: usize, reject_press: Option<&'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let client = Client::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let server = thread::spawn(move || {
            // The listener owns all request workers, including a stalled status
            // worker while another worker acknowledges a cleanup release.
            thread::scope(|scope| loop {
                if worker_shared.state.lock().unwrap().shutdown {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let shared = Arc::clone(&worker_shared);
                        scope.spawn(move || {
                            if let Err(error) =
                                serve(stream, &shared, stall_status_call, reject_press)
                            {
                                shared.state.lock().unwrap().errors.push(error.to_string());
                            }
                        });
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        let state = worker_shared.state.lock().unwrap();
                        if !state.shutdown {
                            let _ = worker_shared
                                .wake
                                .wait_timeout(state, Duration::from_millis(5))
                                .unwrap();
                        }
                    }
                    Err(error) => {
                        worker_shared
                            .state
                            .lock()
                            .unwrap()
                            .errors
                            .push(error.to_string());
                        break;
                    }
                }
            });
        });
        Self {
            client,
            shared,
            server: Some(server),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Also runs during assertion unwinding. No ten-second stall sleeps or
        // detached workers: notify wakes both the listener and stalled requests.
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .shutdown = true;
        self.shared.wake.notify_all();
        if let Some(server) = self.server.take() {
            let result = server.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn serve(
    stream: TcpStream,
    shared: &Shared,
    stall_status_call: usize,
    reject_press: Option<&str>,
) -> io::Result<()> {
    // macOS accepted sockets inherit the nonblocking listener's mode.
    stream.set_nonblocking(false)?;
    // Bound request reads too, so teardown cannot hang on a partial request.
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let mut reader = BufReader::new(stream);
    loop {
        if shared.state.lock().unwrap().shutdown {
            return Ok(());
        }
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()),
            Err(error)
                if line.is_empty()
                    && matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
            {
                // Keep pooled HTTP connections alive, but check shutdown every
                // 100 ms so even idle request workers are lifetime-owned.
                continue;
            }
            result => {
                result?;
            }
        }
        if !line.starts_with("POST ") {
            return Err(io::Error::other(format!(
                "Expected HTTP POST, got {line:?}"
            )));
        }
        let mut content_length = None;
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                return Err(io::Error::other("Incomplete HTTP request headers"));
            }
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                content_length = Some(value.trim().parse::<usize>().map_err(io::Error::other)?);
            }
        }
        let length = content_length
            .filter(|length| *length <= 1024 * 1024)
            .ok_or_else(|| io::Error::other("Missing or oversized Content-Length"))?;
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        let request: Value = serde_json::from_slice(&body)?;
        let method = request["method"]
            .as_str()
            .ok_or_else(|| io::Error::other("Missing BRP method"))?;
        let status_call = {
            let mut state = shared.state.lock().unwrap();
            state.requests.push(request.clone());
            if method == "titan.status" {
                state.status_calls += 1;
            }
            state.status_calls
        };
        if method == "titan.status" && status_call == stall_status_call {
            // Send no headers or body. Client timeouts, not an HTTP/BRP error,
            // must end the barrier. Drop unblocks this worker immediately.
            let state = shared.state.lock().unwrap();
            drop(
                shared
                    .wake
                    .wait_while(state, |state| !state.shutdown)
                    .unwrap(),
            );
            return Ok(());
        }
        let result = match method {
            RPC_DISCOVER_METHOD => json!({"methods": [
                {"name": RPC_DISCOVER_METHOD},
                {"name": BRP_GET_COMPONENTS_METHOD},
                {"name": BRP_MUTATE_COMPONENTS_METHOD},
                {"name": BRP_WRITE_MESSAGE_METHOD},
                {"name": "titan.status"}
            ]}),
            "titan.status" => json!({
                "paused": true, "frame": status_call - 1, "pending_steps": 0
            }),
            BRP_GET_COMPONENTS_METHOD => json!({(WINDOW): {
                "resolution": {"scale_factor_override": null, "scale_factor": 1.0}
            }}),
            // Always acknowledge input writes, including best-effort releases, so
            // their transport timeout cannot obscure the status barrier's budget.
            BRP_MUTATE_COMPONENTS_METHOD | BRP_WRITE_MESSAGE_METHOD => Value::Null,
            _ => return Err(io::Error::other(format!("Unexpected BRP method: {method}"))),
        };
        let params = &request["params"];
        let is_press = params["value"]["state"] == "Pressed"
            || params["value"]["KeyboardInput"]["state"] == "Pressed";
        let rejected = method == BRP_WRITE_MESSAGE_METHOD
            && is_press
            && reject_press.is_some_and(|message| params["message"] == message);
        let payload = if rejected {
            json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32603,"message":"Rejected press for regression"}})
        } else {
            json!({"jsonrpc":"2.0","id":request["id"],"result":result})
        };
        let response = serde_json::to_vec(&payload)?;
        let stream = reader.get_mut();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            response.len()
        )?;
        stream.write_all(&response)?;
        stream.flush()?;
    }
}

fn assert_barrier_deadline(tool: &str, stall_status_call: usize) {
    let fixture = Fixture::start(stall_status_call);
    // A real, serializable entity avoids window-discovery traffic. Click still
    // reads the reflected Window resolution before mutating the cursor.
    let window = serde_json::to_value(Entity::from_bits(1)).unwrap();
    let args = match tool {
        "send_key" => json!({"key": "KeyW", "action": "tap", "window": window}),
        "click" => json!({"x": 8.5, "y": 12.25, "window": window}),
        _ => unreachable!(),
    };
    let start = Instant::now();
    let result = tools::call(&fixture.client, tool, args);
    let elapsed = start.elapsed();
    let error = result.expect_err("A stalled status response must fail the input barrier");
    assert!(
        error.contains("titan.status") || error.contains("separating input frames"),
        "{tool}, status call {stall_status_call}: {error}"
    );
    let lower = error.to_lowercase();
    assert!(
        lower.contains("timeout") || lower.contains("timed out"),
        "Expected barrier timeout, got: {error}"
    );
    assert!(
        elapsed > Duration::from_secs(2) && elapsed < Duration::from_secs(6),
        "{tool}, status call {stall_status_call}: expected the three-second barrier budget, not the ten-second transport timeout; elapsed {elapsed:?}, error: {error}"
    );
    // Do not hold the mutex across assertions: a test panic must not poison
    // the shutdown state needed by Drop and its lifetime-owned workers.
    let state = fixture.shared.state.lock().unwrap().clone();
    assert!(
        state.errors.is_empty(),
        "HTTP fixture errors: {:?}",
        state.errors
    );
    assert_eq!(state.status_calls, stall_status_call);
    let message_states = |message: &str| {
        state
            .requests
            .iter()
            .filter(|request| {
                request["method"] == BRP_WRITE_MESSAGE_METHOD
                    && request["params"]["message"] == message
            })
            .map(|request| request["params"]["value"]["state"].clone())
            .collect::<Vec<_>>()
    };
    if tool == "send_key" {
        assert_eq!(
            message_states(KEYBOARD_INPUT),
            vec![json!("Pressed"), json!("Released")]
        );
    } else if stall_status_call >= 4 {
        assert_eq!(
            message_states(MOUSE_BUTTON_INPUT),
            vec![json!("Pressed"), json!("Released")]
        );
    } else {
        // The cursor barrier must fail before pressing a mouse button.
        assert!(message_states(MOUSE_BUTTON_INPUT).is_empty());
    }
}

#[test]
fn key_tap_stalled_baseline_obeys_barrier_deadline() {
    assert_barrier_deadline("send_key", 1);
}

#[test]
fn key_tap_stalled_poll_obeys_barrier_deadline() {
    assert_barrier_deadline("send_key", 2);
}

#[test]
fn click_cursor_stalled_baseline_obeys_barrier_deadline() {
    assert_barrier_deadline("click", 1);
}

#[test]
fn click_cursor_stalled_poll_obeys_barrier_deadline() {
    assert_barrier_deadline("click", 2);
}

#[test]
fn click_button_stalled_baseline_obeys_barrier_deadline() {
    assert_barrier_deadline("click", 4);
}

#[test]
fn click_button_stalled_poll_obeys_barrier_deadline() {
    assert_barrier_deadline("click", 5);
}

#[test]
fn key_tap_stalled_release_baseline_obeys_barrier_deadline() {
    assert_barrier_deadline("send_key", 4);
}

#[test]
fn key_tap_stalled_release_poll_obeys_barrier_deadline() {
    assert_barrier_deadline("send_key", 5);
}

#[test]
fn click_release_stalled_baseline_obeys_barrier_deadline() {
    assert_barrier_deadline("click", 7);
}

#[test]
fn click_release_stalled_poll_obeys_barrier_deadline() {
    assert_barrier_deadline("click", 8);
}

#[test]
fn partial_key_press_attempts_both_deliveries_and_releases() {
    for rejected_message in [KEYBOARD_INPUT, WINDOW_EVENT] {
        let fixture = Fixture::configure(usize::MAX, Some(rejected_message));
        let window = serde_json::to_value(Entity::from_bits(1)).unwrap();
        let error = tools::call(
            &fixture.client,
            "send_key",
            json!({"key":"KeyW","window":window}),
        )
        .unwrap_err();
        assert!(error.contains("Rejected press for regression"), "{error}");
        let state = fixture.shared.state.lock().unwrap().clone();
        assert!(state.errors.is_empty(), "{:?}", state.errors);
        assert_eq!(state.status_calls, 0, "No barrier after a failed press");
        let writes: Vec<_> = state
            .requests
            .iter()
            .filter(|request| request["method"] == BRP_WRITE_MESSAGE_METHOD)
            .map(|request| {
                let params = &request["params"];
                let value = &params["value"];
                let phase = if params["message"] == WINDOW_EVENT {
                    &value["KeyboardInput"]["state"]
                } else {
                    &value["state"]
                };
                json!([params["message"], phase])
            })
            .collect();
        assert_eq!(
            writes,
            vec![
                json!([KEYBOARD_INPUT, "Pressed"]),
                json!([WINDOW_EVENT, "Pressed"]),
                json!([KEYBOARD_INPUT, "Released"]),
                json!([WINDOW_EVENT, "Released"]),
            ]
        );
    }
}
