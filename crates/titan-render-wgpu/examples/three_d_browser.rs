//! Browser entry point for the same opt-in GPU fixture used by native tests.
#[cfg(target_arch = "wasm32")]
#[path = "support/three_d_fixture.rs"]
mod fixture;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Select exactly one real GPU backend; never silently fall back to another.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn verify_three_d(
    backend: &str,
    canvas: web_sys::HtmlCanvasElement,
) -> Result<String, JsValue> {
    use titan_render_wgpu::wgpu;
    let backends = match backend {
        "webgpu" => wgpu::Backends::BROWSER_WEBGPU,
        "webgl2" => wgpu::Backends::GL,
        _ => return Err(JsValue::from_str("select webgpu or webgl2")),
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    // wgpu's WebGL adapter is tied to a canvas context, even for offscreen draws.
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|error| JsValue::from_str(&format!("{backend} canvas: {error}")))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .map_err(|error| JsValue::from_str(&format!("{backend} unavailable: {error}")))?;
    let formats = fixture::validate_adapter(&adapter).map_err(|e| JsValue::from_str(&e))?;
    let info = format!("{:?}", adapter.get_info());
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            ..Default::default()
        })
        .await
        .map_err(|error| JsValue::from_str(&format!("{backend} device: {error}")))?;
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = fixture::run(&device, &queue).await;
    if let Some(error) = validation.pop().await {
        return Err(JsValue::from_str(&format!(
            "{backend} GPU validation: {error}"
        )));
    }
    let evidence = result.map_err(|error| JsValue::from_str(&error))?;
    serde_json::to_string(&serde_json::json!({
        "backend": backend,
        "adapter": info,
        "formats": formats,
        "evidence": evidence,
    }))
    .map_err(|error| JsValue::from_str(&error.to_string()))
}
