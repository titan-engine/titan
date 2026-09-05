//! Browser protocol adapter for this game.
//!
//! JavaScript owns the same-origin message boundary. Each synchronous `handle`
//! call owns the application exclusively; no simulation tick runs between calls.

use titan::{
    App, Startup,
    inspection::{BrowserSession, InspectionConfig},
};
use titan_protocol::{CaptureResult, ProtocolError};
use wasm_bindgen::prelude::*;

use crate::game;

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
        let config = InspectionConfig::controlled("arena-browser", "arena");
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

/// Headless adapter for exercising the same live session under actual WASM.
/// GPU presentation is verified separately through BrowserPlayer.
#[wasm_bindgen]
pub struct BrowserLiveRuntime {
    session: crate::live::ArenaSession,
}

#[wasm_bindgen]
impl BrowserLiveRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            session: crate::live::ArenaSession::new(
                "arena-live-browser",
                "arena",
                titan_protocol::RunMode::Browser,
                false,
            ),
        }
    }
    pub fn handle(&mut self, request_json: &str) -> String {
        self.session.handle_json(request_json)
    }
    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), JsValue> {
        self.session
            .set_action(name, pressed)
            .map_err(|error| JsValue::from_str(&error))
    }
    pub fn tick(&mut self) {
        self.session.tick();
    }
    pub fn resume(&mut self) {
        self.session.resume();
    }
    pub fn pause(&mut self) {
        self.session.pause();
    }
    pub fn set_control_enabled(&mut self, enabled: bool) {
        self.session.set_control_enabled(enabled);
    }
}

impl Default for BrowserLiveRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Replay an exported recording in a fresh headless game, with a bounded input.
#[wasm_bindgen]
pub fn verify_recording_json(recording_json: &str) -> Result<String, JsValue> {
    if recording_json.len() > 2 * 1024 * 1024 {
        return Err(JsValue::from_str("recording exceeds the 2 MiB size bound"));
    }
    let result = serde_json::from_str(recording_json)
        .map_err(|error| error.to_string())
        .and_then(crate::live::verify_recording)
        .and_then(|value| serde_json::to_string(&value).map_err(|error| error.to_string()));
    result.map_err(|error| JsValue::from_str(&error))
}

fn capture(app: &App) -> Result<CaptureResult, ProtocolError> {
    let image = game::render_image(app.world())?;
    titan_diagnostics::png_capture(&image)
}

#[cfg(target_arch = "wasm32")]
mod player {
    use titan::render::{ImageAssets, RenderFrame};
    use titan_protocol::RunMode;
    use titan_render_wgpu::{SurfaceRenderer, wgpu};
    use wasm_bindgen::prelude::*;
    use web_sys::HtmlCanvasElement;

    use crate::{game, live::ArenaSession};

    /// Interactive canvas runner. The browser owns keyboard events and animation timing.
    #[wasm_bindgen]
    pub struct BrowserPlayer {
        session: ArenaSession,
        renderer: SurfaceRenderer,
        canvas: HtmlCanvasElement,
        accumulated_ms: f64,
        clock_epoch: u64,
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
            let session = ArenaSession::new("arena-live-browser", "arena", RunMode::Browser, false);
            let clock_epoch = session.clock_epoch();
            Ok(Self {
                session,
                renderer,
                canvas,
                accumulated_ms: 0.0,
                clock_epoch,
            })
        }

        pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), JsValue> {
            self.session.set_action(name, pressed).map_err(js_error)
        }

        /// Cancel held actions and buffered taps on focus loss or pause.
        pub fn clear_input(&mut self) {
            self.session.clear_input();
        }

        /// Cancel one interrupted gesture without dropping other buffered actions.
        pub fn cancel_action(&mut self, name: &str) -> Result<(), JsValue> {
            self.session.cancel_action(name).map_err(js_error)
        }

        /// Advance fixed 60 Hz ticks, then render. Long background pauses are capped.
        /// Calling frame(0) renders current state without advancing the game.
        pub fn frame(&mut self, elapsed_ms: f64) -> Result<(), JsValue> {
            if !elapsed_ms.is_finite() || elapsed_ms < 0.0 {
                return Err(JsValue::from_str(
                    "elapsed milliseconds must be finite and nonnegative",
                ));
            }
            if self.clock_epoch != self.session.clock_epoch() {
                self.accumulated_ms = 0.0;
                self.clock_epoch = self.session.clock_epoch();
            }
            if !self.session.paused() {
                self.accumulated_ms += elapsed_ms.min(250.0);
                while self.accumulated_ms >= 1000.0 / 60.0 {
                    self.session.tick();
                    self.accumulated_ms -= 1000.0 / 60.0;
                }
            }
            let frame = self
                .session
                .app()
                .extracted::<RenderFrame>()
                .ok_or_else(|| js_error("game render extraction unavailable"))?;
            let assets = self
                .session
                .app()
                .world()
                .resource::<ImageAssets>()
                .ok_or_else(|| js_error("game image assets unavailable"))?;
            self.renderer.render(frame, assets).map_err(js_error)?;
            Ok(())
        }

        pub fn resize(&mut self, width: u32, height: u32) {
            let (width, height) = self.renderer.resize(width, height);
            self.canvas.set_width(width);
            self.canvas.set_height(height);
        }

        pub fn restart(&mut self) {
            self.session.pause();
            self.session.restart();
            self.accumulated_ms = 0.0;
        }

        pub fn status(&self) -> String {
            game::status(self.session.app())
        }

        pub fn pause(&mut self) {
            self.session.pause();
        }
        pub fn resume(&mut self) {
            self.session.resume();
        }
        pub fn paused(&self) -> bool {
            self.session.paused()
        }
        pub fn clock_epoch(&self) -> String {
            self.session.clock_epoch().to_string()
        }
        pub fn control_enabled(&self) -> bool {
            self.session.control_enabled()
        }
        pub fn set_control_enabled(&mut self, enabled: bool) {
            self.session.set_control_enabled(enabled);
        }
        /// Inspect and control the exact session presented on this canvas.
        pub fn handle(&mut self, request_json: &str) -> String {
            self.session.handle_json(request_json)
        }
    }

    fn js_error(error: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&error.to_string())
    }
}
#[cfg(target_arch = "wasm32")]
pub use player::BrowserPlayer;

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::EntityId;
    use titan_protocol::ErrorCode;
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
            [Operation::Inspect, Operation::Query, Operation::Capture]
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
