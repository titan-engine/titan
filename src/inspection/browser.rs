//! Browser inspection policy; DOM and message-origin checks stay in the host.

use super::{Dispatch, Inspector};
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

    pub fn capture_timeout(&self) -> std::time::Duration {
        self.inspector.capture_timeout()
    }

    /// Accept at a safe point and release the session before awaiting completion.
    pub fn dispatch_json(&mut self, request_json: &str) -> Dispatch {
        dispatch_json_with_policy(
            &mut self.app,
            &mut self.inspector,
            self.enable_control,
            request_json,
        )
    }

    /// Executes a protocol envelope and returns exactly one JSON response envelope.
    pub fn handle(&mut self, request_json: &str) -> String {
        handle_json_with_policy(
            &mut self.app,
            &mut self.inspector,
            self.enable_control,
            request_json,
        )
    }
}

/// Parse a browser/host request against the caller's existing application.
/// This borrows the actual player; it neither constructs a replacement game nor
/// changes its clock policy. The host must enforce its message origin boundary.
pub fn handle_json_with_policy(
    app: &mut App,
    inspector: &mut Inspector,
    enable_control: bool,
    request_json: &str,
) -> String {
    let response =
        dispatch_json_with_policy(app, inspector, enable_control, request_json).into_ready();
    serde_json::to_string(&response).expect("serializable protocol response")
}

/// Parse and accept a request without retaining any application borrow.
pub fn dispatch_json_with_policy(
    app: &mut App,
    inspector: &mut Inspector,
    enable_control: bool,
    request_json: &str,
) -> Dispatch {
    match serde_json::from_str::<RequestEnvelope>(request_json) {
        Ok(request) => dispatch_with_policy(app, inspector, enable_control, &request),
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
            Dispatch::Ready(failure(
                app,
                inspector,
                &RequestEnvelope::new(request_id, Request::Status),
                ProtocolError::new(ErrorCode::InvalidValue, format!("invalid request: {error}")),
            ))
        }
    }
}

/// Enforce read-only/control opt-in at an exclusive caller-owned safe point.
/// Queries remain available without controls; mutation, commands and input do not.
pub fn handle_with_policy(
    app: &mut App,
    inspector: &mut Inspector,
    enable_control: bool,
    request: &RequestEnvelope,
) -> ResponseEnvelope {
    dispatch_with_policy(app, inspector, enable_control, request).into_ready()
}

/// Apply browser permissions before snapshot acceptance; queries remain synchronous.
pub fn dispatch_with_policy(
    app: &mut App,
    inspector: &mut Inspector,
    enable_control: bool,
    request: &RequestEnvelope,
) -> Dispatch {
    if !enable_control
        && matches!(
            request.request,
            Request::Step { .. }
                | Request::Invoke { .. }
                | Request::InjectInput { .. }
                | Request::SetField { .. }
        )
    {
        return Dispatch::Ready(failure(
            app,
            inspector,
            request,
            ProtocolError::new(
                ErrorCode::MutationDisabled,
                "controls were not explicitly enabled",
            ),
        ));
    }
    let mut response = match inspector.dispatch(app, request) {
        Dispatch::Ready(response) => response,
        pending @ Dispatch::Pending(_) => return pending,
    };
    if !enable_control {
        match &mut response.outcome {
            ResponseOutcome::Success {
                response: Response::Capabilities(capabilities),
            } => {
                capabilities.operations.retain(|operation| {
                    matches!(
                        operation,
                        Operation::Inspect | Operation::Query | Operation::Capture
                    )
                });
                capabilities.mutation_enabled = false;
            }
            ResponseOutcome::Success {
                response: Response::Commands { commands },
            } => commands.clear(),
            _ => {}
        }
    }
    Dispatch::Ready(response)
}

fn failure(
    app: &mut App,
    inspector: &mut Inspector,
    request: &RequestEnvelope,
    error: ProtocolError,
) -> ResponseEnvelope {
    let probe = RequestEnvelope {
        request: Request::Status,
        ..request.clone()
    };
    let mut response = inspector.handle(app, &probe);
    if matches!(response.outcome, ResponseOutcome::Success { .. }) {
        response.outcome = ResponseOutcome::Failure { error };
    }
    response
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
    fn live_queries_borrow_current_game_and_preserve_read_only_revision() {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Args {
            add: u32,
        }
        let mut app = App::new();
        app.world_mut().insert_resource(7_u32);
        let mut inspector =
            Inspector::new(super::super::InspectionConfig::controlled("live", "test"));
        let metadata = titan_protocol::QueryMetadata {
            name: "score".into(),
            description: "Current score".into(),
            arguments: Default::default(),
        };
        inspector
            .register_query(metadata.clone(), |app, args: Args| {
                Ok(serde_json::json!(
                    *app.world().resource::<u32>().unwrap() + args.add
                ))
            })
            .unwrap();
        assert!(
            inspector
                .register_query(metadata, |_, _: Args| Ok(0.into()))
                .is_err()
        );
        inspector.set_run_mode(RunMode::Interactive);
        inspector.set_controlled(false);
        inspector.set_mutation_enabled(true);
        app.advance_fixed(2);
        *app.world_mut().resource_mut::<u32>().unwrap() = 10;
        inspector.note_external_change();
        let call = |app: &mut App, inspector: &mut Inspector, request| {
            handle_with_policy(
                app,
                inspector,
                false,
                &RequestEnvelope::new("read", request),
            )
        };
        let status = call(&mut app, &mut inspector, Request::Status);
        assert_eq!((status.observed_frame, status.state_revision), (2, 1));
        assert!(
            matches!(success(status), Response::Status(state) if !state.paused && state.run_mode == RunMode::Interactive)
        );
        assert!(
            matches!(success(call(&mut app, &mut inspector, Request::Queries)), Response::Queries { queries } if queries.len() == 1)
        );
        let query = call(
            &mut app,
            &mut inspector,
            Request::Query {
                name: "score".into(),
                arguments: [("add".into(), 3.into())].into(),
            },
        );
        assert_eq!((query.observed_frame, query.state_revision), (2, 1));
        assert_eq!(success(query), Response::QueryResult { value: 13.into() });
        let invalid = call(
            &mut app,
            &mut inspector,
            Request::Query {
                name: "score".into(),
                arguments: [("extra".into(), true.into())].into(),
            },
        );
        assert!(
            matches!(invalid.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
        );
        assert_eq!((invalid.observed_frame, invalid.state_revision), (2, 1));
        let capabilities = success(call(&mut app, &mut inspector, Request::Capabilities));
        assert!(
            matches!(capabilities, Response::Capabilities(c) if c.operations == [Operation::Inspect, Operation::Query] && !c.mutation_enabled)
        );
        inspector.set_controlled(true);
        inspector.note_external_change();
        let status = call(&mut app, &mut inspector, Request::Status);
        assert_eq!((status.observed_frame, status.state_revision), (2, 2));
        assert!(matches!(success(status), Response::Status(state) if state.paused));
        let denied = call(&mut app, &mut inspector, Request::Step { frames: 1 });
        assert_eq!((denied.observed_frame, denied.state_revision), (2, 2));
        assert!(
            matches!(denied.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
        );
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

/// Convert an owned dispatch into a Promise after the caller's mutable borrow ends.
/// Timer tasks drive completion even when presentation and simulation are paused.
#[cfg(all(target_arch = "wasm32", feature = "browser-capture"))]
pub fn response_promise(
    timeout: std::time::Duration,
    accept: impl FnOnce() -> Dispatch,
) -> js_sys::Promise {
    use wasm_bindgen::{JsCast, JsValue};
    let clock = || -> Result<(js_sys::Function, JsValue, f64), JsValue> {
        let performance = js_sys::Reflect::get(&js_sys::global(), &"performance".into())?;
        let now =
            js_sys::Reflect::get(&performance, &"now".into())?.dyn_into::<js_sys::Function>()?;
        let started = now
            .call0(&performance)?
            .as_f64()
            .ok_or_else(|| JsValue::from_str("invalid monotonic browser clock"))?;
        Ok((now, performance, started))
    }();
    let dispatch = accept();
    wasm_bindgen_futures::future_to_promise(async move {
        let elapsed = || -> Result<std::time::Duration, JsValue> {
            let (now, performance, started) = clock.as_ref().map_err(Clone::clone)?;
            let current = now
                .call0(performance)?
                .as_f64()
                .ok_or_else(|| JsValue::from_str("invalid monotonic browser clock"))?;
            let seconds = (current - started) / 1000.0;
            Ok(if seconds.is_finite() && seconds >= 0.0 {
                std::time::Duration::from_secs_f64(seconds)
            } else {
                std::time::Duration::MAX
            })
        };
        let mut response = match dispatch {
            Dispatch::Ready(response) => response,
            Dispatch::Pending(mut pending) => {
                let global = js_sys::global();
                let timer = js_sys::Reflect::get(&global, &"setTimeout".into())?
                    .dyn_into::<js_sys::Function>()?;
                loop {
                    if let Some(response) = pending.poll(elapsed()?) {
                        break response;
                    }
                    let wait = js_sys::Promise::new(&mut |resolve, reject| {
                        if let Err(error) = timer.call2(&global, &resolve, &4.into()) {
                            let _ = reject.call1(&JsValue::UNDEFINED, &error);
                        }
                    });
                    wasm_bindgen_futures::JsFuture::from(wait).await?;
                }
            }
        };
        let capture = matches!(
            &response.outcome,
            ResponseOutcome::Success {
                response: Response::Capture(_)
            }
        );
        let timeout_response = |response: &mut ResponseEnvelope| {
            response.outcome = ResponseOutcome::Failure {
                error: ProtocolError::new(
                    ErrorCode::Timeout,
                    "capture deadline exceeded during response preparation",
                ),
            };
        };
        if capture && elapsed()? >= timeout {
            timeout_response(&mut response);
        }
        let mut json = serde_json::to_string(&response).expect("serializable protocol response");
        // Serialization is part of the budget; never publish an oversized-time success.
        if capture && elapsed()? >= timeout {
            timeout_response(&mut response);
            json = serde_json::to_string(&response).expect("serializable timeout response");
        }
        Ok(JsValue::from_str(&json))
    })
}
