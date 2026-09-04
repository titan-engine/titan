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
    ] {
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
