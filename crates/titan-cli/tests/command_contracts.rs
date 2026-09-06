use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use titan_protocol::{Request, Response, ResponseEnvelope, RunMode, RuntimeStatus, SCHEMA_VERSION};

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "titan-cli-contract-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_titan"))
            .arg("--project")
            .arg(&self.0)
            .args(arguments)
            .env("CARGO_BUILD_JOBS", "4")
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_TARGET_DIR", self.0.join("target"))
            .output()
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn successful_cargo_workflows_preserve_arguments_and_output_streams() {
    let project = Project::new();
    std::fs::create_dir_all(project.0.join("src")).unwrap();
    std::fs::create_dir_all(project.0.join("examples")).unwrap();
    std::fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"cli-contract-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(project.0.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
    std::fs::write(
        project.0.join("examples/hello.rs"),
        "fn main() { println!(\"fixture stdout\"); eprintln!(\"fixture stderr\"); }\n",
    )
    .unwrap();

    for (arguments, command, cargo_arguments) in [
        (
            vec!["check"],
            "check",
            vec!["check", "--workspace", "--all-targets", "--all-features"],
        ),
        (
            vec!["test"],
            "test",
            vec!["test", "--workspace", "--all-targets"],
        ),
        (
            vec!["run-example", "hello"],
            "run_example",
            vec!["run", "--example", "hello"],
        ),
    ] {
        let mut arguments = arguments;
        arguments.push("--format=json");
        let output = project.run(&arguments);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["command"], command);
        assert_eq!(result["success"], true);
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["data"]["type"], "process");
        assert_eq!(
            result["data"]["arguments"],
            serde_json::json!(cargo_arguments)
        );
        assert!(result.get("diagnostic_bundle").is_none());
        assert!(result.get("error_code").is_none());
        if command == "run_example" {
            assert_eq!(result["stdout"], "fixture stdout\n");
            assert!(
                result["stderr"]
                    .as_str()
                    .unwrap()
                    .contains("fixture stderr")
            );
        }
    }
    let human = project.run(&["run-example", "hello"]);
    assert!(human.status.success(), "{human:?}");
    assert_eq!(human.stdout, b"fixture stdout\n");
    let stderr = String::from_utf8(human.stderr).unwrap();
    assert!(stderr.contains("fixture stderr"));
    assert!(stderr.contains("Titan command `run_example` completed successfully."));
    assert!(!project.0.join("target/titan/diagnostics").exists());
}

#[test]
fn remote_status_preserves_envelope_and_instance_listing_redacts_credentials() {
    let project = Project::new();
    let (server, queue) = titan_remote::Server::start(titan_remote::ServerConfig::new(
        &project.0,
        "cli-contract",
        RunMode::Headless,
    ))
    .unwrap();
    let listing = project.run(&["instances", "--format=json"]);
    assert!(listing.status.success(), "{listing:?}");
    assert!(listing.stderr.is_empty());
    let result: serde_json::Value = serde_json::from_slice(&listing.stdout).unwrap();
    assert_eq!(result["instances"].as_array().unwrap().len(), 1);
    assert_eq!(result["instances"][0]["instance_id"], "cli-contract");
    assert!(result["instances"][0].get("token").is_none());
    assert!(
        !String::from_utf8(listing.stdout)
            .unwrap()
            .contains(&server.registration().token)
    );

    for format in ["json", "human"] {
        std::thread::scope(|scope| {
            let child = scope.spawn(|| {
                project.run(&["status", "--instance", "cli-contract", "--format", format])
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut requests = 0;
            while !child.is_finished() && Instant::now() < deadline {
                requests += queue.drain(|request| {
                    assert_eq!(request.request, Request::Status);
                    assert_eq!(request.target_instance.as_deref(), Some("cli-contract"));
                    ResponseEnvelope::success(
                        request,
                        "cli-contract",
                        42,
                        7,
                        Response::Status(RuntimeStatus {
                            project: "contract-project".into(),
                            run_mode: RunMode::Headless,
                            current_frame: 42,
                            paused: true,
                        }),
                    )
                });
                std::thread::sleep(Duration::from_millis(2));
            }
            let output = child.join().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty());
            assert_eq!(requests, 1);
            let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["status"], "success");
            assert_eq!(result["schema_version"], SCHEMA_VERSION);
            assert_eq!(result["instance_id"], "cli-contract");
            assert_eq!(result["observed_frame"], 42);
            assert_eq!(result["state_revision"], 7);
            assert_eq!(result["response"]["type"], "status");
            assert_eq!(result["response"]["current_frame"], 42);
            let text = String::from_utf8(output.stdout).unwrap();
            assert_eq!(text.lines().count() > 1, format == "human");
        });
    }
    assert!(!project.0.join("target/titan/diagnostics").exists());
}

#[test]
fn help_and_version_stay_text_even_when_json_is_requested() {
    let project = Project::new();
    for arguments in [
        vec!["--help"],
        vec!["--format=json", "--help"],
        vec!["compare-images", "--help", "--format", "json"],
    ] {
        let output = project.run(&arguments);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("Usage: titan")
        );
    }
    let version = project.run(&["--format=json", "--version"]);
    assert!(version.status.success());
    assert!(version.stderr.is_empty());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        format!("titan {}\n", env!("CARGO_PKG_VERSION"))
    );
    let info = project.run(&["info"]);
    assert!(info.status.success());
    assert!(info.stderr.is_empty());
    assert_eq!(
        String::from_utf8(info.stdout).unwrap(),
        format!(
            "Titan CLI {}\nInspection protocol schema {SCHEMA_VERSION}\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    let invalid = project.run(&["step", "not-a-number", "--diagnostics", "never"]);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    assert!(
        String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("invalid value")
    );
}
