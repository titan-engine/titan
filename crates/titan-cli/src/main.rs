use std::env;
use std::path::Path;
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use titan_protocol::SCHEMA_VERSION;

#[derive(Parser)]
#[command(name = "titan", version, about = "Agent-friendly Titan game workflows")]
struct Cli {
    /// Selects human-readable or stable machine-readable output.
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Reports CLI and inspection protocol versions.
    Info,
    /// Checks all workspace targets with Cargo.
    Check,
    /// Tests all workspace targets with Cargo.
    Test,
    /// Runs a named Cargo example.
    RunExample { name: String },
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
    let cli = Cli::parse();
    let result = execute(cli.command);
    render(cli.format, &result);
    if result.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn execute(command: CliCommand) -> CommandResult {
    match command {
        CliCommand::Info => CommandResult {
            command: "info",
            success: true,
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            data: Some(CommandData::Info {
                cli_version: env!("CARGO_PKG_VERSION"),
                protocol_schema: SCHEMA_VERSION,
            }),
        },
        CliCommand::Check => run_cargo(
            "check",
            &["check", "--workspace", "--all-targets", "--all-features"],
        ),
        CliCommand::Test => run_cargo("test", &["test", "--workspace", "--all-targets"]),
        CliCommand::RunExample { name } => {
            run_cargo("run_example", &["run", "--example", name.as_str()])
        }
    }
}

fn run_cargo(command_name: &'static str, arguments: &[&str]) -> CommandResult {
    match Command::new(cargo_program())
        .args(arguments)
        .current_dir(project_directory())
        .output()
    {
        Ok(output) => CommandResult {
            command: command_name,
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            data: Some(CommandData::Process {
                program: cargo_program(),
                arguments: arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
            }),
        },
        Err(error) => CommandResult {
            command: command_name,
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: format!("failed to execute Cargo: {error}"),
            data: None,
        },
    }
}

fn cargo_program() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn project_directory() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("titan-cli must be inside the Titan workspace")
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
        let result = execute(CliCommand::Info);
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
}
