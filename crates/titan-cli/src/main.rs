use std::collections::BTreeMap;
use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use titan::render::{Image, ImageDecodeLimits};
use titan_diagnostics::{
    ComparisonOptions, ComparisonReportError, ImageComparison, compare_images,
    write_comparison_report,
};
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
    /// Sets an explicitly registered writable component field.
    SetField {
        index: u32,
        generation: u32,
        component: String,
        field: String,
        /// A JSON value, validated against the field type by the runtime.
        #[arg(long, allow_hyphen_values = true)]
        value: String,
    },
    Commands,
    /// Lists registered read-only game queries.
    Queries,
    /// Reads game-owned state without changing the simulation.
    Query {
        name: String,
        #[arg(long, default_value = "{}")]
        arguments: String,
        /// Read a JSON argument object from a regular file (at most 1 MiB).
        #[arg(long, conflicts_with = "arguments")]
        arguments_file: Option<PathBuf>,
    },
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
        /// Read a JSON argument object from a regular file (at most 1 MiB).
        #[arg(long, conflicts_with = "arguments")]
        arguments_file: Option<PathBuf>,
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
    /// Compares two existing PNGs and writes a self-contained visual diff report.
    CompareImages {
        expected: PathBuf,
        actual: PathBuf,
        /// Directory below which a unique comparison report is created.
        #[arg(long)]
        output: PathBuf,
        /// Requires byte-for-byte RGBA equality.
        #[arg(
            long,
            conflicts_with_all = [
                "maximum_channel_error",
                "minimum_ssim",
                "maximum_linear_rmse"
            ]
        )]
        exact: bool,
        /// Optional maximum error for every RGBA byte (0-255).
        #[arg(long)]
        maximum_channel_error: Option<u8>,
        /// Minimum block SSIM score (-1 to 1; default 0.99).
        #[arg(long, allow_hyphen_values = true)]
        minimum_ssim: Option<f64>,
        /// Maximum linear-RGB RMSE (0 to 1; default 0.01).
        #[arg(long)]
        maximum_linear_rmse: Option<f64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
    elapsed_ms: u64,
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
    ImageComparison {
        expected: PathBuf,
        actual: PathBuf,
        width: u32,
        height: u32,
        options: ComparisonOptions,
        comparison: ImageComparison,
        artifacts: ComparisonArtifacts,
    },
}

#[derive(Serialize)]
struct ComparisonArtifacts {
    directory: PathBuf,
    manifest: PathBuf,
    expected: PathBuf,
    actual: PathBuf,
    difference: PathBuf,
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
        CliCommand::Info
            | CliCommand::Check
            | CliCommand::Test
            | CliCommand::RunExample { .. }
            | CliCommand::CompareImages { .. }
    ) {
        let mut result = execute(
            &cli.command,
            &cli.project,
            Duration::from_millis(cli.process_timeout_ms),
        );
        let mut summary = serde_json::json!({"command": result.command, "success": result.success, "exit_code": result.exit_code, "error_code": result.error_code, "elapsed_ms": result.elapsed_ms, "stdout": result.stdout, "stderr": result.stderr});
        capture_diagnostic(&cli, &mut summary, result.success);
        result.diagnostic_bundle = summary["diagnostic_bundle"].as_str().map(str::to_owned);
        result.diagnostic_error = summary["diagnostic_error"].as_str().map(str::to_owned);
        render(cli.format, &result);
        return command_exit_code(&result);
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

fn command_exit_code(result: &CommandResult) -> ExitCode {
    if result.success {
        ExitCode::SUCCESS
    } else if result.error_code == Some("visual_mismatch") {
        ExitCode::from(2)
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
    fn arguments(
        inline: &str,
        file: &Option<PathBuf>,
    ) -> Result<BTreeMap<String, serde_json::Value>, LocalError> {
        let Some(path) = file else {
            return object(inline);
        };
        const LIMIT: u64 = 1024 * 1024;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        // Opening a FIFO must not block before we can reject its file type.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let input = options.open(path).map_err(|error| {
            (
                "invalid_value".into(),
                format!("cannot open arguments file: {error}"),
            )
        })?;
        let metadata = input.metadata().map_err(|error| {
            (
                "invalid_value".into(),
                format!("cannot inspect arguments file: {error}"),
            )
        })?;
        if !metadata.is_file() || metadata.len() > LIMIT {
            return Err((
                "invalid_value".into(),
                "arguments file must be a regular file of at most 1 MiB".into(),
            ));
        }
        let mut bytes = Vec::new();
        input
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                (
                    "invalid_value".into(),
                    format!("cannot read arguments file: {error}"),
                )
            })?;
        if bytes.len() as u64 > LIMIT {
            return Err((
                "invalid_value".into(),
                "arguments file exceeds 1 MiB".into(),
            ));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            (
                "invalid_value".into(),
                "arguments file must contain UTF-8 JSON".into(),
            )
        })?;
        object(text)
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
        CliCommand::SetField {
            index,
            generation,
            component,
            field,
            value,
        } => Request::SetField {
            entity: EntityId {
                index: *index,
                generation: *generation,
            },
            component: component.clone(),
            field: field.clone(),
            value: serde_json::from_str(value).map_err(|error| {
                (
                    "invalid_value".into(),
                    format!("expected a JSON value: {error}"),
                )
            })?,
        },
        CliCommand::Commands => Request::Commands,
        CliCommand::Queries => Request::Queries,
        CliCommand::Query {
            name,
            arguments: inline,
            arguments_file,
        } => Request::Query {
            name: name.clone(),
            arguments: arguments(inline, arguments_file)?,
        },
        CliCommand::Step { frames } => Request::Step { frames: *frames },
        CliCommand::Input { frame, actions } => Request::InjectInput {
            frame: *frame,
            actions: object::<InputValue>(actions)?,
        },
        CliCommand::Invoke {
            name,
            arguments: inline,
            arguments_file,
        } => Request::Invoke {
            name: name.clone(),
            arguments: arguments(inline, arguments_file)?,
        },
        CliCommand::Capture => Request::Capture,
        _ => return Err(("unsupported".into(), "not an inspection request".into())),
    })
}

fn execute_remote(cli: &Cli) -> Result<(serde_json::Value, bool), LocalError> {
    if let CliCommand::Step { frames } = cli.command
        && frames > cli.max_frames
    {
        return Err((
            "budget_exceeded".into(),
            format!(
                "step requests {frames} frames, exceeding --max-frames {}",
                cli.max_frames
            ),
        ));
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
            error_code: None,
            elapsed_ms: 0,
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
        CliCommand::CompareImages {
            expected,
            actual,
            output,
            exact,
            maximum_channel_error,
            minimum_ssim,
            maximum_linear_rmse,
        } => compare_image_files(
            expected,
            actual,
            output,
            *exact,
            *maximum_channel_error,
            *minimum_ssim,
            *maximum_linear_rmse,
        ),
        _ => unreachable!("remote commands handled separately"),
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_image_files(
    expected_path: &Path,
    actual_path: &Path,
    output: &Path,
    exact: bool,
    maximum_channel_error: Option<u8>,
    minimum_ssim: Option<f64>,
    maximum_linear_rmse: Option<f64>,
) -> CommandResult {
    let started = std::time::Instant::now();
    let options = if exact {
        ComparisonOptions::exact()
    } else {
        let defaults = ComparisonOptions::default();
        ComparisonOptions {
            maximum_channel_error,
            minimum_ssim: minimum_ssim.unwrap_or(defaults.minimum_ssim),
            maximum_linear_rmse: maximum_linear_rmse.unwrap_or(defaults.maximum_linear_rmse),
        }
    };
    let failure = |code, message: String| CommandResult {
        command: "compare_images",
        success: false,
        exit_code: Some(1),
        stdout: String::new(),
        stderr: format!("{message}\n"),
        data: None,
        diagnostic_bundle: None,
        diagnostic_error: None,
        error_code: Some(code),
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    };
    let expected = match read_png(expected_path) {
        Ok(image) => image,
        Err(error) => return failure("invalid_value", error),
    };
    let actual = match read_png(actual_path) {
        Ok(image) => image,
        Err(error) => return failure("invalid_value", error),
    };
    let comparison = match compare_images(&expected, &actual, options) {
        Ok(comparison) => comparison,
        Err(error) => return failure("invalid_value", error.to_string()),
    };
    let written = match write_comparison_report(output, &expected, &actual, options) {
        Ok(written) => written,
        Err(ComparisonReportError::Comparison(error)) => {
            return failure("invalid_value", error.to_string());
        }
        Err(error) => return failure("artifact_write_failed", error.to_string()),
    };
    let success = comparison.passes;
    CommandResult {
        command: "compare_images",
        success,
        exit_code: Some(if success { 0 } else { 2 }),
        stdout: String::new(),
        stderr: String::new(),
        data: Some(CommandData::ImageComparison {
            expected: expected_path.to_path_buf(),
            actual: actual_path.to_path_buf(),
            width: expected.width(),
            height: expected.height(),
            options,
            comparison,
            artifacts: ComparisonArtifacts {
                directory: written.directory,
                manifest: written.manifest,
                expected: written.expected,
                actual: written.actual,
                difference: written.difference,
            },
        }),
        diagnostic_bundle: None,
        diagnostic_error: None,
        error_code: (!success).then_some("visual_mismatch"),
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    }
}

fn read_png(path: &Path) -> Result<Image, String> {
    let limits = ImageDecodeLimits::default();
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let input = options
        .open(path)
        .map_err(|error| format!("cannot open PNG `{}`: {error}", path.display()))?;
    let metadata = input
        .metadata()
        .map_err(|error| format!("cannot inspect PNG `{}`: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("PNG `{}` must be a regular file", path.display()));
    }
    if metadata.len() > limits.max_encoded_bytes as u64 {
        return Err(format!(
            "PNG `{}` exceeds the {} byte encoded input limit",
            path.display(),
            limits.max_encoded_bytes
        ));
    }
    let mut bytes = Vec::new();
    input
        .take(limits.max_encoded_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read PNG `{}`: {error}", path.display()))?;
    Image::from_png(&bytes, limits)
        .map_err(|error| format!("cannot decode PNG `{}`: {error}", path.display()))
}

fn run_cargo(
    command_name: &'static str,
    project: &Path,
    arguments: &[&str],
    timeout: Duration,
) -> CommandResult {
    let mut command = Command::new(cargo_program());
    command.args(arguments).current_dir(project);
    let started = std::time::Instant::now();
    let output = process::run(&mut command, timeout);
    let (success, exit_code, stdout, stderr, error_code) = match output {
        Ok(output) => (
            output.success,
            output.exit_code,
            output.stdout,
            output.stderr,
            if output.timed_out {
                Some("timeout")
            } else if !output.success {
                Some("process_failed")
            } else {
                None
            },
        ),
        Err(error) => (
            false,
            None,
            String::new(),
            format!("failed to execute Cargo: {error}"),
            Some("spawn_failed"),
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
        error_code,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
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
/// request arguments or environment variables. Child logs are bounded by process::run.
fn capture_diagnostic(cli: &Cli, result: &mut serde_json::Value, success: bool) {
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
    if let Some(path) = &result.diagnostic_bundle {
        eprintln!("Diagnostics: {path}");
    }
    if let Some(error) = &result.diagnostic_error {
        eprintln!("Diagnostic capture failed: {error}");
    }

    if let Some(CommandData::Info {
        cli_version,
        protocol_schema,
    }) = &result.data
    {
        println!("Titan CLI {cli_version}");
        println!("Inspection protocol schema {protocol_schema}");
        return;
    }

    if let Some(CommandData::ImageComparison {
        width,
        height,
        options,
        comparison,
        artifacts,
        ..
    }) = &result.data
    {
        println!(
            "Image comparison: {}",
            if comparison.passes {
                "PASS"
            } else {
                "MISMATCH"
            }
        );
        println!("Dimensions: {width}x{height}");
        println!(
            "Exact: {}; differing pixels: {}; maximum channel error: {}",
            comparison.exact, comparison.differing_pixels, comparison.maximum_channel_error
        );
        println!(
            "Mean absolute channel error: {:.6}; linear RMSE: {:.6} (maximum {:.6}); SSIM: {:.6} (minimum {:.6})",
            comparison.mean_absolute_channel_error,
            comparison.linear_rmse,
            options.maximum_linear_rmse,
            comparison.ssim,
            options.minimum_ssim
        );
        if let Some(maximum) = options.maximum_channel_error {
            println!("Maximum channel error threshold: {maximum}");
        }
        println!("Report: {}", artifacts.manifest.display());
        println!("Difference image: {}", artifacts.difference.display());
        return;
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
    fn argument_files_are_bounded_objects_and_conflict_with_inline_arguments() {
        let path =
            std::env::temp_dir().join(format!("titan-argument-file-{}.json", std::process::id()));
        let path_str = path.to_str().unwrap();
        for command in ["query", "invoke"] {
            std::fs::write(&path, br#"{"save":{"format_version":1}}"#).unwrap();
            let cli = Cli::try_parse_from(["titan", command, "save", "--arguments-file", path_str])
                .unwrap();
            let request = super::request_for(&cli.command).unwrap();
            let values = match request {
                titan_protocol::Request::Query { arguments, .. }
                | titan_protocol::Request::Invoke { arguments, .. } => arguments,
                other => panic!("unexpected request {other:?}"),
            };
            assert_eq!(values["save"]["format_version"], 1);
            assert!(
                Cli::try_parse_from([
                    "titan",
                    command,
                    "save",
                    "--arguments",
                    "{}",
                    "--arguments-file",
                    path_str
                ])
                .is_err()
            );
            for invalid in [b"[]".to_vec(), vec![0xff], vec![b' '; 1024 * 1024 + 1]] {
                std::fs::write(&path, invalid).unwrap();
                assert_eq!(
                    super::request_for(&cli.command).unwrap_err().0,
                    "invalid_value"
                );
            }
            std::fs::remove_file(&path).unwrap();
            assert_eq!(
                super::request_for(&cli.command).unwrap_err().0,
                "invalid_value"
            );
        }
    }

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
    fn read_only_query_arguments_are_parsed_before_discovery() {
        let cli = Cli::try_parse_from([
            "titan",
            "query",
            "arena_state",
            "--arguments",
            r#"{"limit":2}"#,
        ])
        .unwrap();
        assert!(
            matches!(super::request_for(&cli.command).unwrap(), titan_protocol::Request::Query { name, arguments } if name == "arena_state" && arguments["limit"] == 2)
        );
        let invalid =
            Cli::try_parse_from(["titan", "query", "recording", "--arguments", "[]"]).unwrap();
        assert!(super::request_for(&invalid.command).is_err());
        let list = Cli::try_parse_from(["titan", "queries"]).unwrap();
        assert!(matches!(
            super::request_for(&list.command).unwrap(),
            titan_protocol::Request::Queries
        ));
    }

    #[test]
    fn set_field_preserves_json_value_types_and_entity_identity() {
        for value in [
            "true",
            "false",
            "42",
            "-3.5",
            r#""hello""#,
            "null",
            "[1,2]",
            r#"{"nested":true}"#,
        ] {
            let cli = Cli::try_parse_from([
                "titan",
                "set-field",
                "7",
                "3",
                "Position",
                "x",
                "--value",
                value,
            ])
            .unwrap();
            assert_eq!(
                super::request_for(&cli.command).unwrap(),
                titan_protocol::Request::SetField {
                    entity: titan_protocol::EntityId {
                        index: 7,
                        generation: 3
                    },
                    component: "Position".into(),
                    field: "x".into(),
                    value: serde_json::from_str(value).unwrap(),
                }
            );
        }
        for value in ["", "{", "undefined", "NaN", "1 2"] {
            let cli = Cli::try_parse_from([
                "titan",
                "set-field",
                "7",
                "3",
                "Position",
                "x",
                "--value",
                value,
            ])
            .unwrap();
            assert_eq!(
                super::request_for(&cli.command).unwrap_err().0,
                "invalid_value"
            );
        }
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
    fn fallback_bundle_preserves_protocol_response_evidence() {
        let project =
            std::env::temp_dir().join(format!("titan-cli-response-{}", std::process::id()));
        let cli = Cli::try_parse_from(["titan", "--project", project.to_str().unwrap(), "status"])
            .unwrap();
        let mut result = serde_json::json!({
            "schema_version": 1, "request_id": "test", "instance_id": "one",
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
