//! Shared surface acquisition, configuration, and presentation.

use crate::{GpuRenderer, wgpu};
use titan::render::{ImageAssets, RenderFrame};

/// A default surface presenter for native windows and browser canvases.
///
/// The caller creates the surface and retains ownership of its window/canvas,
/// game extraction, event loop, and timing. Use [`GpuRenderer`] directly when
/// custom adapter, device, or surface configuration is required.
pub struct SurfaceRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: GpuRenderer,
    suspended: bool,
}

impl SurfaceRenderer {
    /// Request a compatible adapter/device and configure the supplied surface.
    /// Uses portable WebGL2 limits and prefers non-sRGB presentation. Zero-sized
    /// surfaces are configured at 1x1 and suspended until a nonzero resize.
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("request GPU adapter: {error}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Titan game device"),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("request GPU device: {error}"))?;
        let (width, height) = bounded_size(width, height, device.limits().max_texture_dimension_2d);
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or("surface has no supported configuration")?;
        // The software reference blends in byte-space. A non-sRGB surface keeps
        // presentation from applying a second color-space conversion.
        if let Some(format) = surface
            .get_capabilities(&adapter)
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
        {
            config.format = format;
        }
        surface.configure(&device, &config);
        let renderer = GpuRenderer::new(device.clone(), queue.clone(), config.format);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            renderer,
            suspended: width == 0 || height == 0,
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

    /// Render and present a game-owned snapshot and its image assets.
    /// Returns `false` when suspended, occluded, timed out, or reconfigured after
    /// an outdated surface. Suboptimal frames are presented before reconfiguring.
    /// Surface loss/validation and renderer failures return an error; the host
    /// decides whether to exit or recreate its surface.
    pub fn render(&mut self, frame: &RenderFrame, assets: &ImageAssets) -> Result<bool, String> {
        if self.suspended {
            return Ok(false);
        }
        let (texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                return Err("GPU surface lost; restart the player".into());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("GPU surface validation failed".into());
            }
        };
        self.renderer
            .prepare(frame, assets)
            .map_err(|error| format!("prepare frame: {error}"))?;
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.renderer
            .render(&mut encoder, &view)
            .map_err(|error| format!("render frame: {error}"))?;
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        if suboptimal {
            self.surface.configure(&self.device, &self.config);
        }
        Ok(true)
    }
}

fn bounded_size(width: u32, height: u32, maximum: u32) -> (u32, u32) {
    (width.min(maximum), height.min(maximum))
}

#[cfg(test)]
mod tests {
    #[test]
    fn surface_dimensions_respect_device_limits_and_preserve_suspension() {
        assert_eq!(super::bounded_size(800, 560, 4096), (800, 560));
        assert_eq!(super::bounded_size(u32::MAX, 560, 4096), (4096, 560));
        assert_eq!(super::bounded_size(u32::MAX, u32::MAX, 4096), (4096, 4096));
        assert_eq!(super::bounded_size(0, u32::MAX, 4096), (0, 4096));
    }
}
