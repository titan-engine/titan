use crate::{args::Cli, dispatch::LocalError};
use std::time::Duration;
use titan_protocol::{Request, RequestEnvelope, ResponseOutcome};

pub(crate) fn execute_remote(
    cli: &Cli,
    request: Option<Request>,
) -> Result<(serde_json::Value, bool), LocalError> {
    if let Some(Request::Step { frames }) = &request
        && *frames > cli.max_frames
    {
        return Err((
            "budget_exceeded".into(),
            format!(
                "step requests {frames} frames, exceeding --max-frames {}",
                cli.max_frames
            ),
        ));
    }
    let directory = titan_remote::registry_dir(&cli.project);
    let registrations = titan_remote::discover(&directory, &cli.project).map_err(remote_error)?;
    let Some(request) = request else {
        let instances: Vec<_> = registrations.iter().map(public_registration).collect();
        return Ok((
            serde_json::json!({"status": "success", "instances": instances}),
            true,
        ));
    };
    let registration =
        titan_remote::select(&registrations, cli.instance.as_deref()).map_err(remote_error)?;
    let mut envelope = RequestEnvelope::new(format!("cli-{}", std::process::id()), request);
    envelope.target_instance = cli.instance.clone();
    let response = titan_remote::send(
        &registration,
        &envelope,
        Duration::from_millis(cli.timeout_ms),
    )
    .map_err(remote_error)?;
    let success = matches!(response.outcome, ResponseOutcome::Success { .. });
    Ok((
        serde_json::to_value(response).expect("response serializes"),
        success,
    ))
}

fn public_registration(registration: &titan_remote::Registration) -> serde_json::Value {
    let value = serde_json::to_value(registration).expect("registration serializes");
    let mut public = serde_json::Map::new();
    for key in [
        "instance_id",
        "project",
        "pid",
        "endpoint",
        "schema_version",
        "run_mode",
    ] {
        if let Some(value) = value.get(key) {
            public.insert(key.to_owned(), value.clone());
        }
    }
    serde_json::Value::Object(public)
}

fn remote_error(error: titan_remote::RemoteError) -> LocalError {
    use titan_remote::RemoteError;
    let code = match &error {
        RemoteError::NotFound => "not_found",
        RemoteError::AmbiguousTarget => "ambiguous_target",
        RemoteError::Busy => "busy",
        RemoteError::Timeout => "timeout",
        RemoteError::Invalid(_) | RemoteError::Json(_) => "invalid_value",
        RemoteError::Unauthorized => "unauthorized",
        RemoteError::Io(_) => "internal",
    };
    (code.into(), error.to_string())
}

#[cfg(test)]
mod tests {

    #[test]
    fn public_instances_never_include_authentication_tokens() {
        let registration = titan_remote::Registration {
            instance_id: "one".into(),
            project: "/tmp/game".into(),
            pid: 1,
            endpoint: "http://127.0.0.1:1234/request".into(),
            schema_version: titan_protocol::SCHEMA_VERSION,
            run_mode: titan_protocol::RunMode::Headless,
            token: "super-secret".into(),
        };
        let public = super::public_registration(&registration);
        assert_eq!(public["instance_id"], "one");
        assert!(public.get("token").is_none());
        assert!(!public.to_string().contains("super-secret"));
    }

    #[test]
    fn selection_errors_preserve_machine_readable_codes() {
        assert_eq!(
            super::remote_error(titan_remote::RemoteError::NotFound).0,
            "not_found"
        );
        assert_eq!(
            super::remote_error(titan_remote::RemoteError::AmbiguousTarget).0,
            "ambiguous_target"
        );
    }
}
