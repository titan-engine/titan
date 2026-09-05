use crate::game;
use titan::render::{ImageAssets, RenderFrame};
use titan_render_wgpu::{SurfaceRenderer, wgpu};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;
/// Interactive canvas runner. The browser owns keyboard events and animation timing.
#[wasm_bindgen]
pub struct BrowserPlayer {
    session: game::live::RpgSession,
    renderer: SurfaceRenderer,
    canvas: HtmlCanvasElement,
    accumulated_ms: f64,
    clock_epoch: u64,
}

#[wasm_bindgen]
impl BrowserPlayer {
    pub async fn create(canvas: HtmlCanvasElement) -> Result<BrowserPlayer, JsValue> {
        Self::from_session(canvas, crate::live_session()).await
    }

    /// Compatibility path: replace the player and generate the reference tree.
    pub async fn create_with_player_png(
        canvas: HtmlCanvasElement,
        bytes: Vec<u8>,
    ) -> Result<BrowserPlayer, JsValue> {
        let session = crate::live_session_from_app(crate::player_png_app(&bytes)?);
        Self::from_session(canvas, session).await
    }

    pub async fn create_with_pngs(
        canvas: HtmlCanvasElement,
        player: Vec<u8>,
        tree: Vec<u8>,
    ) -> Result<BrowserPlayer, JsValue> {
        let session = crate::live_session_from_app(crate::pngs_app(&player, &tree)?);
        Self::from_session(canvas, session).await
    }

    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), JsValue> {
        self.session.set_action(name, pressed).map_err(js_error)
    }

    pub fn journal_open(&self) -> bool {
        self.session.journal_open()
    }

    pub fn journal_key(&mut self, key: &str) -> bool {
        self.session.journal_key(key)
    }

    /// Canvas backing pixels; nonfinite coordinates represent an outside pointer.
    pub fn journal_pointer(&mut self, x: f64, y: f64, pressed: bool) -> bool {
        let point = self
            .session
            .app()
            .extracted::<RenderFrame>()
            .and_then(|frame| {
                titan::ui::point_from_surface(
                    x,
                    y,
                    f64::from(self.canvas.width()),
                    f64::from(self.canvas.height()),
                    frame.width(),
                    frame.height(),
                )
            });
        self.session.journal_pointer(point, pressed)
    }

    pub fn cancel_journal_input(&mut self) {
        self.session.cancel_journal_input();
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
        self.session.cancel_journal_input();
        let (width, height) = self.renderer.resize(width, height);
        self.canvas.set_width(width);
        self.canvas.set_height(height);
    }

    pub fn replay_reference(&mut self) {
        self.session.replay_reference();
        self.accumulated_ms = 0.0;
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
    pub fn load_recording(&mut self, json: &str) -> Result<(), JsValue> {
        self.session
            .load_replay(crate::parse_recording_json(json)?)
            .map_err(|error| JsValue::from_str(&error.message))
    }
    pub fn playback_active(&self) -> bool {
        self.session.replay_active()
    }
    pub fn playback_status(&self) -> String {
        self.session.replay_status().to_string()
    }
    pub fn step_playback(&mut self) -> Result<(), JsValue> {
        self.session
            .step_replay()
            .map_err(|error| JsValue::from_str(&error.message))
    }
    pub fn restart_playback(&mut self) -> Result<(), JsValue> {
        self.session
            .restart_replay()
            .map_err(|error| JsValue::from_str(&error.message))
    }
    pub fn exit_playback(&mut self) -> Result<(), JsValue> {
        self.session
            .stop_replay()
            .map_err(|error| JsValue::from_str(&error.message))
    }

    /// Inspect and control the exact session presented on this canvas.
    pub fn handle(&mut self, request_json: &str) -> String {
        self.session.handle_json(request_json)
    }
    /// Accept synchronously; the returned Promise owns completion, not the player.
    #[cfg(target_arch = "wasm32")]
    pub fn dispatch(&mut self, request_json: &str) -> titan::inspection::BrowserPromise {
        titan::inspection::response_promise(|| self.session.dispatch_json(request_json))
    }
}

impl BrowserPlayer {
    async fn from_session(
        canvas: HtmlCanvasElement,
        session: game::live::RpgSession,
    ) -> Result<Self, JsValue> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
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
        let clock_epoch = session.clock_epoch();
        Ok(Self {
            session,
            renderer,
            canvas,
            accumulated_ms: 0.0,
            clock_epoch,
        })
    }
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
