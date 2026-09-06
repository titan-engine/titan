use crate::args::OutputFormat;
use serde::Serialize;
use std::process::ExitCode;
use titan_diagnostics::{ComparisonOptions, ImageComparison};

#[derive(Serialize)]
pub(crate) struct CommandResult {
    pub(crate) command: &'static str,
    pub(crate) success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) exit_code: Option<i32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) stdout: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<CommandData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostic_bundle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostic_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<&'static str>,
    pub(crate) elapsed_ms: u64,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum CommandData {
    Info {
        cli_version: &'static str,
        protocol_schema: u32,
    },
    Process {
        program: String,
        arguments: Vec<String>,
    },
    ImageComparison {
        expected: String,
        actual: String,
        width: u32,
        height: u32,
        options: ComparisonOptions,
        comparison: ImageComparison,
        artifacts: ComparisonArtifacts,
    },
}

#[derive(Serialize)]
pub(crate) struct ComparisonArtifacts {
    pub(crate) directory: String,
    pub(crate) manifest: String,
    pub(crate) expected: String,
    pub(crate) actual: String,
    pub(crate) difference: String,
}

pub(crate) fn command_exit_code(result: &CommandResult) -> ExitCode {
    if result.success {
        ExitCode::SUCCESS
    } else if result.error_code == Some("visual_mismatch") {
        ExitCode::from(2)
    } else {
        ExitCode::FAILURE
    }
}

pub(crate) fn local_failure(code: &str, message: String) -> serde_json::Value {
    serde_json::json!({"status": "failure", "error": {"code": code, "message": message, "details": {}, "retryable": false}})
}

pub(crate) fn render(format: OutputFormat, result: &CommandResult) {
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
        println!("Report: {}", artifacts.manifest);
        println!("Difference image: {}", artifacts.difference);
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
    use super::CommandData;

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
