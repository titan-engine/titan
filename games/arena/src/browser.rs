//! Browser protocol adapter for this game.
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

use crate::game;

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
        let mut config = InspectionConfig::controlled("arena-browser", "arena");
        config.run_mode = RunMode::Browser;
        // The explicit control opt-in also permits registered component fields.
        config.mutation_enabled = enable_control;
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

#[cfg(target_arch = "wasm32")]
mod player {
    use titan::{App, Startup};
    use titan_render_wgpu::wgpu;
    use wasm_bindgen::prelude::*;
    use web_sys::HtmlCanvasElement;

    use crate::game;
    use crate::surface::SurfaceRenderer;

    /// Interactive canvas runner. The browser owns keyboard events and animation timing.
    #[wasm_bindgen]
    pub struct BrowserPlayer {
        app: App,
        input: game::InteractiveInput,
        renderer: SurfaceRenderer,
        canvas: HtmlCanvasElement,
        accumulated_ms: f64,
    }

    #[wasm_bindgen]
    impl BrowserPlayer {
        pub async fn create(canvas: HtmlCanvasElement) -> Result<BrowserPlayer, JsValue> {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
                .map_err(js_error)?;
            let mut renderer =
                SurfaceRenderer::new(&instance, surface, canvas.width(), canvas.height())
                    .await
                    .map_err(js_error)?;
            let (width, height) = renderer.resize(canvas.width(), canvas.height());
            canvas.set_width(width);
            canvas.set_height(height);
            let mut app = game::build_game();
            app.update_schedule(Startup);
            Ok(Self {
                app,
                input: game::InteractiveInput::default(),
                renderer,
                canvas,
                accumulated_ms: 0.0,
            })
        }

        pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), JsValue> {
            self.input.set_action(name, pressed).map_err(js_error)
        }

        /// Advance fixed 60 Hz ticks, then render. Long background pauses are capped.
        /// Calling frame(0) renders current state without advancing the game.
        pub fn frame(&mut self, elapsed_ms: f64) -> Result<(), JsValue> {
            if !elapsed_ms.is_finite() || elapsed_ms < 0.0 {
                return Err(JsValue::from_str(
                    "elapsed milliseconds must be finite and nonnegative",
                ));
            }
            self.accumulated_ms += elapsed_ms.min(250.0);
            while self.accumulated_ms >= 1000.0 / 60.0 {
                self.input.tick(&mut self.app);
                self.accumulated_ms -= 1000.0 / 60.0;
            }
            self.renderer.render(&self.app).map_err(js_error)?;
            Ok(())
        }

        pub fn resize(&mut self, width: u32, height: u32) {
            let (width, height) = self.renderer.resize(width, height);
            self.canvas.set_width(width);
            self.canvas.set_height(height);
        }

        pub fn restart(&mut self) {
            super::restart_player(&mut self.app, &mut self.input, &mut self.accumulated_ms);
        }

        pub fn status(&self) -> String {
            game::status(&self.app)
        }
    }

    fn js_error(error: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&error.to_string())
    }
}
#[cfg(target_arch = "wasm32")]
pub use player::BrowserPlayer;

#[cfg(any(target_arch = "wasm32", test))]
fn restart_player(app: &mut App, input: &mut game::InteractiveInput, accumulated_ms: &mut f64) {
    game::restart(app);
    *input = game::InteractiveInput::default();
    *accumulated_ms = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::EntityId;

    #[test]
    fn player_restart_preserves_frame_and_clears_input() {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let mut input = game::InteractiveInput::default();
        input.set_action("right", true).unwrap();
        input.tick(&mut app);
        let mut accumulated_ms = 12.0;
        restart_player(&mut app, &mut input, &mut accumulated_ms);
        let reset: serde_json::Value = serde_json::from_str(&game::status(&app)).unwrap();
        assert_eq!(reset["frame"], 1);
        assert_eq!(accumulated_ms, 0.0);
        input.tick(&mut app);
        let next: serde_json::Value = serde_json::from_str(&game::status(&app)).unwrap();
        assert_eq!(next["frame"], 2);
        assert_eq!(next["position"], reset["position"]);
    }

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
                name: "restart".into(),
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
}
