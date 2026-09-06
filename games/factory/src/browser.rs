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
        let config = InspectionConfig::controlled("factory-browser", "factory");
        let inspector = game::inspector_with_capture(config, capture);
        Self {
            session: BrowserSession::new(app, inspector, enable_control),
        }
    }

    /// Construct an explicit bounded transport test setup; not a player command.
    pub fn transport_fixture(name: &str, enable_control: bool) -> Result<BrowserRuntime, JsValue> {
        let mut app = game::build_transport_fixture(name).map_err(|e| JsValue::from_str(&e))?;
        app.update_schedule(Startup);
        let inspector = game::inspector_with_capture(
            InspectionConfig::controlled("factory-browser", "factory"),
            capture,
        );
        Ok(Self {
            session: BrowserSession::new(app, inspector, enable_control),
        })
    }

    /// Construct an explicit bounded production test setup; not a player command.
    pub fn production_fixture(name: &str, enable_control: bool) -> Result<BrowserRuntime, JsValue> {
        let mut app = game::build_production_fixture(name).map_err(|e| JsValue::from_str(&e))?;
        app.update_schedule(Startup);
        let inspector = game::inspector_with_capture(
            InspectionConfig::controlled("factory-browser", "factory"),
            capture,
        );
        Ok(Self {
            session: BrowserSession::new(app, inspector, enable_control),
        })
    }

    /// Executes one request at a safe point and returns its JSON response envelope.
    pub fn handle(&mut self, request_json: &str) -> String {
        self.session.handle(request_json)
    }
    /// Accept synchronously; the returned Promise owns completion, not the player.
    #[cfg(target_arch = "wasm32")]
    pub fn dispatch(&mut self, request_json: &str) -> titan::inspection::BrowserPromise {
        titan::inspection::response_promise(self.session.capture_timeout(), || {
            self.session.dispatch_json(request_json)
        })
    }
}

fn capture(app: &App) -> Result<CaptureResult, ProtocolError> {
    let image = game::render_image(app.world())?;
    titan_diagnostics::png_capture(&image)
}

#[cfg(target_arch = "wasm32")]
mod player {
    use titan::{
        App, Startup,
        render::{ImageAssets, RenderFrame},
    };
    use titan_render_wgpu::{SurfaceRenderer, wgpu};
    use wasm_bindgen::prelude::*;
    use web_sys::HtmlCanvasElement;

    use crate::game;

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
            Self::create_with_app(canvas, game::build_game()).await
        }

        /// Separate fixture constructor keeps seeding out of player commands.
        pub async fn create_transport_fixture(
            canvas: HtmlCanvasElement,
            name: &str,
        ) -> Result<BrowserPlayer, JsValue> {
            Self::create_with_app(
                canvas,
                game::build_transport_fixture(name).map_err(js_error)?,
            )
            .await
        }
    }

    impl BrowserPlayer {
        async fn create_with_app(
            canvas: HtmlCanvasElement,
            mut app: App,
        ) -> Result<BrowserPlayer, JsValue> {
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
            app.update_schedule(Startup);
            let input = game::InteractiveInput::for_app(&app);
            Ok(Self {
                app,
                input,
                renderer,
                canvas,
                accumulated_ms: 0.0,
            })
        }
    }

    #[wasm_bindgen]
    impl BrowserPlayer {
        pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), JsValue> {
            self.input
                .set_action(&self.app, name, pressed)
                .map_err(js_error)
        }

        /// Cancel held actions and buffered taps on focus loss or pause.
        pub fn clear_input(&mut self) {
            self.input = game::InteractiveInput::for_app(&self.app);
            self.accumulated_ms = 0.0;
        }

        /// Cancel one interrupted gesture without dropping other buffered actions.
        pub fn cancel_action(&mut self, name: &str) -> Result<(), JsValue> {
            self.input.cancel_action(&self.app, name).map_err(js_error)
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
            let frame = self
                .app
                .extracted::<RenderFrame>()
                .ok_or_else(|| js_error("game render extraction unavailable"))?;
            let assets = self
                .app
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
            super::restart_player(&mut self.app, &mut self.input, &mut self.accumulated_ms);
        }

        /// Player construction uses the same validated boundary operations as tools.
        pub fn command(&mut self, json: &str) -> Result<String, JsValue> {
            game::player_command(&mut self.app, json).map_err(js_error)
        }

        /// Coordinates are in the fixed 384 by 256 logical framebuffer.
        pub fn pointer(&mut self, x: f64, y: f64, action: &str) -> Result<String, JsValue> {
            game::pointer(&mut self.app, x, y, action).map_err(js_error)
        }

        pub fn camera(&mut self, dx: f64, dy: f64, zoom: f64) -> Result<(), JsValue> {
            game::camera(&mut self.app, dx, dy, zoom).map_err(js_error)
        }

        /// Exact user-requested ticks, independent of the animation accumulator.
        pub fn step(&mut self, ticks: u32) -> Result<(), JsValue> {
            if ticks > 600 {
                return Err(js_error("step accepts at most 600 ticks"));
            }
            self.clear_input();
            for _ in 0..ticks {
                self.input.tick(&mut self.app);
            }
            self.frame(0.0)
        }

        pub fn preview_action(&mut self, action: &str) -> Result<(), JsValue> {
            game::set_preview_action(&mut self.app, action).map_err(js_error)
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
    *input = game::InteractiveInput::for_app(app);
    *accumulated_ms = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::EntityId;
    use titan_protocol::ErrorCode;
    use titan_protocol::{
        Operation, Request, RequestEnvelope, Response, ResponseEnvelope, ResponseOutcome, RunMode,
    };

    #[test]
    fn player_restart_preserves_frame_and_clears_input() {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let mut input = game::InteractiveInput::for_app(&app);
        input.set_action(&app, "right", true).unwrap();
        input.tick(&mut app);
        let mut accumulated_ms = 12.0;
        restart_player(&mut app, &mut input, &mut accumulated_ms);
        let reset: serde_json::Value = serde_json::from_str(&game::status(&app)).unwrap();
        assert_eq!(reset["frame"], 1);
        assert_eq!(accumulated_ms, 0.0);
        input.tick(&mut app);
        let next: serde_json::Value = serde_json::from_str(&game::status(&app)).unwrap();
        assert_eq!(next["frame"], 2);
        assert_eq!(next["camera"], reset["camera"]);
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
