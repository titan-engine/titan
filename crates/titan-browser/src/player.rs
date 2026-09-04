use titan::{App, Startup};
use titan_render_wgpu::wgpu;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::game;
#[path = "../../../examples/support/gpu_surface.rs"]
mod gpu_surface;
use gpu_surface::SurfaceRenderer;

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

    pub fn replay_reference(&mut self) {
        self.app = game::build_game();
        self.input = game::InteractiveInput::default();
        self.accumulated_ms = 0.0;
        game::replay(&mut self.app, &game::recorded_walk());
    }

    pub fn status(&self) -> String {
        game::status(&self.app)
    }
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
