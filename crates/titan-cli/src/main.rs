use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use titan_protocol::{
    EntityId, EntityQuery, InputValue, PageRequest, Request, RequestEnvelope, ResponseOutcome,
    SCHEMA_VERSION,
};

#[derive(Parser)]
#[command(name = "titan", version, about = "Agent-friendly Titan game workflows")]
struct Cli {
    /// Selects human-readable or stable machine-readable output.
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
    /// Project directory whose local runtime registry should be used.
    #[arg(long, global = true, default_value = ".")]
    project: PathBuf,
    /// Runtime instance ID; required when multiple runtimes match.
    #[arg(long, global = true)]
    instance: Option<String>,
    /// Maximum duration of an inspection request in milliseconds.
    #[arg(long, global = true, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
    timeout_ms: u64,
    /// Diagnostic capture policy.
    #[arg(long, global = true, value_enum, default_value_t = CapturePolicy::OnFailure)]
    diagnostics: CapturePolicy,
    /// Maximum frames accepted by a single step request.
    #[arg(long, global = true, default_value_t = 10000, value_parser = clap::value_parser!(u64).range(1..))]
    max_frames: u64,
    /// Wall-clock limit for Cargo workflows, including compilation.
    #[arg(long, global = true, default_value_t = 120000, value_parser = clap::value_parser!(u64).range(1..))]
    process_timeout_ms: u64,
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CapturePolicy {
    OnFailure,
    Always,
    Never,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Reports CLI and inspection protocol versions.
    Info,
    /// Lists discoverable runtimes without authentication tokens.
    Instances,
    Capabilities,
    Status,
    Entities {
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "component")]
        components: Vec<String>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..))]
        limit: u32,
    },
    Entity {
        index: u32,
        generation: u32,
    },
    Commands,
    Step {
        frames: u64,
    },
    /// Queues action values (a JSON object) for a future frame.
    Input {
        frame: u64,
        #[arg(long)]
        actions: String,
    },
    /// Invokes a registered game command with a JSON object of arguments.
    Invoke {
        name: String,
        #[arg(long, default_value = "{}")]
        arguments: String,
    },
    Capture,
    /// Checks all workspace targets with Cargo.
    Check,
    /// Tests all workspace targets with Cargo.
    Test,
    /// Runs a named Cargo example.
    RunExample {
        name: String,
    },
}

#[derive(Serialize)]
struct CommandResult {
    command: &'static str,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    stdout: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<CommandData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_bundle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_error: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CommandData {
    Info {
        cli_version: &'static str,
        protocol_schema: u32,
    },
    Process {
        program: String,
        arguments: Vec<String>,
    },
}

fn main() -> ExitCode {
    let arguments: Vec<_> = env::args_os().collect();
    let json = arguments
        .windows(2)
        .any(|pair| pair[0] == "--format" && pair[1] == "json")
        || arguments.iter().any(|arg| arg == "--format=json");
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) => {
            if error.use_stderr() && json {
                println!("{}", local_failure("invalid_value", error.to_string()));
            } else {
                let _ = error.print();
            }
            return if error.use_stderr() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    if matches!(
        cli.command,
        CliCommand::Info | CliCommand::Check | CliCommand::Test | CliCommand::RunExample { .. }
    ) {
        let mut result = execute(
            &cli.command,
            &cli.project,
            Duration::from_millis(cli.process_timeout_ms),
        );
        let mut summary = serde_json::json!({"command": result.command, "success": result.success, "exit_code": result.exit_code});
        capture_diagnostic(&cli, &mut summary, result.success);
        result.diagnostic_bundle = summary["diagnostic_bundle"].as_str().map(str::to_owned);
        result.diagnostic_error = summary["diagnostic_error"].as_str().map(str::to_owned);
        render(cli.format, &result);
        return if result.success {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let (mut result, success) = match execute_remote(&cli) {
        Ok(result) => result,
        Err((code, message)) => (local_failure(&code, message), false),
    };
    capture_diagnostic(&cli, &mut result, success);
    match cli.format {
        OutputFormat::Json => println!("{result}"),
        OutputFormat::Human => println!(
            "{}",
            serde_json::to_string_pretty(&result).expect("JSON value serializes")
        ),
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn local_failure(code: &str, message: String) -> serde_json::Value {
    serde_json::json!({"status": "failure", "error": {"code": code, "message": message, "details": {}, "retryable": false}})
}

type LocalError = (String, String);

fn request_for(command: &CliCommand) -> Result<Request, LocalError> {
    fn object<T: serde::de::DeserializeOwned>(
        value: &str,
    ) -> Result<BTreeMap<String, T>, LocalError> {
        serde_json::from_str(value).map_err(|error| {
            (
                "invalid_value".into(),
                format!("expected a JSON object: {error}"),
            )
        })
    }
    Ok(match command {
        CliCommand::Capabilities => Request::Capabilities,
        CliCommand::Status => Request::Status,
        CliCommand::Entities {
            name,
            components,
            cursor,
            limit,
        } => Request::Entities {
            query: EntityQuery {
                name: name.clone(),
                with_components: components.clone(),
            },
            page: PageRequest {
                cursor: cursor.clone(),
                limit: *limit,
            },
        },
        CliCommand::Entity { index, generation } => Request::Entity {
            entity: EntityId {
                index: *index,
                generation: *generation,
            },
        },
        CliCommand::Commands => Request::Commands,
        CliCommand::Step { frames } => Request::Step { frames: *frames },
        CliCommand::Input { frame, actions } => Request::InjectInput {
            frame: *frame,
            actions: object::<InputValue>(actions)?,
        },
        CliCommand::Invoke { name, arguments } => Request::Invoke {
            name: name.clone(),
            arguments: object(arguments)?,
        },
        CliCommand::Capture => Request::Capture,
        _ => return Err(("unsupported".into(), "not an inspection request".into())),
    })
}

fn execute_remote(cli: &Cli) -> Result<(serde_json::Value, bool), LocalError> {
    if let CliCommand::Step { frames } = cli.command {
        if frames > cli.max_frames {
            return Err((
                "budget_exceeded".into(),
                format!(
                    "step requests {frames} frames, exceeding --max-frames {}",
                    cli.max_frames
                ),
            ));
        }
    }
    // Parse payloads before discovery so invalid input has a useful local error.
    let request = if matches!(cli.command, CliCommand::Instances) {
        None
    } else {
        Some(request_for(&cli.command)?)
    };
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

fn execute(command: &CliCommand, project: &Path, timeout: Duration) -> CommandResult {
    match command {
        CliCommand::Info => CommandResult {
            command: "info",
            success: true,
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            diagnostic_bundle: None,
            diagnostic_error: None,
            data: Some(CommandData::Info {
                cli_version: env!("CARGO_PKG_VERSION"),
                protocol_schema: SCHEMA_VERSION,
            }),
        },
        CliCommand::Check => run_cargo(
            "check",
            project,
            &["check", "--workspace", "--all-targets", "--all-features"],
            timeout,
        ),
        CliCommand::Test => run_cargo(
            "test",
            project,
            &["test", "--workspace", "--all-targets"],
            timeout,
        ),
        CliCommand::RunExample { name } => run_cargo(
            "run_example",
            project,
            &["run", "--example", name.as_str()],
            timeout,
        ),
        _ => unreachable!("remote commands handled separately"),
    }
}

fn run_cargo(
    command_name: &'static str,
    project: &Path,
    arguments: &[&str],
    timeout: Duration,
) -> CommandResult {
    let mut command = Command::new(cargo_program());
    command.args(arguments).current_dir(project);
    let output = process::run(&mut command, timeout);
    let (success, exit_code, stdout, stderr) = match output {
        Ok(output) => (
            output.success,
            output.exit_code,
            output.stdout,
            output.stderr,
        ),
        Err(error) => (
            false,
            None,
            String::new(),
            format!("failed to execute Cargo: {error}"),
        ),
    };
    CommandResult {
        command: command_name,
        success,
        exit_code,
        stdout,
        stderr,
        diagnostic_bundle: None,
        diagnostic_error: None,
        data: Some(CommandData::Process {
            program: cargo_program(),
            arguments: arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
        }),
    }
}

mod process;

/// Persist an allowlisted summary: never transport registration/authentication,
/// request arguments, environment variables, or arbitrary child output.
fn capture_diagnostic(cli: &Cli, result: &mut serde_json::Value, success: bool) {
    if matches!(cli.diagnostics, CapturePolicy::Never)
        || (success && matches!(cli.diagnostics, CapturePolicy::OnFailure))
        || result.pointer("/error/details/diagnostic_bundle").is_some()
    {
        return;
    }
    let summary = serde_json::json!({
        "success": success,
        "error_code": result.pointer("/error/code"),
        "command": result.get("command"),
        "exit_code": result.get("exit_code"),
    });
    let mut bundle = titan_diagnostics::DiagnosticBundle::local_failure(summary);
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

fn cargo_program() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn render(format: OutputFormat, result: &CommandResult) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string(result).expect("command result must serialize")
            );
        }
        OutputFormat::Human => render_human(result),
    }
}

fn render_human(result: &CommandResult) {
    if let Some(CommandData::Info {
        cli_version,
        protocol_schema,
    }) = &result.data
    {
        println!("Titan CLI {cli_version}");
        println!("Inspection protocol schema {protocol_schema}");
        return;
    }

    if let Some(path) = &result.diagnostic_bundle {
        eprintln!("Diagnostics: {path}");
    }
    if let Some(error) = &result.diagnostic_error {
        eprintln!("Diagnostic capture failed: {error}");
    }
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    if result.success {
        eprintln!("Titan command `{}` completed successfully.", result.command);
    } else {
        eprintln!("Titan command `{}` failed.", result.command);
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, CliCommand, CommandData, execute};

    #[test]
    fn info_is_a_structured_result() {
        let result = execute(
            &CliCommand::Info,
            std::path::Path::new("."),
            std::time::Duration::from_secs(1),
        );
        let json = serde_json::to_value(result).unwrap();

        assert_eq!(json["command"], "info");
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["type"], "info");
        assert_eq!(json["data"]["protocol_schema"], 1);
    }

    #[test]
    fn output_format_is_global() {
        let before = Cli::try_parse_from(["titan", "--format", "json", "info"]);
        let after = Cli::try_parse_from(["titan", "info", "--format", "json"]);

        assert!(before.is_ok());
        assert!(after.is_ok());
    }

    #[test]
    fn process_data_has_an_explicit_command_shape() {
        let data = CommandData::Process {
            program: "cargo".to_owned(),
            arguments: vec!["check".to_owned()],
        };
        let json = serde_json::to_value(data).unwrap();

        assert_eq!(json["type"], "process");
        assert_eq!(json["arguments"][0], "check");
    }
    #[test]
    fn remote_payloads_are_typed_and_reject_invalid_json() {
        let cli = Cli::try_parse_from([
            "titan",
            "--project",
            "/tmp/game",
            "--instance",
            "one",
            "step",
            "12",
            "--timeout-ms",
            "40",
        ])
        .unwrap();
        assert_eq!(cli.project, std::path::Path::new("/tmp/game"));
        assert_eq!(cli.instance.as_deref(), Some("one"));
        assert_eq!(
            super::request_for(&cli.command).unwrap(),
            titan_protocol::Request::Step { frames: 12 }
        );
        for arguments in ["[1]", "null", "broken"] {
            let cli = Cli::try_parse_from(["titan", "invoke", "reset", "--arguments", arguments])
                .unwrap();
            assert_eq!(
                super::request_for(&cli.command).unwrap_err().0,
                "invalid_value"
            );
        }
        let cli = Cli::try_parse_from([
            "titan",
            "input",
            "3",
            "--actions",
            r#"{"jump":{"kind":"button","value":true}}"#,
        ])
        .unwrap();
        assert!(matches!(
            super::request_for(&cli.command).unwrap(),
            titan_protocol::Request::InjectInput { frame: 3, .. }
        ));
        assert!(Cli::try_parse_from(["titan", "status", "--timeout-ms", "0"]).is_err());
        assert!(Cli::try_parse_from(["titan", "entities", "--limit", "0"]).is_err());
    }

    #[test]
    fn public_instances_never_include_authentication_tokens() {
        let registration = titan_remote::Registration {
            instance_id: "one".into(),
            project: "/tmp/game".into(),
            pid: 1,
            endpoint: "http://127.0.0.1:1234/request".into(),
            schema_version: 1,
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
