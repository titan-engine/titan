use std::process::Command;

use titan::render::{Image, ImageDecodeLimits};

fn run(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_titan"))
        .args(arguments)
        .output()
        .unwrap()
}

fn temporary_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "titan-cli-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn write_png(path: &std::path::Path, width: u32, height: u32, pixels: Vec<u8>) {
    let image = Image::new(width, height, pixels).unwrap();
    let mut bytes = Vec::new();
    titan_diagnostics::write_png(&image, &mut bytes).unwrap();
    std::fs::write(path, bytes).unwrap();
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

#[test]
fn image_comparison_reports_exact_pass_and_mismatch_with_artifacts() {
    let root = temporary_path("image-comparison");
    std::fs::create_dir_all(&root).unwrap();
    let expected = root.join("expected.png");
    let actual = root.join("actual.png");
    let reports = root.join("reports");
    write_png(&expected, 2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    write_png(&actual, 2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255]);

    let arguments = [
        "--format",
        "json",
        "--diagnostics",
        "never",
        "compare-images",
        expected.to_str().unwrap(),
        actual.to_str().unwrap(),
        "--output",
        reports.to_str().unwrap(),
        "--exact",
    ];
    let output = run(&arguments);
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["command"], "compare_images");
    assert_eq!(result["success"], true);
    assert_eq!(result["data"]["type"], "image_comparison");
    assert_eq!(result["data"]["comparison"]["exact"], true);
    assert_eq!(result["data"]["comparison"]["passes"], true);
    assert_eq!(result["data"]["comparison"]["differing_pixels"], 0);

    write_png(&actual, 2, 1, vec![10, 20, 30, 255, 41, 50, 60, 255]);
    let output = run(&arguments);
    assert_eq!(output.status.code(), Some(2));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["success"], false);
    assert_eq!(result["exit_code"], 2);
    assert_eq!(result["error_code"], "visual_mismatch");
    assert_eq!(result["data"]["comparison"]["passes"], false);
    assert_eq!(result["data"]["comparison"]["differing_pixels"], 1);
    let manifest = result["data"]["artifacts"]["manifest"].as_str().unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    assert_eq!(report["comparison"], result["data"]["comparison"]);
    for artifact in ["expected", "actual", "difference"] {
        let path = result["data"]["artifacts"][artifact].as_str().unwrap();
        let bytes = std::fs::read(path).unwrap();
        Image::from_png(&bytes, ImageDecodeLimits::default()).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn image_comparison_applies_explicit_tolerances() {
    let root = temporary_path("image-tolerance");
    std::fs::create_dir_all(&root).unwrap();
    let expected = root.join("expected.png");
    let actual = root.join("actual.png");
    let reports = root.join("reports");
    write_png(&expected, 1, 1, vec![100, 100, 100, 255]);
    write_png(&actual, 1, 1, vec![102, 100, 100, 255]);

    let invoke = |maximum_channel_error: &str| {
        run(&[
            "--format",
            "json",
            "--diagnostics",
            "never",
            "compare-images",
            expected.to_str().unwrap(),
            actual.to_str().unwrap(),
            "--output",
            reports.to_str().unwrap(),
            "--maximum-channel-error",
            maximum_channel_error,
            "--minimum-ssim",
            "-1",
            "--maximum-linear-rmse",
            "1",
        ])
    };
    let passing = invoke("2");
    assert!(passing.status.success());
    let result: serde_json::Value = serde_json::from_slice(&passing.stdout).unwrap();
    assert_eq!(result["data"]["comparison"]["exact"], false);
    assert_eq!(result["data"]["comparison"]["passes"], true);
    assert_eq!(result["data"]["options"]["maximum_channel_error"], 2);

    let failing = invoke("1");
    assert_eq!(failing.status.code(), Some(2));
    let result: serde_json::Value = serde_json::from_slice(&failing.stdout).unwrap();
    assert_eq!(result["error_code"], "visual_mismatch");
    assert_eq!(result["data"]["comparison"]["passes"], false);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn image_comparison_rejects_invalid_bounded_inputs_and_write_failures() {
    let root = temporary_path("image-errors");
    std::fs::create_dir_all(&root).unwrap();
    let valid = root.join("valid.png");
    let malformed = root.join("malformed.png");
    let oversized = root.join("oversized.png");
    let unequal = root.join("unequal.png");
    let blocked_output = root.join("blocked-output");
    write_png(&valid, 1, 1, vec![0, 0, 0, 255]);
    write_png(&unequal, 2, 1, vec![0; 8]);
    std::fs::write(&malformed, b"not a PNG").unwrap();
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    std::fs::write(&blocked_output, b"not a directory").unwrap();

    let invoke = |left: &std::path::Path,
                  right: &std::path::Path,
                  output: &std::path::Path,
                  extra: &[&str]| {
        let mut arguments = vec![
            "--format",
            "json",
            "--diagnostics",
            "never",
            "compare-images",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ];
        arguments.extend_from_slice(extra);
        run(&arguments)
    };
    for (left, right, extra) in [
        (malformed.as_path(), valid.as_path(), Vec::<&str>::new()),
        (oversized.as_path(), valid.as_path(), Vec::<&str>::new()),
        (valid.as_path(), unequal.as_path(), Vec::<&str>::new()),
        (
            valid.as_path(),
            valid.as_path(),
            vec!["--minimum-ssim", "nan"],
        ),
    ] {
        let output = invoke(left, right, &root.join("reports"), &extra);
        assert_eq!(output.status.code(), Some(1));
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["error_code"], "invalid_value");
        assert!(result["data"].is_null());
    }

    let output = invoke(&valid, &valid, &blocked_output, &[]);
    assert_eq!(output.status.code(), Some(1));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["error_code"], "artifact_write_failed");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn image_comparison_rejects_non_utf8_paths_without_panicking_or_writing() {
    use std::os::unix::ffi::OsStringExt;

    let root = temporary_path("image-non-utf8");
    std::fs::create_dir_all(&root).unwrap();
    let valid = root.join("valid.png");
    let reports = root.join("reports");
    write_png(&valid, 1, 1, vec![0, 0, 0, 255]);
    let invalid = std::ffi::OsString::from_vec(b"invalid-\xff.png".to_vec());
    let output = Command::new(env!("CARGO_BIN_EXE_titan"))
        .arg("--format")
        .arg("json")
        .arg("--diagnostics")
        .arg("never")
        .arg("compare-images")
        .arg(invalid)
        .arg(&valid)
        .arg("--output")
        .arg(&reports)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["error_code"], "invalid_value");
    assert!(
        result["stderr"]
            .as_str()
            .unwrap()
            .contains("representable as UTF-8")
    );
    assert!(!reports.exists());
    std::fs::remove_dir_all(root).unwrap();
}
