use crate::args::{CapturePolicy, Cli};

/// Persist an allowlisted summary: never transport registration/authentication,
/// request arguments or environment variables. Child logs are bounded by process::run.
pub(crate) fn capture_diagnostic(cli: &Cli, result: &mut serde_json::Value, success: bool) {
    if matches!(cli.diagnostics, CapturePolicy::Never)
        || (success && matches!(cli.diagnostics, CapturePolicy::OnFailure))
        || result.pointer("/error/details/diagnostic_bundle").is_some()
    {
        return;
    }
    let summary = serde_json::json!({
        "success": success,
        "error": result.get("error"),
        "error_code": result.pointer("/error/code").or_else(|| result.get("error_code")),
        "command": result.get("command"),
        "exit_code": result.get("exit_code"),
    });
    let mut bundle = titan_diagnostics::DiagnosticBundle::local_failure(summary.clone());
    if let Ok(response) = serde_json::from_value::<titan_protocol::ResponseEnvelope>(result.clone())
    {
        bundle.response = Some(response);
        bundle.local_error = None;
    } else if success {
        bundle.local_error = None;
    }
    bundle.context.insert("result".into(), summary);
    if let Some(elapsed) = result["elapsed_ms"].as_u64() {
        bundle
            .timings_us
            .insert("process".into(), elapsed.saturating_mul(1000));
    }
    for stream in ["stdout", "stderr"] {
        if let Some(message) = result[stream]
            .as_str()
            .filter(|message| !message.is_empty())
        {
            bundle.logs.push(titan_diagnostics::DiagnosticLog {
                level: stream.into(),
                message: message.into(),
                frame: None,
            });
        }
    }
    bundle.context.insert("source".into(), "titan-cli".into());
    let root = cli.project.join("target/titan/diagnostics");
    match titan_diagnostics::write_bundle(&root, &bundle, None) {
        Ok(written) => {
            let path = serde_json::Value::String(written.manifest.to_string_lossy().into_owned());
            if let Some(details) = result
                .pointer_mut("/error/details")
                .and_then(serde_json::Value::as_object_mut)
            {
                details.insert("diagnostic_bundle".into(), path);
            } else {
                result["diagnostic_bundle"] = path;
            }
        }
        Err(error) => {
            result["diagnostic_error"] = error.to_string().into();
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::args::Cli;
    use clap::Parser;

    #[test]
    fn fallback_bundle_preserves_protocol_response_evidence() {
        let project =
            std::env::temp_dir().join(format!("titan-cli-response-{}", std::process::id()));
        let cli = Cli::try_parse_from(["titan", "--project", project.to_str().unwrap(), "status"])
            .unwrap();
        let mut result = serde_json::json!({
            "schema_version": titan_protocol::SCHEMA_VERSION, "request_id": "test", "instance_id": "one",
            "observed_frame": 42, "state_revision": 9, "status": "failure",
            "error": {"code": "invalid_value", "message": "specific failure reason", "details": {"key": "value"}, "retryable": false}
        });
        let response = result.clone();
        super::capture_diagnostic(&cli, &mut result, false);
        let manifest = result["error"]["details"]["diagnostic_bundle"]
            .as_str()
            .unwrap();
        let bundle: serde_json::Value =
            serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        assert_eq!(bundle["response"], response);
        assert!(bundle["local_error"].is_null());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn runtime_bundle_is_preserved_without_local_duplicate() {
        let project =
            std::env::temp_dir().join(format!("titan-cli-preserve-{}", std::process::id()));
        let cli = Cli::try_parse_from([
            "titan",
            "--project",
            project.to_str().unwrap(),
            "status",
            "--diagnostics",
            "always",
        ])
        .unwrap();
        let mut result = serde_json::json!({"status": "failure", "error": {"details": {"diagnostic_bundle": "/runtime/bundle.json"}}});
        super::capture_diagnostic(&cli, &mut result, false);
        assert_eq!(
            result["error"]["details"]["diagnostic_bundle"],
            "/runtime/bundle.json"
        );
        assert!(!project.exists());
    }
}
