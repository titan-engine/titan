use std::process::Command;

fn run(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_titan"))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn parse_and_payload_failures_are_single_json_results() {
    for arguments in [
        vec!["--format", "json", "step", "not-a-number"],
        vec!["step", "1", "--format=json", "--timeout-ms", "0"],
        vec!["--format", "json", "invoke", "reset", "--arguments", "{"],
        vec!["--format", "json", "input", "1", "--actions", "[]"],
        vec![
            "--format",
            "json",
            "set-field",
            "0",
            "1",
            "Position",
            "x",
            "--value",
            "{",
        ],
    ] {
        let mut arguments = arguments;
        arguments.extend(["--diagnostics", "never"]);
        let output = run(&arguments);
        assert!(!output.status.success());
        assert!(output.stderr.is_empty());
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "failure");
        assert_eq!(response["error"]["code"], "invalid_value");
    }
}

#[test]
fn discovery_uses_explicit_project_and_empty_registry_is_not_found() {
    let project = std::env::temp_dir().join(format!("titan-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&project).unwrap();
    let output = run(&[
        "--format",
        "json",
        "--project",
        project.to_str().unwrap(),
        "status",
    ]);
    assert!(!output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["code"], "not_found");
    let output = run(&[
        "--format",
        "json",
        "--project",
        project.to_str().unwrap(),
        "instances",
    ]);
    assert!(output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["instances"], serde_json::json!([]));
    std::fs::remove_dir_all(&project).unwrap();
}

#[test]
fn diagnostic_policy_budgets_and_write_failure_are_structured() {
    let project = std::env::temp_dir().join(format!("titan-cli-policy-{}", std::process::id()));
    std::fs::create_dir_all(&project).unwrap();
    let invoke = |args: &[&str]| {
        let mut arguments = vec!["--format", "json", "--project", project.to_str().unwrap()];
        arguments.extend_from_slice(args);
        let output = run(&arguments);
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let result = invoke(&["step", "5", "--max-frames", "4"]);
    assert_eq!(result["error"]["code"], "budget_exceeded");
    let manifest = result["error"]["details"]["diagnostic_bundle"]
        .as_str()
        .unwrap();
    let bundle: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    assert_eq!(bundle["local_error"]["error_code"], "budget_exceeded");
    assert!(
        bundle["local_error"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("exceeding")
    );
    let root = project.join("target/titan/diagnostics");
    let count = || std::fs::read_dir(&root).unwrap().count();
    assert_eq!(count(), 1);
    invoke(&["status", "--diagnostics", "never"]);
    invoke(&["info"]);
    assert_eq!(count(), 1);
    let result = invoke(&["info", "--diagnostics", "always"]);
    assert!(result["diagnostic_bundle"].as_str().is_some());
    assert_eq!(count(), 2);
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::write(&root, "blocks directory").unwrap();
    let result = invoke(&["status"]);
    assert_eq!(result["error"]["code"], "not_found");
    assert!(result["diagnostic_error"].as_str().is_some());
    std::fs::remove_dir_all(project).unwrap();
}

#[test]
fn cargo_failure_bundle_contains_bounded_logs_and_cause() {
    let project = std::env::temp_dir().join(format!("titan-cli-cargo-{}", std::process::id()));
    std::fs::create_dir_all(&project).unwrap();
    // An empty directory is an actual Cargo failure, independent of shell fixtures.
    let output = run(&[
        "--format",
        "json",
        "--project",
        project.to_str().unwrap(),
        "check",
    ]);
    assert!(!output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["error_code"], "process_failed");
    let bytes = std::fs::read(result["diagnostic_bundle"].as_str().unwrap()).unwrap();
    let bundle: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(bundle["local_error"]["error_code"], "process_failed");
    assert!(
        bundle["logs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|log| log["message"].as_str().unwrap().contains("Cargo.toml"))
    );
    assert!(bundle["timings_us"]["process"].is_u64());
    std::fs::remove_dir_all(project).unwrap();
}
