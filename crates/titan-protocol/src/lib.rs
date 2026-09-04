//! Transport-neutral messages shared by Titan runtimes, tools, and browsers.
//!
//! Titan does not currently promise compatibility between engine versions.
//! Peers therefore require an exact schema version match and can report a
//! structured mismatch rather than attempting an ambiguous fallback.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;

pub type RequestId = String;
pub type InstanceId = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub schema_version: u32,
    pub request_id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_instance: Option<InstanceId>,
    pub request: Request,
}

impl RequestEnvelope {
    pub fn new(request_id: impl Into<RequestId>, request: Request) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id: request_id.into(),
            target_instance: None,
            request,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Capabilities,
    Status,
    Entities {
        #[serde(default)]
        query: EntityQuery,
        #[serde(default)]
        page: PageRequest,
    },
    Entity {
        entity: EntityId,
    },
    SetField {
        entity: EntityId,
        component: String,
        field: String,
        value: Value,
    },
    Commands,
    Invoke {
        name: String,
        #[serde(default)]
        arguments: BTreeMap<String, Value>,
    },
    Step {
        frames: u64,
    },
    InjectInput {
        frame: u64,
        actions: BTreeMap<String, InputValue>,
    },
    Capture,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub with_components: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: u32,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId {
    pub index: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum InputValue {
    Button(bool),
    Axis(i16),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub schema_version: u32,
    pub request_id: RequestId,
    pub instance_id: InstanceId,
    pub observed_frame: u64,
    pub state_revision: u64,
    #[serde(flatten)]
    pub outcome: ResponseOutcome,
}

impl ResponseEnvelope {
    pub fn success(
        request: &RequestEnvelope,
        instance_id: impl Into<InstanceId>,
        observed_frame: u64,
        state_revision: u64,
        response: Response,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id: request.request_id.clone(),
            instance_id: instance_id.into(),
            observed_frame,
            state_revision,
            outcome: ResponseOutcome::Success { response },
        }
    }

    pub fn failure(
        request: &RequestEnvelope,
        instance_id: impl Into<InstanceId>,
        observed_frame: u64,
        state_revision: u64,
        error: ProtocolError,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id: request.request_id.clone(),
            instance_id: instance_id.into(),
            observed_frame,
            state_revision,
            outcome: ResponseOutcome::Failure { error },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseOutcome {
    Success { response: Response },
    Failure { error: ProtocolError },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Capabilities(Capabilities),
    Status(RuntimeStatus),
    Entities(EntityPage),
    Entity(EntityDetails),
    Commands { commands: Vec<CommandMetadata> },
    Applied { applied_frame: u64 },
    Stepped { frames: u64, current_frame: u64 },
    Capture(CaptureResult),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub schema_version: u32,
    pub run_mode: RunMode,
    pub mutation_enabled: bool,
    pub controlled: bool,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    Interactive,
    Headless,
    Browser,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Inspect,
    Mutate,
    Invoke,
    Step,
    InjectInput,
    Capture,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub project: String,
    pub run_mode: RunMode,
    pub current_frame: u64,
    pub paused: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityPage {
    pub entities: Vec<EntitySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySummary {
    pub id: EntityId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub components: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityDetails {
    pub id: EntityId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub components: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arguments: BTreeMap<String, FieldMetadata>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldMetadata {
    pub type_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub writable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureResult {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub artifact: String,
    pub checksum: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, Value>,
    pub retryable: bool,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
            retryable: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MutationDisabled,
    ReadOnly,
    InvalidValue,
    RequiresCommand,
    NotControlled,
    NotFound,
    AmbiguousTarget,
    ProtocolMismatch,
    Timeout,
    Busy,
    Internal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_shape_is_tagged_and_stable() {
        let request = RequestEnvelope::new(
            "req-1",
            Request::SetField {
                entity: EntityId {
                    index: 7,
                    generation: 2,
                },
                component: "game::Health".to_owned(),
                field: "current".to_owned(),
                value: Value::from(80),
            },
        );

        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["schema_version"], SCHEMA_VERSION);
        assert_eq!(json["request"]["type"], "set_field");
        assert_eq!(json["request"]["entity"]["generation"], 2);
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(json).unwrap(),
            request
        );
    }

    #[test]
    fn failures_retain_frame_and_request_correlation() {
        let request = RequestEnvelope::new("req-2", Request::Step { frames: 1 });
        let response = ResponseEnvelope::failure(
            &request,
            "game-1",
            42,
            9,
            ProtocolError::new(ErrorCode::NotControlled, "interactive clock owns stepping"),
        );

        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["request_id"], "req-2");
        assert_eq!(json["observed_frame"], 42);
        assert_eq!(json["state_revision"], 9);
        assert_eq!(json["status"], "failure");
        assert_eq!(json["error"]["code"], "not_controlled");
    }
}
