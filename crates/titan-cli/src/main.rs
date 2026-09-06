use clap::Parser;
use std::env;
use std::process::ExitCode;
use std::time::Duration;

mod args;
mod comparison;
mod diagnostics;
mod dispatch;
mod local;
mod output;
mod process;
mod remote;

use args::{Cli, OutputFormat};
use diagnostics::capture_diagnostic;
use dispatch::{CommandRoute, classify};
use local::execute;
use output::{command_exit_code, local_failure, render};
use remote::execute_remote;

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
    let remote_result = match classify(&cli.command) {
        Ok(CommandRoute::Local(command)) => {
            let mut result = execute(
                command,
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
        Ok(CommandRoute::Remote(request)) => execute_remote(&cli, request),
        Err(error) => Err(error),
    };
    let (mut result, success) = match remote_result {
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
