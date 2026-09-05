//! Browser adapter for the same procedural RPG used by native acceptance tests.
//!
//! JavaScript owns the same-origin message boundary. Each synchronous `handle`
//! call owns the application exclusively; no simulation tick runs between calls.

use titan::{
    App, Startup,
    inspection::{BrowserSession, InspectionConfig},
};
use titan_protocol::{CaptureResult, ProtocolError};
use wasm_bindgen::prelude::*;

#[path = "../../../examples/support/procedural_rpg.rs"]
pub mod game;

/// An isolated, paused game instance. Controls require an explicit `true`.
#[wasm_bindgen]
pub struct BrowserRuntime {
    session: BrowserSession,
}

#[wasm_bindgen]
impl BrowserRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new(enable_control: bool) -> Self {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let config = InspectionConfig::controlled("procedural-rpg-browser", "procedural-rpg");
        let inspector = game::inspector_with_capture(config, capture);
        Self {
            session: BrowserSession::new(app, inspector, enable_control),
        }
    }

    /// Executes one request at a safe point and returns its JSON response envelope.
    pub fn handle(&mut self, request_json: &str) -> String {
        self.session.handle(request_json)
    }
}

fn capture(app: &App) -> Result<CaptureResult, ProtocolError> {
    let image = game::render_image(app.world())?;
    titan_diagnostics::png_capture(&image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use titan_protocol::ErrorCode;
    use titan_protocol::{EntityId, InputValue};
    use titan_protocol::{
        Operation, Request, RequestEnvelope, Response, ResponseEnvelope, ResponseOutcome, RunMode,
    };

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
        assert!(capabilities.mutation_enabled);
        assert!(capabilities.operations.contains(&Operation::Mutate));
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
        assert_eq!(capture.checksum, "f7a298f62ad75c1c");
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
        assert_eq!(game::image_checksum(&decoded), 0xf7a2_98f6_2ad7_5c1c);
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

#[cfg(target_arch = "wasm32")]
mod player;
#[cfg(target_arch = "wasm32")]
pub use player::BrowserPlayer;
