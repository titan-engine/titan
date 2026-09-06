//! Shared surface acquisition, configuration, and presentation.

use std::sync::{Arc, Mutex};

use crate::{GpuRenderer, GpuSceneRenderer3d, wgpu};
use titan::render::three_d::{BaseColor, RenderFrame3d};
use titan::render::{ImageAssets, RenderFrame};

/// A default surface presenter for native windows and browser canvases.
///
/// The caller creates the surface and retains ownership of its window/canvas,
/// game extraction, event loop, and timing. Use [`GpuRenderer`] directly when
/// custom adapter, device, or surface configuration is required.
struct SurfaceState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    suspended: bool,
    error: Arc<Mutex<Option<String>>>,
    adapter_info: wgpu::AdapterInfo,
}

impl SurfaceState {
    /// Request a compatible adapter/device and configure the supplied surface.
    /// Uses portable WebGL2 limits and prefers non-sRGB presentation. Zero-sized
    /// surfaces are configured at 1x1 and suspended until a nonzero resize.
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        scene_3d: bool,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("request GPU adapter: {error}"))?;
        for (format, usage) in [
            (
                wgpu::TextureFormat::Depth24Plus,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            (
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            (
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
            ),
        ] {
            if format == wgpu::TextureFormat::Depth24Plus && !scene_3d {
                continue;
            }
            let features = adapter.get_texture_format_features(format);
            if !features.allowed_usages.contains(usage)
                || (format == wgpu::TextureFormat::Rgba16Float
                    && !features.flags.contains(
                        wgpu::TextureFormatFeatureFlags::BLENDABLE
                            | wgpu::TextureFormatFeatureFlags::FILTERABLE,
                    ))
            {
                return Err(format!(
                    "GPU backend lacks required {format:?} render/sample/blend support; WebGL2 requires floating-point color attachments"
                ));
            }
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Titan game device"),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("request GPU device: {error}"))?;
        let error = Arc::new(Mutex::new(None));
        let errors = error.clone();
        device.on_uncaptured_error(Arc::new(move |failure| {
            *errors.lock().unwrap() = Some(format!("GPU error: {failure}; restart the player"));
        }));
        let errors = error.clone();
        device.set_device_lost_callback(move |reason, message| {
            *errors.lock().unwrap() = Some(format!(
                "GPU device lost ({reason:?}): {message}; restart the player"
            ));
        });
        let (width, height) = bounded_size(width, height, device.limits().max_texture_dimension_2d);
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or("surface has no supported configuration")?;
        // The software reference blends in byte-space. A non-sRGB surface keeps
        // presentation from applying a second color-space conversion.
        let capabilities = surface.get_capabilities(&adapter);
        let supported = |format: &&wgpu::TextureFormat| {
            !scene_3d
                || matches!(
                    format,
                    wgpu::TextureFormat::Rgba8Unorm
                        | wgpu::TextureFormat::Rgba8UnormSrgb
                        | wgpu::TextureFormat::Bgra8Unorm
                        | wgpu::TextureFormat::Bgra8UnormSrgb
                )
        };
        config.format = capabilities
            .formats
            .iter()
            .filter(supported)
            .find(|format| !format.is_srgb())
            .or_else(|| capabilities.formats.iter().find(supported))
            .copied()
            .ok_or("surface has no supported RGBA/BGRA8 composition format")?;
        surface.configure(&device, &config);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            suspended: width == 0 || height == 0,
            error,
            adapter_info: adapter.get_info(),
        })
    }

    /// Returns the actual dimensions, bounded by the device texture limit.
    pub fn resize(&mut self, width: u32, height: u32) -> (u32, u32) {
        let (width, height) =
            bounded_size(width, height, self.device.limits().max_texture_dimension_2d);
        self.suspended = width == 0 || height == 0;
        if self.suspended {
            return (width, height);
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        (width, height)
    }

    fn check_error(&self) -> Result<(), String> {
        match self.error.lock().unwrap().as_ref() {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
    fn acquire(&mut self) -> Result<Option<(wgpu::SurfaceTexture, bool)>, String> {
        self.check_error()?;
        if self.suspended {
            return Ok(None);
        }
        acquired_texture(self.surface.get_current_texture(), || {
            self.surface.configure(&self.device, &self.config);
        })
    }

    fn present(
        &self,
        texture: wgpu::SurfaceTexture,
        suboptimal: bool,
        encoder: wgpu::CommandEncoder,
    ) -> Result<(), String> {
        self.check_error()?;
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        if suboptimal {
            self.surface.configure(&self.device, &self.config);
        }
        self.check_error()
    }
}

/// Default sprite surface presenter. Event loops and extraction remain host-owned.
pub struct SurfaceRenderer {
    state: SurfaceState,
    renderer: GpuRenderer,
}
impl SurfaceRenderer {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let state = SurfaceState::new(instance, surface, width, height, false).await?;
        let renderer = GpuRenderer::new(
            state.device.clone(),
            state.queue.clone(),
            state.config.format,
        );
        Ok(Self { state, renderer })
    }
    pub fn resize(&mut self, width: u32, height: u32) -> (u32, u32) {
        self.state.resize(width, height)
    }
    /// False means suspended, occluded, timed out, or outdated/reconfigured.
    /// Lost/invalid surfaces return an error; hosts must stop or recreate them.
    pub fn render(&mut self, frame: &RenderFrame, assets: &ImageAssets) -> Result<bool, String> {
        let Some((texture, suboptimal)) = self.state.acquire()? else {
            return Ok(false);
        };
        self.renderer
            .prepare(frame, assets)
            .map_err(|e| e.to_string())?;
        let view = texture.texture.create_view(&Default::default());
        let mut encoder = self
            .state
            .device
            .create_command_encoder(&Default::default());
        self.renderer
            .render(&mut encoder, &view)
            .map_err(|e| e.to_string())?;
        self.state.present(texture, suboptimal, encoder)?;
        Ok(true)
    }
}

/// Native/browser 3D scene and byte-space UI presenter using the same surface
/// acquisition and failure policy as the sprite presenter.
pub struct SurfaceRenderer3d {
    state: SurfaceState,
    renderer: GpuSceneRenderer3d,
    aspect_ratio: Option<(u32, u32)>,
    requested_size: (u32, u32),
}
impl SurfaceRenderer3d {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let requested_size = (width, height);
        let (width, height) = bounded_size(width, height, 2048);
        let state = SurfaceState::new(instance, surface, width, height, true).await?;
        let renderer = GpuSceneRenderer3d::new(
            state.device.clone(),
            state.queue.clone(),
            state.config.width,
            state.config.height,
            state.config.format,
        )?;
        Ok(Self {
            state,
            renderer,
            aspect_ratio: None,
            requested_size,
        })
    }
    /// Fit scene and UI to a centered aspect ratio, clearing unused pixels black.
    /// `None` (the default) fills the entire surface. Both ratio terms must be
    /// nonzero. The game must use the matching camera aspect; capture dimensions
    /// remain caller-owned and are unaffected. Fitting rounds down to whole pixels
    /// with a minimum of one pixel per axis on very small surfaces. Oversized
    /// surfaces are proportionally bounded to preserve window/canvas proportions.
    pub fn set_aspect_ratio(&mut self, ratio: Option<(u32, u32)>) -> Result<(), String> {
        if ratio.is_some_and(|(width, height)| width == 0 || height == 0) {
            return Err("surface aspect ratio terms must be nonzero".into());
        }
        self.aspect_ratio = ratio;
        self.resize(self.requested_size.0, self.requested_size.1);
        Ok(())
    }
    /// Cloned handles for owned asynchronous offscreen jobs on this adapter.
    pub fn capture_device(&self) -> (wgpu::Device, wgpu::Queue) {
        (self.state.device.clone(), self.state.queue.clone())
    }
    /// Bound each axis to 2048 and device limits to bound scene/UI allocations.
    /// With a fixed aspect ratio, scale both axes together before fitting the scene.
    /// A zero dimension suspends presentation, preserving existing targets.
    pub fn resize(&mut self, width: u32, height: u32) -> (u32, u32) {
        self.requested_size = (width, height);
        let maximum = 2048.min(self.state.device.limits().max_texture_dimension_2d);
        let (width, height) = if self.aspect_ratio.is_some() {
            proportionally_bounded_size(width, height, maximum)
        } else {
            bounded_size(width, height, maximum)
        };
        let size = self.state.resize(width, height);
        if !self.state.suspended {
            // Dimensions are bounded below the renderer's validated budget.
            let (_, _, width, height) = fitted_viewport(size, self.aspect_ratio);
            self.renderer
                .resize(width, height)
                .expect("bounded surface dimensions");
        }
        size
    }
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.state.adapter_info
    }
    pub fn suspended(&self) -> bool {
        self.state.suspended
    }
    pub fn size(&self) -> (u32, u32) {
        (self.state.config.width, self.state.config.height)
    }
    /// Renders the immutable scene then UI without depth; see SurfaceRenderer
    /// for skipped frames and fatal surface/device errors.
    pub fn render(
        &mut self,
        frame: &RenderFrame3d,
        clear: BaseColor,
        overlay: &RenderFrame,
        assets: &ImageAssets,
    ) -> Result<bool, String> {
        let Some((texture, suboptimal)) = self.state.acquire()? else {
            return Ok(false);
        };
        self.renderer.prepare(frame, clear, overlay, assets)?;
        let view = texture.texture.create_view(&Default::default());
        let mut encoder = self
            .state
            .device
            .create_command_encoder(&Default::default());
        self.renderer.render_in_viewport(
            &mut encoder,
            &view,
            self.aspect_ratio
                .map(|_| fitted_viewport(self.size(), self.aspect_ratio)),
        )?;
        self.state.present(texture, suboptimal, encoder)?;
        Ok(true)
    }
}

// Scale an oversized backing surface uniformly so the compositor's viewport
// is not distorted again when the OS/browser maps it to the requested size.
fn proportionally_bounded_size(width: u32, height: u32, maximum: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= maximum {
        return (width, height);
    }
    let scale = |axis| {
        if axis == 0 {
            0
        } else {
            (u64::from(axis) * u64::from(maximum) / u64::from(longest)).max(1) as u32
        }
    };
    (scale(width), scale(height))
}

// Inputs are nonzero configured surface dimensions and a validated ratio.
fn fitted_viewport((width, height): (u32, u32), ratio: Option<(u32, u32)>) -> (u32, u32, u32, u32) {
    let Some((rw, rh)) = ratio else {
        return (0, 0, width, height);
    };
    let (inner_width, inner_height) =
        if u64::from(width) * u64::from(rh) > u64::from(height) * u64::from(rw) {
            (
                (u64::from(height) * u64::from(rw) / u64::from(rh)).max(1) as u32,
                height,
            )
        } else {
            (
                width,
                (u64::from(width) * u64::from(rh) / u64::from(rw)).max(1) as u32,
            )
        };
    (
        (width - inner_width) / 2,
        (height - inner_height) / 2,
        inner_width,
        inner_height,
    )
}

// Keep the acquisition policy independent from a live OS surface so every
// failure status can be exercised without intentionally losing a real device.
fn acquired_texture(
    status: wgpu::CurrentSurfaceTexture,
    reconfigure: impl FnOnce(),
) -> Result<Option<(wgpu::SurfaceTexture, bool)>, String> {
    match status {
        wgpu::CurrentSurfaceTexture::Success(texture) => Ok(Some((texture, false))),
        wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Ok(Some((texture, true))),
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => Ok(None),
        wgpu::CurrentSurfaceTexture::Outdated => {
            reconfigure();
            Ok(None)
        }
        wgpu::CurrentSurfaceTexture::Lost => Err("GPU surface lost; restart the player".into()),
        wgpu::CurrentSurfaceTexture::Validation => {
            Err("GPU surface validation failed; restart the player".into())
        }
    }
}

fn bounded_size(width: u32, height: u32, maximum: u32) -> (u32, u32) {
    (width.min(maximum), height.min(maximum))
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixed_aspect_surface_bounds_preserve_high_dpi_proportions() {
        use super::proportionally_bounded_size as bound;
        assert_eq!(bound(2560, 1440, 2048), (2048, 1152));
        assert_eq!(bound(2200, 1238, 2048), (2048, 1152));
        assert_eq!(bound(1440, 2560, 2048), (1152, 2048));
        assert_eq!(bound(2560, 1440, 1024), (1024, 576));
        assert_eq!(bound(960, 540, 2048), (960, 540));
        assert_eq!(bound(0, 3000, 2048), (0, 2048));
        assert_eq!(bound(3000, 0, 2048), (2048, 0));
        assert_eq!(bound(0, 0, 2048), (0, 0));
        assert_eq!(bound(u32::MAX, 1, 2048), (2048, 1));
        assert_eq!(bound(u32::MAX, u32::MAX, 2048), (2048, 2048));
    }
    #[test]
    fn fixed_aspect_fits_centered_without_overflow_or_empty_targets() {
        use super::fitted_viewport as fit;
        assert_eq!(fit((960, 540), Some((16, 9))), (0, 0, 960, 540));
        assert_eq!(fit((960, 720), Some((16, 9))), (0, 90, 960, 540));
        assert_eq!(fit((1280, 540), Some((16, 9))), (160, 0, 960, 540));
        assert_eq!(fit((961, 541), Some((16, 9))), (0, 0, 961, 540));
        assert_eq!(fit((1, 1), Some((16, 9))), (0, 0, 1, 1));
        assert_eq!(fit((100, 100), Some((u32::MAX, 1))), (0, 49, 100, 1));
        assert_eq!(fit((100, 100), Some((1, u32::MAX))), (49, 0, 1, 100));
        assert_eq!(fit((100, 200), None), (0, 0, 100, 200));
    }
    #[test]
    fn surface_failures_skip_reconfigure_or_fail_explicitly() {
        use super::{acquired_texture, wgpu::CurrentSurfaceTexture as Status};
        for status in [Status::Timeout, Status::Occluded] {
            assert!(
                acquired_texture(status, || panic!("transient failure must not reconfigure"))
                    .unwrap()
                    .is_none()
            );
        }
        let mut count = 0;
        assert!(
            acquired_texture(Status::Outdated, || count += 1)
                .unwrap()
                .is_none()
        );
        assert_eq!(count, 1);
        for status in [Status::Lost, Status::Validation] {
            let error =
                acquired_texture(status, || panic!("fatal failure must reach host")).unwrap_err();
            assert!(error.contains("restart the player"));
        }
    }
    #[test]
    fn surface_dimensions_respect_device_limits_and_preserve_suspension() {
        assert_eq!(super::bounded_size(800, 560, 4096), (800, 560));
        assert_eq!(super::bounded_size(u32::MAX, 560, 4096), (4096, 560));
        assert_eq!(super::bounded_size(u32::MAX, u32::MAX, 4096), (4096, 4096));
        assert_eq!(super::bounded_size(0, u32::MAX, 4096), (0, 4096));
    }
}
