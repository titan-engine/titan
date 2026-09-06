use crate::{
    comparison::compare_image_files,
    output::{CommandData, CommandResult},
    process,
};
use std::env;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use titan_protocol::SCHEMA_VERSION;

/// Local operations cannot contain an inspection command.
pub(crate) enum LocalCommand<'a> {
    Info,
    Check,
    Test,
    RunExample {
        name: &'a str,
    },
    CompareImages {
        expected: &'a Path,
        actual: &'a Path,
        output: &'a Path,
        exact: bool,
        maximum_channel_error: Option<u8>,
        minimum_ssim: Option<f64>,
        maximum_linear_rmse: Option<f64>,
    },
}

pub(crate) fn execute(
    command: LocalCommand<'_>,
    project: &Path,
    timeout: Duration,
) -> CommandResult {
    match command {
        LocalCommand::Info => CommandResult {
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
        LocalCommand::Check => run_cargo(
            "check",
            project,
            &["check", "--workspace", "--all-targets", "--all-features"],
            timeout,
        ),
        LocalCommand::Test => run_cargo(
            "test",
            project,
            &["test", "--workspace", "--all-targets"],
            timeout,
        ),
        LocalCommand::RunExample { name } => {
            run_cargo("run_example", project, &["run", "--example", name], timeout)
        }
        LocalCommand::CompareImages {
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
            exact,
            maximum_channel_error,
            minimum_ssim,
            maximum_linear_rmse,
        ),
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

fn cargo_program() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn info_is_a_structured_result() {
        let result = execute(
            super::LocalCommand::Info,
            std::path::Path::new("."),
            std::time::Duration::from_secs(1),
        );
        let json = serde_json::to_value(result).unwrap();

        assert_eq!(json["command"], "info");
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["type"], "info");
        assert_eq!(
            json["data"]["protocol_schema"],
            titan_protocol::SCHEMA_VERSION
        );
    }
}
