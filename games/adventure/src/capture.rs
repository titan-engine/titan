//! Safe-point freezing and bounded GPU completion, independent of presentation.
use crate::player::{FrozenCapture, PlayerSession};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use titan::inspection::CaptureCompleter;
use titan_protocol::{CaptureIdentity, ErrorCode, ProtocolError};
use titan_render_wgpu::{OwnedGpuCapture, wgpu};

type Work = (FrozenCapture, CaptureIdentity, CaptureCompleter);
#[derive(Default)]
pub struct CaptureQueue(Arc<Mutex<Option<Work>>>);
impl CaptureQueue {
    pub fn install(session: &mut PlayerSession) -> Self {
        let queue = Self::default();
        let work = queue.0.clone();
        session.register_capture(move |app, identity, completion| {
            let snapshot = FrozenCapture::new(app)?;
            *work.lock().unwrap() = Some((snapshot, identity, completion));
            Ok(())
        });
        queue
    }
    /// Take owned work after dispatch; no session/renderer borrow reaches a wait.
    pub fn start(&self, device: wgpu::Device, queue: wgpu::Queue) {
        let Some((snapshot, identity, completion)) = self.0.lock().unwrap().take() else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || {
            pollster::block_on(run(snapshot, identity, completion, device, queue))
        });
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(run(snapshot, identity, completion, device, queue));
    }
}
async fn run(
    snapshot: FrozenCapture,
    identity: CaptureIdentity,
    completion: CaptureCompleter,
    device: wgpu::Device,
    queue: wgpu::Queue,
) {
    let started = now();
    let elapsed = || Duration::from_secs_f64(((now() - started) / 1000.0).max(0.0));
    let mut capture = match OwnedGpuCapture::composed(
        device.clone(),
        queue,
        snapshot.scene,
        snapshot.overlay,
        snapshot.assets,
        (identity.width, identity.height),
        crate::player::CAPTURE_CLEAR,
    ) {
        Ok(capture) => capture,
        Err(error) => {
            completion.complete(Err(ProtocolError::new(
                ErrorCode::Internal,
                error.to_string(),
            )));
            return;
        }
    };
    let result = loop {
        if completion.is_cancelled() {
            break None;
        }
        match capture.poll(elapsed()) {
            Ok(None) => wait().await,
            Ok(Some(image)) => {
                let result = titan_diagnostics::png_capture(&image).map(|mut result| {
                    result.identity = identity;
                    result
                });
                break Some(result);
            }
            Err(error) => {
                break Some(Err(ProtocolError::new(
                    if matches!(error, titan_render_wgpu::GpuCaptureError::Timeout) {
                        ErrorCode::Timeout
                    } else {
                        ErrorCode::Internal
                    },
                    error.to_string(),
                )));
            }
        }
    };
    // Retain admission through GPU retirement, including timeout/cancellation.
    let retired = Arc::new(AtomicBool::new(false));
    let signal = retired.clone();
    capture.retire(move || {
        if let Some(result) = result {
            completion.complete(result);
        }
        signal.store(true, Ordering::Release);
    });
    // Native/WebGL callbacks require polling even without a presentation loop.
    // Device loss releases callback ownership; do not retain a timer forever.
    while !retired.load(Ordering::Acquire) && elapsed() < Duration::from_secs(10) {
        if device.poll(wgpu::PollType::Poll).is_err() {
            break;
        }
        wait().await;
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn now() -> f64 {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ORIGIN
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1000.0
}
#[cfg(target_arch = "wasm32")]
fn now() -> f64 {
    web_sys::window().unwrap().performance().unwrap().now()
}
#[cfg(not(target_arch = "wasm32"))]
async fn wait() {
    std::thread::sleep(Duration::from_millis(4));
}
#[cfg(target_arch = "wasm32")]
async fn wait() {
    use wasm_bindgen::JsCast;
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(resolve.unchecked_ref(), 4)
            .unwrap();
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
