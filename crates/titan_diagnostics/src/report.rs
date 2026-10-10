use alloc::collections::BTreeMap;
use bevy_ecs::error::ErrorContext;
use serde::{Deserialize, Serialize};

/// Version 1 of the JSON report schema. Optional context is explicitly null.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticReport {
    /// Schema version; currently 1.
    pub schema_version: u32,
    /// Unique report ID, also used in the output filename.
    pub id: String,
    /// `error` (fallback handler) or `panic` (panic hook).
    pub kind: String,
    /// Advisory Bevy severity, or `Panic` for a panic hook report.
    pub severity: Option<String>,
    /// Configured application name.
    pub app_name: String,
    /// Configured application version.
    pub app_version: Option<String>,
    /// System, command, observer, or run-condition context, when known.
    pub context: Option<FailureContext>,
    /// Innermost active Bevy schedule's debug label, if tracing is installed.
    pub schedule: Option<String>,
    /// `FrameCount` sampled at the start of the app update.
    pub frame: Option<u32>,
    /// First occurrence's frame snapshot.
    pub first_frame: Option<u32>,
    /// Error display text (including Bevy's origin backtrace when enabled).
    pub message: String,
    /// Backtrace captured at the reporting point, when `RUST_BACKTRACE` is enabled.
    pub backtrace: Option<String>,
    /// Panic source location, if provided by Rust.
    pub location: Option<String>,
    /// Number of matching occurrences within this sink's retained report window.
    pub count: u64,
    /// First occurrence's Unix timestamp in milliseconds.
    pub first_seen_unix_ms: u128,
    /// Latest occurrence's Unix timestamp in milliseconds.
    pub last_seen_unix_ms: u128,
    /// Latest occurrence's bounded warning/error history, oldest first.
    pub recent_logs: Vec<RecentLog>,
}

/// Structured information from Bevy's public error context or active system span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureContext {
    /// `system`, `command`, `observer`, or `run_condition`.
    pub kind: String,
    /// Name provided by Bevy; diagnostics enables Bevy's `debug` feature.
    pub name: String,
    /// Last ECS change tick, not a frame number.
    pub last_run: Option<u32>,
    /// Run condition's associated system, if applicable.
    pub system: Option<String>,
    /// Whether the run condition is attached to a system set.
    pub on_set: Option<bool>,
}

impl From<&ErrorContext> for FailureContext {
    fn from(context: &ErrorContext) -> Self {
        let (kind, last_run, system, on_set) = match context {
            ErrorContext::System { last_run, .. } => ("system", Some(last_run.get()), None, None),
            ErrorContext::Command { .. } => ("command", None, None, None),
            ErrorContext::Observer { last_run, .. } => {
                ("observer", Some(last_run.get()), None, None)
            }
            ErrorContext::RunCondition {
                last_run,
                system,
                on_set,
                ..
            } => (
                "run_condition",
                Some(last_run.get()),
                Some(system.to_string()),
                Some(*on_set),
            ),
        };
        Self {
            kind: kind.into(),
            name: context.name().to_string(),
            last_run,
            system,
            on_set,
        }
    }
}

/// A warning or error tracing event. All recorded fields are preserved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentLog {
    /// Unix timestamp in milliseconds.
    pub unix_ms: u128,
    /// `WARN` or `ERROR`.
    pub level: String,
    /// Tracing event target.
    pub target: String,
    /// Named event fields, including `message`, formatted using tracing's visitor API.
    pub fields: BTreeMap<String, String>,
}
