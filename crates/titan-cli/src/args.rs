use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "titan", version, about = "Agent-friendly Titan game workflows")]
pub(crate) struct Cli {
    /// Selects human-readable or stable machine-readable output.
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Human)]
    pub(crate) format: OutputFormat,
    /// Project directory whose local runtime registry should be used.
    #[arg(long, global = true, default_value = ".")]
    pub(crate) project: PathBuf,
    /// Runtime instance ID; required when multiple runtimes match.
    #[arg(long, global = true)]
    pub(crate) instance: Option<String>,
    /// Maximum duration of an inspection request in milliseconds.
    #[arg(long, global = true, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) timeout_ms: u64,
    /// Diagnostic capture policy.
    #[arg(long, global = true, value_enum, default_value_t = CapturePolicy::OnFailure)]
    pub(crate) diagnostics: CapturePolicy,
    /// Maximum frames accepted by a single step request.
    #[arg(long, global = true, default_value_t = 10000, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) max_frames: u64,
    /// Wall-clock limit for Cargo workflows, including compilation.
    #[arg(long, global = true, default_value_t = 120000, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) process_timeout_ms: u64,
    #[command(subcommand)]
    pub(crate) command: CliCommand,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum CapturePolicy {
    OnFailure,
    Always,
    Never,
}

#[derive(Subcommand)]
pub(crate) enum CliCommand {
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

#[cfg(test)]
mod tests {

    use crate::args::Cli;
    use clap::Parser;

    #[test]
    fn output_format_is_global() {
        let before = Cli::try_parse_from(["titan", "--format", "json", "info"]);
        let after = Cli::try_parse_from(["titan", "info", "--format", "json"]);

        assert!(before.is_ok());
        assert!(after.is_ok());
    }
}
