//! Real process lifecycle tests, without cargo-in-cargo or timing-sensitive frames.

#[expect(
    dead_code,
    reason = "The example main is unused when its run function is tested"
)]
#[path = "../examples/server.rs"]
mod server;

use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use titan_mcp::{
    client::Client,
    process::{CommandSpec, ProcessConfig, ProcessManager},
    tools,
};

#[test]
#[ignore = "spawned by lifecycle tests"]
fn game_fixture() {
    let port = fs::read_to_string("port").unwrap().parse().unwrap();
    thread::spawn(|| loop {
        if fs::exists("crash").unwrap() {
            std::process::exit(42);
        }
        thread::sleep(Duration::from_millis(5));
    });
    server::run(port);
}

#[test]
#[ignore = "spawned by lifecycle tests"]
fn crash_fixture() {
    std::process::exit(42);
}

#[test]
#[ignore = "spawned by lifecycle tests"]
fn build_fixture() {
    fs::write("built", "yes").unwrap();
    if fs::exists("fail").unwrap() {
        writeln!(
            std::io::stderr().lock(),
            "error[E0308]: mismatched types: expected u32, found &str"
        )
        .unwrap();
        // Fill both pipes past their capture budgets; the parent must still drain.
        writeln!(std::io::stdout().lock(), "{}", "x".repeat(100_000)).unwrap();
        writeln!(std::io::stderr().lock(), "{}", "y".repeat(100_000)).unwrap();
        writeln!(
            std::io::stderr().lock(),
            "error[E0425]: cannot find value late_error in this scope"
        )
        .unwrap();
        std::process::exit(1);
    }
}

#[test]
#[ignore = "spawned by lifecycle tests"]
fn tree_fixture() {
    let spec = command("game_fixture");
    let mut child = Command::new(spec.program)
        .args(spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Keep the real child handle until shutdown; group/job owns both processes.
    let _ = child.wait();
}

#[test]
#[ignore = "spawned by lifecycle tests"]
#[expect(
    clippy::zombie_processes,
    reason = "Deliberately orphan a descendant to test tree cleanup"
)]
fn build_tree_fixture() {
    let spec = command("hang_fixture");
    let child = Command::new(spec.program).args(spec.args).spawn().unwrap();
    fs::write("descendant", child.id().to_string()).unwrap();
    // Deliberately exit while a descendant still owns BOTH build output pipes.
}

#[test]
#[ignore = "spawned by lifecycle tests"]
fn graceful_descendant_fixture() {
    let cancelled = titan_mcp::process::ProcessCancellation::default();
    let flag = cancelled.clone();
    ctrlc::set_handler(move || flag.request()).unwrap();
    fs::write("descendant_ready", "yes").unwrap();
    while !cancelled.is_requested() {
        thread::sleep(Duration::from_millis(5));
    }
    thread::sleep(Duration::from_millis(150));
    fs::write("graceful_done", "yes").unwrap();
}

#[test]
#[ignore = "spawned by lifecycle tests"]
#[expect(
    clippy::zombie_processes,
    reason = "Leader exits on SIGTERM before its descendant"
)]
fn graceful_tree_fixture() {
    let spec = command("graceful_descendant_fixture");
    let child = Command::new(spec.program).args(spec.args).spawn().unwrap();
    fs::write("descendant", child.id().to_string()).unwrap();
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "spawned by lifecycle tests"]
fn hang_fixture() {
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn command(fixture: &str) -> CommandSpec {
    CommandSpec {
        program: std::env::current_exe()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned(),
        args: vec![
            "--ignored".into(),
            "--exact".into(),
            fixture.into(),
            "--nocapture".into(),
        ],
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    client: Client,
    config: ProcessConfig,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        fs::write(directory.path().join("port"), port.to_string()).unwrap();
        let client = Client::new(&format!("http://127.0.0.1:{port}")).unwrap();
        let mut config = ProcessConfig::new(command("game_fixture"));
        config.build = Some(command("build_fixture"));
        config.directory = Some(directory.path().to_owned());
        // Startup excludes compilation, and no exact frame-count assumptions.
        config.ready_timeout = Duration::from_secs(30);
        config.stop_timeout = Duration::from_millis(100);
        drop(listener);
        Self {
            directory,
            client,
            config,
        }
    }

    fn manager(&self) -> ProcessManager {
        ProcessManager::new(self.config.clone()).unwrap()
    }

    fn call(&self, manager: &mut ProcessManager, name: &str, args: Value) -> Value {
        tools::call_managed(&self.client, manager, name, args).unwrap()
    }
}

#[test]
fn launch_query_rebuild_restart_query_and_stop_example() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let status = fixture.call(&mut manager, "game_status", json!({}));
    assert_eq!(status["process"]["state"], "stopped");
    assert_eq!(status["reachable"], false);
    let launched = fixture.call(&mut manager, "launch_game", json!({}));
    assert_eq!(launched["owned"], true);
    assert!(launched["pid"].as_u64().is_some());
    assert!(
        tools::call_managed(&fixture.client, &mut manager, "launch_game", json!({}))
            .unwrap_err()
            .contains("already running")
    );
    let query = json!({"resource":"Counter"});
    assert!(
        fixture.call(&mut manager, "get_resource", query.clone())["value"]["ticks"]
            .as_u64()
            .unwrap()
            > 0
    );
    fixture.call(&mut manager, "restart_game", json!({"rebuild":true}));
    assert!(fixture.directory.path().join("built").exists());
    assert!(
        fixture.call(&mut manager, "get_resource", query)["value"]["ticks"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        fixture.call(&mut manager, "game_status", json!({}))["reachable"],
        true
    );
    fixture.call(&mut manager, "rebuild_game", json!({}));
    assert!(!manager.status().unwrap().owned);
    fixture.call(&mut manager, "launch_game", json!({}));
    assert_eq!(
        fixture.call(&mut manager, "stop_game", json!({}))["owned"],
        false
    );
    fixture.call(&mut manager, "stop_game", json!({}));
    assert!(fixture.client.call("rpc.discover", None).is_err());
}

#[test]
fn build_errors_are_readable_capped_and_leave_game_stopped() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    manager.launch(&fixture.client).unwrap();
    fs::write(fixture.directory.path().join("fail"), "yes").unwrap();
    let error = tools::call_managed(
        &fixture.client,
        &mut manager,
        "restart_game",
        json!({"rebuild":true}),
    )
    .unwrap_err();
    assert!(error.contains("error[E0308]: mismatched types"), "{error}");
    assert!(
        error.contains("error[E0425]: cannot find value late_error"),
        "{error}"
    );
    assert!(error.contains("truncated"));
    assert!(error.contains("game is stopped"));
    assert!(error.len() < 20 * 1024);
    assert!(!manager.status().unwrap().owned);
    assert!(fixture.client.call("rpc.discover", None).is_err());
    fs::remove_file(fixture.directory.path().join("fail")).unwrap();
    manager.restart(&fixture.client, true).unwrap();
}

#[test]
fn crash_exit_is_reported_in_status_and_next_world_tool() {
    let mut fixture = Fixture::new();
    fixture.config.game = command("crash_fixture");
    let mut manager = fixture.manager();
    let error = manager.launch(&fixture.client).unwrap_err();
    assert!(error.contains("42"), "{error}");
    let status = fixture.call(&mut manager, "game_status", json!({}));
    assert_eq!(status["process"]["exit"]["code"], 42);
    let error = tools::call_managed(&fixture.client, &mut manager, "query_entities", json!({}))
        .unwrap_err();
    assert!(error.contains("exited") && error.contains("42"), "{error}");
}

#[test]
fn running_game_crash_is_reported_on_the_next_call() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    manager.launch(&fixture.client).unwrap();
    fs::write(fixture.directory.path().join("crash"), "yes").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while manager.status().unwrap().owned {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let error = tools::call_managed(&fixture.client, &mut manager, "query_entities", json!({}))
        .unwrap_err();
    assert!(error.contains("42"), "{error}");
}

#[test]
fn real_compiler_failure_returns_diagnostics() {
    let mut fixture = Fixture::new();
    fs::write(
        fixture.directory.path().join("broken.rs"),
        "fn main() { let _: u32 = \"wrong\"; }",
    )
    .unwrap();
    fixture.config.build = Some(CommandSpec {
        program: "rustc".into(),
        args: vec!["--color=never".into(), "broken.rs".into()],
    });
    let mut manager = fixture.manager();
    manager.launch(&fixture.client).unwrap();
    let error = manager.restart(&fixture.client, true).unwrap_err();
    assert!(
        error.contains("error[E0308]") && error.contains("mismatched types"),
        "{error}"
    );
    assert!(!manager.status().unwrap().owned);
}

#[test]
fn readiness_and_build_timeouts_cleanup() {
    let mut fixture = Fixture::new();
    fixture.config.game = command("hang_fixture");
    fixture.config.ready_timeout = Duration::from_millis(300);
    fixture.config.build_timeout = Duration::from_millis(300);
    fixture.config.build = Some(command("hang_fixture"));
    let mut manager = fixture.manager();
    let start = Instant::now();
    assert!(manager
        .launch(&fixture.client)
        .unwrap_err()
        .contains("timed out"));
    assert!(!manager.status().unwrap().owned);
    assert!(manager.rebuild().unwrap_err().contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn attached_process_is_not_claimed_or_stopped_and_commands_cannot_be_overridden() {
    let fixture = Fixture::new();
    let mut owner = fixture.manager();
    owner.launch(&fixture.client).unwrap();
    let mut attached = ProcessManager::attached();
    assert_eq!(
        fixture.call(&mut attached, "game_status", json!({}))["process"]["owned"],
        false
    );
    assert!(attached
        .stop()
        .unwrap_err()
        .contains("attached game is never stopped"));
    assert!(fixture
        .manager()
        .launch(&fixture.client)
        .unwrap_err()
        .contains("already reachable"));
    for name in ["launch_game", "stop_game", "restart_game", "rebuild_game"] {
        let error = tools::call_managed(
            &fixture.client,
            &mut owner,
            name,
            json!({"command":"malicious"}),
        )
        .unwrap_err();
        assert!(error.contains("Unknown argument"));
    }
    drop(attached);
    assert!(fixture.client.call("rpc.discover", None).is_ok());
    drop(owner);
    assert!(fixture.client.call("rpc.discover", None).is_err());
}

#[test]
fn owned_descendants_are_stopped_and_build_pipes_are_released() {
    let mut fixture = Fixture::new();
    fixture.config.game = command("tree_fixture");
    fixture.config.build = Some(command("build_tree_fixture"));
    let mut manager = fixture.manager();
    manager.launch(&fixture.client).unwrap();
    manager.stop().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.client.call("rpc.discover", None).is_ok() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let started = Instant::now();
    manager.rebuild().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(fixture.directory.path().join("descendant").exists());
}

#[cfg(unix)]
#[test]
fn exited_leader_does_not_cut_short_descendant_grace_period() {
    let mut fixture = Fixture::new();
    fixture.config.build = Some(command("graceful_tree_fixture"));
    fixture.config.build_timeout = Duration::from_secs(1);
    // Launch a tree without BRP directly as a private group, then exercise its
    // termination as the configured game after readiness times out. Its leader
    // dies immediately on SIGTERM, while its child flushes a marker later.
    fixture.config.game = command("graceful_tree_fixture");
    fixture.config.ready_timeout = Duration::from_secs(5);
    fixture.config.stop_timeout = Duration::from_millis(500);
    let mut manager = fixture.manager();
    assert!(manager
        .launch(&fixture.client)
        .unwrap_err()
        .contains("timed out"));
    assert!(fixture.directory.path().join("descendant_ready").exists());
    assert!(fixture.directory.path().join("graceful_done").exists());
}

#[cfg(unix)]
#[test]
fn sigterm_resistant_tree_is_forcefully_stopped() {
    let mut fixture = Fixture::new();
    let spec = command("tree_fixture");
    let mut args = vec![
        "-c".into(),
        "trap '' TERM; exec \"$@\"".into(),
        "fixture".into(),
        spec.program,
    ];
    args.extend(spec.args);
    fixture.config.game = CommandSpec {
        program: "/bin/sh".into(),
        args,
    };
    let mut manager = fixture.manager();
    manager.launch(&fixture.client).unwrap();
    let status = manager.stop().unwrap();
    assert!(!status.owned);
    assert!(!status.exit.unwrap().success);
}

#[test]
fn stopping_one_owned_tree_does_not_stop_an_unrelated_game() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    manager.launch(&fixture.client).unwrap();
    let unrelated = Fixture::new();
    let mut other = unrelated.manager();
    other.launch(&unrelated.client).unwrap();
    manager.stop().unwrap();
    assert!(other.status().unwrap().owned);
    assert!(unrelated.client.call("rpc.discover", None).is_ok());
}

#[test]
fn cancellation_interrupts_a_build_and_forbids_new_launches() {
    let mut fixture = Fixture::new();
    fixture.config.build = Some(command("hang_fixture"));
    let mut manager = fixture.manager();
    let cancellation = manager.cancellation();
    let worker = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancellation.request();
    });
    let started = Instant::now();
    assert!(manager.rebuild().unwrap_err().contains("cancelled"));
    assert!(started.elapsed() < Duration::from_secs(5));
    worker.join().unwrap();
    assert!(manager
        .launch(&fixture.client)
        .unwrap_err()
        .contains("cancelled"));
}

#[test]
fn missing_build_is_rejected_without_stopping_game() {
    let mut fixture = Fixture::new();
    fixture.config.build = None;
    let mut manager = fixture.manager();
    manager.launch(&fixture.client).unwrap();
    assert!(manager
        .restart(&fixture.client, true)
        .unwrap_err()
        .contains("not stopped"));
    assert!(manager.status().unwrap().owned);
    assert!(fixture.client.call("rpc.discover", None).is_ok());
}

fn binary(fixture: &Fixture) -> std::process::Child {
    let argv = |spec: &CommandSpec| {
        serde_json::to_string(
            &std::iter::once(&spec.program)
                .chain(spec.args.iter())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    Command::new(env!("CARGO_BIN_EXE_titan_mcp"))
        .args([
            "--url",
            fixture.client.url(),
            "--game-cmd",
            &argv(&fixture.config.game),
            "--build-cmd",
            &argv(fixture.config.build.as_ref().unwrap()),
            "--game-dir",
            fixture.directory.path().to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
fn stdio_eof_stops_owned_game_and_keeps_stdout_protocol_only() {
    let fixture = Fixture::new();
    let mut child = binary(&fixture);
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"launch_game"}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"restart_game","arguments":{"rebuild":true}}}),
    ] {
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], request["id"]);
        assert_ne!(response["result"]["isError"], true, "{response}");
    }
    assert!(fixture.client.call("rpc.discover", None).is_ok());
    drop(input);
    assert!(child.wait().unwrap().success());
    assert!(fixture.client.call("rpc.discover", None).is_err());
}

#[cfg(unix)]
fn assert_sigterm_shutdown(backpressure: bool) {
    use rustix::process::{kill_process, Pid, Signal};
    let fixture = Fixture::new();
    let mut child = binary(&fixture);
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"launch_game"}}),
    ] {
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_ne!(response["result"]["isError"], true, "{response}");
    }
    if backpressure {
        let ping = json!({"jsonrpc":"2.0","id":"x".repeat(512 * 1024),"method":"ping"});
        writeln!(input, "{ping}").unwrap();
        input.flush().unwrap();
        // Observe queued output without draining it, instead of assuming the
        // sidecar reached its blocking write after an arbitrary sleep.
        let deadline = Instant::now() + Duration::from_secs(10);
        while rustix::io::ioctl_fionread(output.get_ref()).unwrap() < 4096 {
            assert!(Instant::now() < deadline, "MCP never filled stdout");
            thread::sleep(Duration::from_millis(10));
        }
    }
    kill_process(
        Pid::from_raw(child.id().try_into().unwrap()).unwrap(),
        Signal::TERM,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("MCP failed to clean up after SIGTERM");
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(fixture.client.call("rpc.discover", None).is_err());
    drop(input);
}

#[cfg(unix)]
#[test]
fn sigterm_stops_owned_game_even_when_client_keeps_stdin_open() {
    assert_sigterm_shutdown(false);
}

#[cfg(unix)]
#[test]
fn sigterm_stops_owned_game_even_when_stdout_is_backpressured() {
    assert_sigterm_shutdown(true);
}
