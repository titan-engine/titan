//! Browser adapter for the same procedural RPG used by native acceptance tests.
//!
//! JavaScript owns the same-origin message boundary. Each synchronous `handle`
//! call owns the application exclusively; no simulation tick runs between calls.

use base64::{Engine, engine::general_purpose::STANDARD};
use titan::{
    App, Startup,
    inspection::{InspectionConfig, Inspector},
};
use titan_protocol::{
    CaptureResult, ErrorCode, Operation, ProtocolError, Request, RequestEnvelope, Response,
    ResponseEnvelope, ResponseOutcome, RunMode,
};
use wasm_bindgen::prelude::*;

#[path = "../../../examples/support/procedural_rpg.rs"]
pub mod game;

/// An isolated, paused game instance. Controls require an explicit `true`.
#[wasm_bindgen]
pub struct BrowserRuntime {
    app: App,
    inspector: Inspector,
    enable_control: bool,
}

#[wasm_bindgen]
impl BrowserRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new(enable_control: bool) -> Self {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let mut config = InspectionConfig::controlled("procedural-rpg-browser", "procedural-rpg");
        config.run_mode = RunMode::Browser;
        // Field mutation remains unavailable; control opt-in enables registered
        // game operations separately from reflected field writes.
        config.mutation_enabled = false;
        let inspector = game::inspector_with_capture(config, capture);
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

impl BrowserRuntime {
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

fn capture(app: &App) -> Result<CaptureResult, ProtocolError> {
    let image = game::render_image(app.world())?;
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, image.width(), image.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(capture_error)?;
        writer
            .write_image_data(image.pixels())
            .map_err(capture_error)?;
        writer.finish().map_err(capture_error)?;
    }
    Ok(CaptureResult {
        width: image.width(),
        height: image.height(),
        format: "png".into(),
        artifact: format!("data:image/png;base64,{}", STANDARD.encode(bytes)),
        checksum: format!("{:016x}", game::image_checksum(&image)),
    })
}

fn capture_error(error: png::EncodingError) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, format!("PNG capture failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::{EntityId, InputValue};

    fn call(runtime: &mut BrowserRuntime, request: Request) -> ResponseEnvelope {
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
        let mut runtime = BrowserRuntime::new(false);
        let Response::Capabilities(capabilities) =
            success(call(&mut runtime, Request::Capabilities))
        else {
            panic!("capabilities")
        };
        assert_eq!(
            capabilities.operations,
            [Operation::Inspect, Operation::Capture]
        );
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
    fn browser_capture_decodes_to_exact_reference_pixels() {
        let mut runtime = BrowserRuntime::new(true);
        let Response::Capabilities(capabilities) =
            success(call(&mut runtime, Request::Capabilities))
        else {
            panic!("capabilities");
        };
        assert!(!capabilities.mutation_enabled);
        assert!(!capabilities.operations.contains(&Operation::Mutate));
        for operation in [Operation::Step, Operation::Invoke, Operation::InjectInput] {
            assert!(capabilities.operations.contains(&operation));
        }

        let mut frame = 0;
        for (action, ticks) in [("right", 2), ("down", 3), ("right", 6)] {
            for _ in 0..ticks {
                frame += 1;
                success(call(
                    &mut runtime,
                    Request::InjectInput {
                        frame,
                        actions: [(action.into(), InputValue::Button(true))].into(),
                    },
                ));
            }
        }
        let stepped = call(&mut runtime, Request::Step { frames: 11 });
        assert_eq!((stepped.observed_frame, stepped.state_revision), (11, 12));
        let Response::Capture(capture) = success(call(&mut runtime, Request::Capture)) else {
            panic!("capture")
        };
        assert_eq!(capture.checksum, "98618cd721c5b52d");
        let bytes = STANDARD
            .decode(
                capture
                    .artifact
                    .strip_prefix("data:image/png;base64,")
                    .unwrap(),
            )
            .unwrap();
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
        let info = decoder.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (160, 112));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let decoded = titan::render::Image::new(info.width, info.height, pixels).unwrap();
        assert_eq!(game::image_checksum(&decoded), 0x9861_8cd7_21c5_b52d);
        success(call(
            &mut runtime,
            Request::Invoke {
                name: "spawn_shard".into(),
                arguments: [("x".into(), 0.into()), ("y".into(), 0.into())].into(),
            },
        ));
        let Response::Capture(changed) = success(call(&mut runtime, Request::Capture)) else {
            panic!("capture")
        };
        assert_ne!(changed.checksum, capture.checksum);
    }
}
