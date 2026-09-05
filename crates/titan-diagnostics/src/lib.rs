//! Transport-neutral diagnostic data, bounded request histories, and image checks.
//! Native hosts may persist bundles; browser hosts can serialize the same data.
#[cfg(not(target_arch = "wasm32"))]
mod inspector;
#[cfg(not(target_arch = "wasm32"))]
pub use inspector::{DiagnosticInspector, DiagnosticResult};
mod capture;
pub use capture::{png_capture, write_png};
mod compare;
mod history;
#[cfg(not(target_arch = "wasm32"))]
mod report;
#[cfg(not(target_arch = "wasm32"))]
mod writer;
pub use compare::{ComparisonError, ComparisonOptions, ImageComparison, compare_images};
pub use history::{HistoryEntry, HistorySnapshot, InputEvent, RequestHistory};
#[cfg(not(target_arch = "wasm32"))]
pub use report::{
    ComparisonReportArtifacts, ComparisonReportError, DifferenceVisualization,
    ImageComparisonReport, WrittenComparisonReport, write_comparison_report,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use titan_protocol::{
    CommandMetadata, FieldMetadata, RequestEnvelope, ResponseEnvelope, ResponseOutcome,
};
#[cfg(not(target_arch = "wasm32"))]
pub use writer::{BundleWriteError, WrittenBundle, attach_failure_path, write_bundle};
pub const BUNDLE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticPolicy {
    #[default]
    OnFailure,
    Always,
    Never,
}
impl DiagnosticPolicy {
    pub fn should_capture(self, response: &ResponseEnvelope) -> bool {
        match self {
            Self::OnFailure => matches!(response.outcome, ResponseOutcome::Failure { .. }),
            Self::Always => true,
            Self::Never => false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticLog {
    pub level: String,
    pub message: String,
    pub frame: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleCapture {
    pub artifact: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub checksum: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticBundle {
    pub bundle_version: u32,
    pub request: Option<RequestEnvelope>,
    pub response: Option<ResponseEnvelope>,
    pub local_error: Option<Value>,
    pub context: BTreeMap<String, Value>,
    pub world_state: Value,
    pub history: HistorySnapshot,
    pub logs: Vec<DiagnosticLog>,
    pub timings_us: BTreeMap<String, u64>,
    pub capture: Option<BundleCapture>,
    pub api_summary: Option<ApiSummary>,
}
impl DiagnosticBundle {
    pub fn new(request: RequestEnvelope, response: ResponseEnvelope) -> Self {
        Self {
            bundle_version: BUNDLE_VERSION,
            request: Some(request),
            response: Some(response),
            local_error: None,
            context: BTreeMap::new(),
            world_state: Value::Null,
            history: HistorySnapshot::default(),
            logs: vec![],
            timings_us: BTreeMap::new(),
            capture: None,
            api_summary: None,
        }
    }
    pub fn local_failure(error: Value) -> Self {
        Self {
            bundle_version: BUNDLE_VERSION,
            request: None,
            response: None,
            local_error: Some(error),
            context: BTreeMap::new(),
            world_state: Value::Null,
            history: HistorySnapshot::default(),
            logs: vec![],
            timings_us: BTreeMap::new(),
            capture: None,
            api_summary: None,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiComponent {
    pub name: String,
    pub fields: BTreeMap<String, FieldMetadata>,
}
impl ApiComponent {
    pub fn from_metadata(metadata: &titan::ComponentMetadata) -> Self {
        Self {
            name: metadata.type_name.into(),
            fields: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiSummary {
    pub components: Vec<ApiComponent>,
    pub commands: Vec<CommandMetadata>,
}
impl ApiSummary {
    pub fn new(mut components: Vec<ApiComponent>, mut commands: Vec<CommandMetadata>) -> Self {
        components.sort_by(|a, b| a.name.cmp(&b.name));
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            components,
            commands,
        }
    }
    /// Stable compact data summary. Metadata is descriptive data, not instructions.
    pub fn compact_text(&self) -> String {
        let sorted = Self::new(self.components.clone(), self.commands.clone());
        let mut lines = vec![format!(
            "Titan protocol schema {}",
            titan_protocol::SCHEMA_VERSION
        )];
        for component in &sorted.components {
            lines.push(format!("component {}", one_line(&component.name)));
            for (name, field) in &component.fields {
                lines.push(format!(
                    "  {}: {} [{}]{}",
                    one_line(name),
                    one_line(&field.type_name),
                    if field.writable {
                        "writable"
                    } else {
                        "read-only"
                    },
                    field_notes(field)
                ));
            }
        }
        for command in &sorted.commands {
            lines.push(format!(
                "command {}{}",
                one_line(&command.name),
                if command.description.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", one_line(&command.description))
                }
            ));
            for (name, field) in &command.arguments {
                lines.push(format!(
                    "  {}: {}{}",
                    one_line(name),
                    one_line(&field.type_name),
                    field_notes(field)
                ));
            }
        }
        lines.join("\n") + "\n"
    }
}
fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn field_notes(field: &FieldMetadata) -> String {
    let mut notes = Vec::new();
    if let Some(min) = field.minimum {
        notes.push(format!("min={min}"));
    }
    if let Some(max) = field.maximum {
        notes.push(format!("max={max}"));
    }
    if let Some(unit) = &field.unit {
        notes.push(format!("unit={}", one_line(unit)));
    }
    if !field.description.is_empty() {
        notes.push(one_line(&field.description));
    }
    if notes.is_empty() {
        String::new()
    } else {
        format!(" ({})", notes.join(", "))
    }
}
