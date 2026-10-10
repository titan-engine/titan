//! Owned log capture against real headless BRP games, without the #73 fixtures.

#[expect(dead_code, reason = "Only the example run function is used")]
#[path = "../examples/server.rs"]
mod server;

use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use titan_mcp::{
    client::Client,
    process::{CommandSpec, ProcessConfig, ProcessManager},
    tools,
};

fn emit_logs() {
    writeln!(std::io::stdout().lock(), "INFO game: startup stdout").unwrap();
    writeln!(
        std::io::stderr().lock(),
        "2026-04-01T12:00:00.123Z \x1b[33m WARN\x1b[0m game: missing asset"
    )
    .unwrap();
    thread::spawn(|| loop {
        if fs::exists("emit").unwrap() {
            fs::remove_file("emit").unwrap();
            writeln!(std::io::stdout().lock(), "INFO game: new poll output").unwrap();
        }
        thread::sleep(Duration::from_millis(5));
    });
}

#[test]
#[ignore = "child game fixture"]
fn example_fixture() {
    emit_logs();
    let port = fs::read_to_string("port").unwrap().parse().unwrap();
    server::run(port);
}

#[test]
#[ignore = "child game fixture"]
fn panic_fixture() {
    use bevy_app::{App, TaskPoolOptions, TaskPoolPlugin, Update};
    use bevy_remote::{http::RemoteHttpPlugin, RemotePlugin};
    emit_logs();
    let port = fs::read_to_string("port").unwrap().parse().unwrap();
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin {
        task_pool_options: TaskPoolOptions::with_num_threads(1),
    })
    .add_plugins((
        RemotePlugin::default(),
        RemoteHttpPlugin::default().with_port(port),
    ))
    .add_systems(Update, || {
        assert!(
            !fs::exists("panic").unwrap(),
            "game crashed after readiness"
        );
    });
    app.finish();
    app.cleanup();
    loop {
        app.update();
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "child game fixture"]
fn early_panic_fixture() {
    writeln!(std::io::stdout().lock(), "startup before panic").unwrap();
    panic!("missing required startup asset");
}

#[test]
#[ignore = "child game fixture"]
fn chatty_fixture() {
    // Both streams exceed OS pipe capacity, including a single enormous line.
    let stdout = thread::spawn(|| {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{}", "x".repeat(200_000)).unwrap();
        for i in 0..5000 {
            writeln!(out, "stdout line {i} {}", "x".repeat(256)).unwrap();
        }
        out.flush().unwrap();
    });
    let mut err = std::io::stderr().lock();
    for i in 0..5000 {
        writeln!(err, "stderr line {i} {}", "y".repeat(256)).unwrap();
    }
    stdout.join().unwrap();
    write!(std::io::stdout().lock(), "final stdout without newline").unwrap();
    writeln!(err, "final stderr marker").unwrap();
}

#[test]
#[ignore = "child game fixture"]
fn timeout_fixture() {
    writeln!(
        std::io::stderr().lock(),
        "WARN startup: BRP plugin is missing"
    )
    .unwrap();
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(unix)]
#[test]
#[ignore = "detached child holding inherited game pipes"]
fn detached_holder_fixture() {
    rustix::process::setsid().unwrap();
    fs::write("holder_ready", "yes").unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut emitted = false;
    while !fs::exists("release_holder").unwrap() && Instant::now() < deadline {
        if !emitted && fs::exists("late_output").unwrap() {
            let _ = writeln!(std::io::stdout().lock(), "detached old stdout");
            let _ = writeln!(std::io::stderr().lock(), "detached old stderr");
            fs::write("late_done", "yes").unwrap();
            emitted = true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    std::process::exit(0);
}

#[cfg(unix)]
#[test]
#[ignore = "game with an intentionally escaped descendant"]
#[expect(
    clippy::zombie_processes,
    reason = "Deliberately orphan a setsid descendant to test bounded pipe cleanup"
)]
fn detached_game_fixture() {
    if !fs::exists("holder_ready").unwrap() {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "detached_holder_fixture",
                "--nocapture",
            ])
            .spawn()
            .unwrap();
        fs::write("holder_pid", child.id().to_string()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fs::exists("holder_ready").unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
    }
    emit_logs();
    thread::spawn(|| loop {
        if fs::exists("exit_game").unwrap() {
            fs::write("exiting", "yes").unwrap();
            std::process::exit(0);
        }
        thread::sleep(Duration::from_millis(5));
    });
    let port = fs::read_to_string("port").unwrap().parse().unwrap();
    server::run(port);
}

struct Fixture {
    directory: tempfile::TempDir,
    client: Client,
    config: ProcessConfig,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        fs::write(directory.path().join("port"), port.to_string()).unwrap();
        let client = Client::new(&format!("http://127.0.0.1:{port}")).unwrap();
        let mut config = ProcessConfig::new(CommandSpec {
            program: std::env::current_exe().unwrap().to_str().unwrap().into(),
            args: vec![
                "--ignored".into(),
                "--exact".into(),
                name.into(),
                "--nocapture".into(),
            ],
        });
        config.directory = Some(directory.path().into());
        config.stop_timeout = Duration::from_millis(50);
        drop(listener);
        Self {
            directory,
            client,
            config,
        }
    }

    fn call(&self, manager: &mut ProcessManager, name: &str, args: Value) -> Value {
        tools::call_managed(&self.client, manager, name, args).unwrap()
    }

    fn logs(&self, manager: &mut ProcessManager, args: Value) -> Value {
        self.call(manager, "game_logs", args)
    }

    fn wait_for_logs(&self, manager: &mut ProcessManager, args: Value, messages: &[&str]) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let page = self.logs(manager, args.clone());
            let text = page["lines"].to_string();
            if messages.iter().all(|message| text.contains(message)) {
                return page;
            }
            assert!(
                Instant::now() < deadline,
                "expected output never arrived: {page}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(unix)]
#[test]
fn detached_descendant_cannot_hang_status_stop_or_contaminate_next_launch() {
    // Test both natural exit (status cleanup) and intentional stop. Each fixture
    // leaves a new-session helper holding BOTH stdout and stderr until released.
    for natural_exit in [true, false] {
        let fixture = Fixture::new("detached_game_fixture");
        struct ReleaseHolder(std::path::PathBuf);
        impl Drop for ReleaseHolder {
            fn drop(&mut self) {
                let _ = fs::write(&self.0, "yes");
            }
        }
        let _release = ReleaseHolder(fixture.directory.path().join("release_holder"));
        let mut manager = ProcessManager::new(fixture.config.clone()).unwrap();
        manager.launch(&fixture.client).unwrap();
        if natural_exit {
            fs::write(fixture.directory.path().join("exit_game"), "yes").unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !fixture.directory.path().join("exiting").exists() {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(5));
            }
        }
        let client = Client::new(fixture.client.url()).unwrap();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let started = Instant::now();
            if natural_exit {
                while manager.status().unwrap().owned {
                    thread::sleep(Duration::from_millis(5));
                }
            }
            let stopped =
                tools::call_managed(&client, &mut manager, "stop_game", json!({})).unwrap();
            let status =
                tools::call_managed(&client, &mut manager, "game_status", json!({})).unwrap();
            let elapsed = started.elapsed();
            let _ = sender.send((manager, stopped, status, elapsed));
        });
        let (mut manager, stopped, status, elapsed) = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("status/stop blocked on a detached pipe holder");
        worker.join().unwrap();
        // One shared one-second reader budget, not a separate wait for each pipe.
        assert!(elapsed < Duration::from_secs(3), "cleanup took {elapsed:?}");
        assert_eq!(stopped["state"], "exited");
        assert_eq!(status["process"]["state"], "exited");
        let logs = fixture.logs(&mut manager, json!({}));
        assert!(logs["reader_error"]
            .as_str()
            .unwrap()
            .contains("detached descendant"));
        assert!(logs.to_string().contains("startup stdout"));
        fs::remove_file(fixture.directory.path().join("exit_game")).ok();
        let launched = fixture.call(&mut manager, "launch_game", json!({}));
        let cursor = launched["log_cursor"].as_u64().unwrap();
        fixture.wait_for_logs(
            &mut manager,
            json!({"since":cursor}),
            &["startup stdout", "missing asset"],
        );
        // Wake the old readers only AFTER the new generation starts.
        fs::write(fixture.directory.path().join("late_output"), "yes").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fixture.directory.path().join("late_done").exists() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        fs::write(fixture.directory.path().join("release_holder"), "yes").unwrap();
        manager.stop().unwrap();
        let logs = fixture.logs(&mut manager, json!({"since":cursor}));
        assert_eq!(logs["reader_error"], Value::Null);
        assert!(!logs["lines"].to_string().contains("detached old"));
    }
}

#[test]
fn launched_example_poll_filters_and_restart_cursors() {
    let fixture = Fixture::new("example_fixture");
    let mut manager = ProcessManager::new(fixture.config.clone()).unwrap();
    let launched = fixture.call(&mut manager, "launch_game", json!({}));
    let cursor = launched["log_cursor"].as_u64().unwrap();
    assert_eq!(cursor, 0);
    let first = fixture.wait_for_logs(
        &mut manager,
        json!({"since":cursor}),
        &["startup stdout", "missing asset"],
    );
    assert!(first["lines"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line["text"] == "INFO game: startup stdout" && line["stream"] == "stdout"));
    let warnings = fixture.logs(
        &mut manager,
        json!({"since":cursor,"level":"warn","contains":"asset"}),
    );
    assert_eq!(warnings["lines"].as_array().unwrap().len(), 1);
    assert_eq!(warnings["lines"][0]["stream"], "stderr");
    assert!(!warnings["lines"][0]["text"]
        .as_str()
        .unwrap()
        .contains('\u{1b}'));
    let cursor = first["cursor"].as_u64().unwrap();
    assert_eq!(
        fixture.logs(&mut manager, json!({"since":cursor}))["lines"],
        json!([])
    );
    fs::write(fixture.directory.path().join("emit"), "yes").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let page = fixture.logs(&mut manager, json!({"since":cursor}));
        if page["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line["text"] == "INFO game: new poll output")
        {
            break;
        }
        assert!(Instant::now() < deadline, "new output never arrived");
        thread::sleep(Duration::from_millis(5));
    }
    let restarted = fixture.call(&mut manager, "restart_game", json!({}));
    let run_cursor = restarted["log_cursor"].as_u64().unwrap();
    assert!(run_cursor > cursor);
    let page = fixture.logs(&mut manager, json!({"since":run_cursor}));
    assert!(page["lines"]
        .as_array()
        .unwrap()
        .iter()
        .all(|line| line["cursor"].as_u64().unwrap() > run_cursor));
    assert!(!page["lines"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line["text"] == "INFO game: new poll output"));
    manager.stop().unwrap();
    assert!(!fixture.logs(&mut manager, json!({}))["lines"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn panic_and_exit_survive_after_ready_game_crashes() {
    let fixture = Fixture::new("panic_fixture");
    let mut manager = ProcessManager::new(fixture.config.clone()).unwrap();
    manager.launch(&fixture.client).unwrap();
    fs::write(fixture.directory.path().join("panic"), "yes").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let page = fixture.logs(
            &mut manager,
            json!({"contains":"game crashed after readiness"}),
        );
        if page["process"]["state"] == "exited" {
            assert!(!page["lines"].as_array().unwrap().is_empty());
            assert_eq!(page["process"]["exit"]["success"], false);
            assert_eq!(page["process"]["exit"]["code"], 101);
            let again = fixture.logs(
                &mut manager,
                json!({"contains":"game crashed after readiness"}),
            );
            assert_eq!(again, page);
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn readiness_failure_includes_panic_or_timeout_logs() {
    for (name, message) in [
        ("early_panic_fixture", "missing required startup asset"),
        ("timeout_fixture", "BRP plugin is missing"),
    ] {
        let mut fixture = Fixture::new(name);
        fixture.config.ready_timeout = Duration::from_secs(2);
        let mut manager = ProcessManager::new(fixture.config.clone()).unwrap();
        let error = manager.launch(&fixture.client).unwrap_err();
        assert!(error.contains(message), "{error}");
        let page = fixture.logs(&mut manager, json!({"contains":message}));
        assert!(!page["lines"].as_array().unwrap().is_empty());
        assert_eq!(page["process"]["state"], "exited");
    }
}

#[test]
fn chatty_dual_pipes_drain_and_retention_loss_is_reported() {
    let fixture = Fixture::new("chatty_fixture");
    let mut manager = ProcessManager::new(fixture.config.clone()).unwrap();
    assert!(manager
        .launch(&fixture.client)
        .unwrap_err()
        .contains("Owned game exited"));
    let page = fixture.logs(&mut manager, json!({"since":0,"max_lines":100}));
    assert!(page["dropped_lines"].as_u64().unwrap() > 0);
    assert!(page["omitted_lines"].as_u64().unwrap() > 0);
    assert_eq!(page["has_more"], true);
    assert!(serde_json::to_vec(&page).unwrap().len() < 24 * 1024);
    let tail = fixture.logs(&mut manager, json!({"contains":"final"}));
    let text = tail.to_string();
    assert!(text.contains("final stdout without newline"));
    assert!(text.contains("final stderr marker"));
    assert_eq!(page["process"]["exit"]["success"], true);
}

#[test]
fn attach_only_and_invalid_log_arguments_are_actionable() {
    let client = Client::new("http://127.0.0.1:1").unwrap();
    assert!(tools::call(&client, "game_logs", json!({}))
        .unwrap_err()
        .contains("only available for launched games"));
    for args in [
        json!({"since":-1}),
        json!({"since":1.5}),
        json!({"since":18446744073709551616.0}),
        json!({"max_lines":0}),
        json!({"max_lines":101}),
        json!({"level":"fatal"}),
        json!({"contains":1}),
        json!({"extra":true}),
    ] {
        assert!(tools::call(&client, "game_logs", args).is_err());
    }
}

#[test]
fn game_output_never_corrupts_mcp_stdout() {
    let fixture = Fixture::new("example_fixture");
    let child = Command::new(env!("CARGO_BIN_EXE_titan_mcp"))
        .args([
            "--url",
            fixture.client.url(),
            "--game-dir",
            fixture.directory.path().to_str().unwrap(),
            "--game-cmd",
            &serde_json::to_string(
                &std::iter::once(fixture.config.game.program.clone())
                    .chain(fixture.config.game.args.clone())
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
            "--stop-timeout-secs",
            "1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // EOF gets a chance to clean up the owned game even if an assertion unwinds.
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(3);
            while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = ChildGuard(child);
    let mut input = child.0.stdin.take().unwrap();
    let mut output = BufReader::new(child.0.stdout.take().unwrap());
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || loop {
        let mut line = String::new();
        match output.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if sender.send(line).is_err() {
                    break;
                }
            }
        }
    });
    let request = |input: &mut std::process::ChildStdin, id: u64, method: &str, params: Value| {
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        input.flush().unwrap();
        let line = receiver
            .recv_timeout(Duration::from_secs(35))
            .expect("MCP response timed out");
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        if id > 1 {
            assert_eq!(response["result"]["isError"], false, "{response}");
        }
        response
    };
    for (id, method, params) in [
        (
            1,
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        ),
        (
            2,
            "tools/call",
            json!({"name":"launch_game","arguments":{}}),
        ),
    ] {
        request(&mut input, id, method, params);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut id = 3;
    loop {
        let response = request(
            &mut input,
            id,
            "tools/call",
            json!({"name":"game_logs","arguments":{"since":0}}),
        );
        let page: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        if page.to_string().contains("startup stdout") {
            break;
        }
        assert!(Instant::now() < deadline, "startup output never arrived");
        id += 1;
        thread::sleep(Duration::from_millis(5));
    }
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "MCP shutdown timed out");
        thread::sleep(Duration::from_millis(10));
    }
    reader.join().unwrap();
}
