//! Synchronous browser inspection policy; DOM and message-origin checks stay in the host.

use super::Inspector;
use crate::App;
use titan_protocol::{
    ErrorCode, Operation, ProtocolError, Request, RequestEnvelope, Response, ResponseEnvelope,
    ResponseOutcome, RunMode,
};

/// An isolated application and inspector with explicit browser control opt-in.
///
/// Calls require exclusive access, so a request runs at a simulation safe point.
/// This boundary parses JSON, preserves response correlation and restricts controls;
/// games still construct the app, register commands/fields and supply capture hooks.
/// The JavaScript host must enforce its own same-origin message boundary.
pub struct BrowserSession {
    app: App,
    inspector: Inspector,
    enable_control: bool,
}

impl BrowserSession {
    /// Assemble a paused browser instance from a game-owned app and inspector.
    /// Control opt-in permits stepping, commands, input and registered writable fields.
    /// Read-only sessions suppress those operations and command discovery.
    pub fn new(app: App, mut inspector: Inspector, enable_control: bool) -> Self {
        inspector.config.run_mode = RunMode::Browser;
        inspector.config.controlled = true;
        inspector.config.mutation_enabled = enable_control;
        Self {
            app,
            inspector,
            enable_control,
        }
    }

    /// Executes a protocol envelope and returns exactly one JSON response envelope.
    /// Malformed input produces InvalidValue with the request ID when recoverable.
    pub fn handle(&mut self, request_json: &str) -> String {
        let response = match serde_json::from_str::<RequestEnvelope>(request_json) {
            Ok(request) => self.execute(&request),
            Err(error) => {
                let request_id = serde_json::from_str::<serde_json::Value>(request_json)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("request_id")
                            .and_then(|id| id.as_str())
                            .map(str::to_owned)
                    })
                    .unwrap_or_default();
                let request = RequestEnvelope::new(request_id, Request::Status);
                self.failure(
                    &request,
                    ProtocolError::new(
                        ErrorCode::InvalidValue,
                        format!("invalid request: {error}"),
                    ),
                )
            }
        };
        serde_json::to_string(&response).expect("protocol responses contain serializable values")
    }
}

impl BrowserSession {
    fn execute(&mut self, request: &RequestEnvelope) -> ResponseEnvelope {
        if !self.enable_control
            && matches!(
                request.request,
                Request::Step { .. }
                    | Request::Invoke { .. }
                    | Request::InjectInput { .. }
                    | Request::SetField { .. }
            )
        {
            return self.failure(
                request,
                ProtocolError::new(
                    ErrorCode::MutationDisabled,
                    "browser controls were not explicitly enabled",
                ),
            );
        }
        let mut response = self.inspector.handle(&mut self.app, request);
        if !self.enable_control {
            match &mut response.outcome {
                ResponseOutcome::Success {
                    response: Response::Capabilities(capabilities),
                } => {
                    capabilities.operations.retain(|operation| {
                        matches!(operation, Operation::Inspect | Operation::Capture)
                    });
                }
                ResponseOutcome::Success {
                    response: Response::Commands { commands },
                } => commands.clear(),
                _ => {}
            }
        }
        response
    }

    fn failure(&mut self, request: &RequestEnvelope, error: ProtocolError) -> ResponseEnvelope {
        // Preserve schema/target validation and exact frame/revision correlation.
        let probe = RequestEnvelope {
            request: Request::Status,
            ..request.clone()
        };
        let mut response = self.inspector.handle(&mut self.app, &probe);
        if matches!(response.outcome, ResponseOutcome::Success { .. }) {
            response.outcome = ResponseOutcome::Failure { error };
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::EntityId;
    fn call(runtime: &mut BrowserSession, request: Request) -> ResponseEnvelope {
        serde_json::from_str(
            &runtime
                .handle(&serde_json::to_string(&RequestEnvelope::new("test", request)).unwrap()),
        )
        .unwrap()
    }

    fn success(response: ResponseEnvelope) -> Response {
        match response.outcome {
            ResponseOutcome::Success { response } => response,
            other => panic!("expected success: {other:?}"),
        }
    }

    #[test]
    fn read_only_policy_covers_every_mutation_and_preserves_validation() {
        let mut runtime = BrowserSession::new(
            App::new(),
            Inspector::new(super::super::InspectionConfig::controlled("test", "test")),
            false,
        );
        let Response::Capabilities(capabilities) =
            success(call(&mut runtime, Request::Capabilities))
        else {
            panic!("capabilities")
        };
        assert_eq!(capabilities.operations, [Operation::Inspect]);
        assert_eq!(capabilities.run_mode, RunMode::Browser);
        assert!(!capabilities.mutation_enabled);
        assert_eq!(
            success(call(&mut runtime, Request::Commands)),
            Response::Commands { commands: vec![] }
        );
        for request in [
            Request::Step { frames: 1 },
            Request::InjectInput {
                frame: 1,
                actions: Default::default(),
            },
            Request::Invoke {
                name: "spawn_shard".into(),
                arguments: Default::default(),
            },
            Request::SetField {
                entity: EntityId {
                    index: 0,
                    generation: 0,
                },
                component: "Position".into(),
                field: "x".into(),
                value: 0.into(),
            },
        ] {
            let response = call(&mut runtime, request);
            assert_eq!((response.observed_frame, response.state_revision), (0, 0));
            assert!(
                matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
            );
        }
        let mut request = RequestEnvelope::new("mismatch", Request::Step { frames: 1 });
        request.schema_version = 999;
        let response: ResponseEnvelope =
            serde_json::from_str(&runtime.handle(&serde_json::to_string(&request).unwrap()))
                .unwrap();
        assert!(
            matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::ProtocolMismatch)
        );
        request.schema_version = titan_protocol::SCHEMA_VERSION;
        request.target_instance = Some("wrong-instance".into());
        let response: ResponseEnvelope =
            serde_json::from_str(&runtime.handle(&serde_json::to_string(&request).unwrap()))
                .unwrap();
        assert!(
            matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::NotFound)
        );
        for malformed in [
            "not json",
            r#"{"request_id":"invalid","request":{"type":"nope"}}"#,
        ] {
            let response: ResponseEnvelope =
                serde_json::from_str(&runtime.handle(malformed)).unwrap();
            assert_eq!((response.observed_frame, response.state_revision), (0, 0));
            assert!(
                matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
            );
        }
    }

    #[test]
    fn enabled_controls_and_malformed_ids_track_the_current_safe_point() {
        let mut runtime = BrowserSession::new(
            App::new(),
            Inspector::new(super::super::InspectionConfig::controlled("test", "test")),
            true,
        );
        let stepped = call(&mut runtime, Request::Step { frames: 2 });
        assert!(matches!(stepped.outcome, ResponseOutcome::Success { .. }));
        let malformed: ResponseEnvelope = serde_json::from_str(
            &runtime.handle(r#"{"request_id":"recover-me","request":{"type":"nope"}}"#),
        )
        .unwrap();
        assert_eq!(malformed.request_id, "recover-me");
        assert_eq!(malformed.observed_frame, stepped.observed_frame);
        assert_eq!(malformed.state_revision, stepped.state_revision);
        assert!(
            matches!(malformed.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
        );
    }
}
