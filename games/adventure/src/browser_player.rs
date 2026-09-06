//! Actual browser GPU host of the same inspectable fixed-tick player session.
use crate::{
    game,
    player::{PlayerSession, reference_recording},
};
use titan::render::{
    ImageAssets, RenderFrame,
    three_d::{Frame3dError, RenderFrame3d},
};
use titan_protocol::RunMode;
use titan_render_wgpu::{SurfaceRenderer3d, wgpu};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

#[wasm_bindgen]
pub struct BrowserPlayer {
    session: PlayerSession,
    captures: crate::capture::CaptureQueue,
    renderer: SurfaceRenderer3d,
    canvas: HtmlCanvasElement,
    accumulated_ms: f64,
    epoch: u64,
}
#[wasm_bindgen]
impl BrowserPlayer {
    pub async fn create(
        canvas: HtmlCanvasElement,
        backend: &str,
    ) -> Result<BrowserPlayer, JsValue> {
        let backends = match backend {
            "auto" => wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            "webgpu" => wgpu::Backends::BROWSER_WEBGPU,
            "webgl2" => wgpu::Backends::GL,
            _ => return Err(js("backend must be auto, webgpu or webgl2")),
        };
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(js)?;
        let mut renderer =
            SurfaceRenderer3d::new(&instance, surface, canvas.width(), canvas.height())
                .await
                .map_err(js)?;
        renderer.set_aspect_ratio(Some((16, 9))).map_err(js)?;
        let mut session = PlayerSession::new(
            "adventure-browser-player",
            "adventure",
            RunMode::Browser,
            false,
        );
        let captures = crate::capture::CaptureQueue::install(&mut session);
        let epoch = session.clock_epoch();
        Ok(Self {
            session,
            captures,
            renderer,
            canvas,
            accumulated_ms: 0.0,
            epoch,
        })
    }
    pub fn frame(&mut self, elapsed_ms: f64) -> Result<bool, JsValue> {
        if !elapsed_ms.is_finite() || elapsed_ms < 0.0 {
            return Err(js("elapsed milliseconds must be finite and nonnegative"));
        }
        if self.epoch != self.session.clock_epoch() {
            self.accumulated_ms = 0.0;
            self.epoch = self.session.clock_epoch();
        }
        if self.session.paused() || self.renderer.suspended() {
            self.accumulated_ms = 0.0;
        } else {
            self.accumulated_ms += elapsed_ms.min(250.0);
            let ticks = (self.accumulated_ms / (1000.0 / 60.0)).floor() as usize;
            self.accumulated_ms -= ticks as f64 * (1000.0 / 60.0);
            for _ in 0..ticks {
                if self.session.paused() {
                    self.accumulated_ms = 0.0;
                    break;
                }
                self.session.tick();
            }
        }
        let app = self.session.app();
        let scene = app
            .extracted::<Result<RenderFrame3d, Frame3dError>>()
            .ok_or_else(|| js("missing 3D extraction"))?
            .as_ref()
            .map_err(js)?;
        let overlay = app
            .extracted::<RenderFrame>()
            .ok_or_else(|| js("missing ECS overlay extraction"))?;
        let images = app
            .world()
            .resource::<ImageAssets>()
            .ok_or_else(|| js("missing UI images"))?;
        self.renderer
            .render(scene, crate::player::CAPTURE_CLEAR, overlay, images)
            .map_err(js)
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        self.session.clear_input();
        self.accumulated_ms = 0.0;
        let (width, height) = self.renderer.resize(width, height);
        self.canvas.set_width(width);
        self.canvas.set_height(height);
    }
    pub fn set_key(&mut self, code: &str, pressed: bool, repeat: bool) {
        self.session.set_key(code, pressed, repeat);
    }
    pub fn pointer(&mut self, x: f64, y: f64, pressed: bool) {
        let position = if x.is_finite()
            && y.is_finite()
            && (0.0..320.0).contains(&x)
            && (0.0..180.0).contains(&y)
        {
            Some((x.floor() as i32, y.floor() as i32))
        } else {
            None
        };
        self.session.pointer(position, pressed);
    }
    pub fn cancel_pointer(&mut self) {
        self.session.cancel_pointer();
    }
    pub fn clear_input(&mut self) {
        self.session.clear_input();
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
    pub fn step(&mut self) -> Result<(), JsValue> {
        self.session.step().map_err(|error| js(error.message))
    }
    pub fn restart(&mut self) {
        self.session.restart();
    }
    pub fn select_room(&mut self, room: u8) -> Result<(), JsValue> {
        self.session
            .select_room(room)
            .map_err(|error| js(error.message))
    }
    pub fn replay_route(&mut self) -> Result<(), JsValue> {
        self.session
            .load_replay(reference_recording())
            .map_err(|error| js(error.message))
    }
    pub fn load_recording(&mut self, json: &str) -> Result<(), JsValue> {
        if json.len() > 2 * 1024 * 1024 {
            return Err(js("recording exceeds 2 MiB"));
        }
        let recording = serde_json::from_str(json).map_err(js)?;
        self.session
            .load_replay(recording)
            .map_err(|error| js(error.message))
    }
    pub fn recording(&self) -> Result<String, JsValue> {
        serde_json::to_string(
            &game::recording(self.session.app()).map_err(|error| js(error.message))?,
        )
        .map_err(js)
    }
    pub fn status(&self) -> String {
        let mut status = game::status(self.session.app());
        status["playback"] = self.session.replay_status();
        status["adapter"] =
            serde_json::Value::String(format!("{:?}", self.renderer.adapter_info()));
        status["surface"] =
            serde_json::json!({"suspended":self.renderer.suspended(),"size":self.renderer.size()});
        status.to_string()
    }
    pub fn set_control_enabled(&mut self, enabled: bool) {
        self.session.set_control_enabled(enabled);
    }
    /// Dispatch owns its eventual result; the app borrow ends before awaiting.
    pub fn dispatch(&mut self, json: &str) -> js_sys::Promise {
        titan::inspection::response_promise(self.session.capture_timeout(), || {
            let dispatch = self.session.dispatch_json(json);
            let (device, queue) = self.renderer.capture_device();
            self.captures.start(device, queue);
            dispatch
        })
    }
}
fn js(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
