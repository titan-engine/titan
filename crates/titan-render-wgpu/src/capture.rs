//! Owned, nonblocking offscreen capture. Hosts own admission and provenance.
use crate::{Gpu3dError, GpuRenderer3d};
use std::{fmt, sync::mpsc, time::Duration};
use titan::render::{
    Image,
    three_d::{BaseColor, RenderFrame3d},
};

pub const MAX_CAPTURE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_CAPTURE_WAIT: Duration = Duration::from_secs(5);

/// Deliberately bounded diagnostics: backend errors may contain driver details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuCaptureError {
    Dimensions,
    Render(Gpu3dError),
    Readback,
    Timeout,
    Finished,
}
impl fmt::Display for GpuCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimensions => f.write_str("capture dimensions exceed readback budget"),
            Self::Render(e) => write!(f, "capture render: {e}"),
            Self::Readback => f.write_str("GPU capture readback failed"),
            Self::Timeout => f.write_str("GPU capture exceeded 5 seconds"),
            Self::Finished => f.write_str("GPU capture already finished or canceled"),
        }
    }
}
impl std::error::Error for GpuCaptureError {}

/// A fresh submission from an owned immutable frame (including resolved mesh assets).
/// No App, player, surface or previous prepared frame is retained. Poll on a host
/// timer even while paused; browser callers must yield to the event loop between
/// polls. Dropping cancels mapping and destroys the staging buffer; queued GPU
/// resources remain backend-owned until safe retirement.
pub struct OwnedGpuCapture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    buffer: Option<wgpu::Buffer>,
    receiver: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    width: u32,
    height: u32,
    row: u32,
}
impl OwnedGpuCapture {
    pub fn three_d(
        device: wgpu::Device,
        queue: wgpu::Queue,
        frame: RenderFrame3d,
        width: u32,
        height: u32,
        clear: BaseColor,
    ) -> Result<Self, GpuCaptureError> {
        let (row, bytes) = layout(width, height, &device.limits())?;
        let mut renderer = GpuRenderer3d::new(
            device.clone(),
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
        )
        .map_err(GpuCaptureError::Render)?;
        renderer
            .prepare(&frame, clear)
            .map_err(GpuCaptureError::Render)?;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Titan capture staging"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer
            .render(&mut encoder)
            .map_err(GpuCaptureError::Render)?;
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: renderer.color_texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (sender, receiver) = mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        Ok(Self {
            device,
            queue,
            buffer: Some(buffer),
            receiver,
            width,
            height,
            row,
        })
    }
    /// `elapsed` is monotonic time since request acceptance, including preparation.
    /// This never blocks; completion is emitted once. Hosts enforce the same
    /// request deadline through encoding and delivery, after this job finishes.
    pub fn poll(&mut self, elapsed: Duration) -> Result<Option<Image>, GpuCaptureError> {
        if self.buffer.is_none() {
            return Err(GpuCaptureError::Finished);
        }
        if elapsed >= MAX_CAPTURE_WAIT {
            self.cancel();
            return Err(GpuCaptureError::Timeout);
        }
        if self.device.poll(wgpu::PollType::Poll).is_err() {
            self.destroy_failed();
            return Err(GpuCaptureError::Readback);
        }
        match self.receiver.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return Ok(None),
            Ok(Ok(())) => {}
            _ => {
                self.destroy_failed();
                return Err(GpuCaptureError::Readback);
            }
        }
        let result = (|| {
            let mapped = self
                .buffer
                .as_ref()
                .unwrap()
                .slice(..)
                .get_mapped_range()
                .map_err(|_| GpuCaptureError::Readback)?;
            let mut pixels = Vec::with_capacity(self.width as usize * self.height as usize * 4);
            for row in mapped.chunks_exact(self.row as usize) {
                pixels.extend_from_slice(&row[..self.width as usize * 4]);
            }
            Image::new(self.width, self.height, pixels).map_err(|_| GpuCaptureError::Readback)
        })();
        if result.is_ok() {
            self.cancel();
        } else {
            self.destroy_failed();
        }
        result.map(Some)
    }
    /// Cancel staging and keep host admission alive until submitted GPU work
    /// retires. Move the CaptureCompleter into this callback, then drop it there.
    /// The host must continue polling the device (or another job on this device)
    /// to drive retirement on native/WebGL. Browser WebGPU drives callbacks itself.
    pub fn retire(mut self, on_retired: impl FnOnce() + Send + 'static) {
        self.cancel();
        self.queue.on_submitted_work_done(on_retired);
    }
    fn destroy_failed(&mut self) {
        // A failed map is already unmapped; calling unmap again is a wgpu
        // validation error on native backends.
        if let Some(buffer) = self.buffer.take() {
            buffer.destroy();
        }
    }
    pub fn cancel(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            if !matches!(self.receiver.try_recv(), Ok(Err(_))) {
                buffer.unmap();
            }
            buffer.destroy();
        }
    }
}
impl Drop for OwnedGpuCapture {
    fn drop(&mut self) {
        self.cancel();
    }
}
fn layout(width: u32, height: u32, limits: &wgpu::Limits) -> Result<(u32, u64), GpuCaptureError> {
    if width == 0
        || height == 0
        || width > limits.max_texture_dimension_2d
        || height > limits.max_texture_dimension_2d
    {
        return Err(GpuCaptureError::Dimensions);
    }
    let row = u64::from(width)
        .checked_mul(4)
        .and_then(|v| v.checked_add(255))
        .map(|v| v / 256 * 256)
        .ok_or(GpuCaptureError::Dimensions)?;
    let bytes = row
        .checked_mul(u64::from(height))
        .ok_or(GpuCaptureError::Dimensions)?;
    if row > u64::from(u32::MAX) || bytes > MAX_CAPTURE_BYTES || bytes > limits.max_buffer_size {
        return Err(GpuCaptureError::Dimensions);
    }
    Ok((row as u32, bytes))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "requires a native GPU to exercise actual map failure and retirement"]
    fn aborted_backend_map_returns_bounded_failure_and_retires() {
        pollster::block_on(async {
            use titan::render::three_d::*;
            let adapter = wgpu::Instance::default()
                .request_adapter(&Default::default())
                .await
                .unwrap();
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    ..Default::default()
                })
                .await
                .unwrap();
            let frame = RenderFrame3d::new(
                PerspectiveCamera::new(
                    Vec3::ZERO,
                    Quaternion::IDENTITY,
                    std::f32::consts::FRAC_PI_2,
                    1.,
                    1.,
                    10.,
                )
                .unwrap(),
                Lighting3d::new(Vec3::ONE, 1., 0.).unwrap(),
                &MeshAssets::new(),
                [],
                Frame3dLimits::default(),
            )
            .unwrap();
            let mut job = OwnedGpuCapture::three_d(
                device.clone(),
                queue,
                frame,
                64,
                64,
                BaseColor::rgb(0, 0, 0),
            )
            .unwrap();
            // Abort the real outstanding map without marking the job canceled.
            // This forces the backend callback error through the production poll path.
            job.buffer.as_ref().unwrap().unmap();
            let start = std::time::Instant::now();
            loop {
                match job.poll(start.elapsed()) {
                    Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                    Err(GpuCaptureError::Readback) => break,
                    other => panic!("expected readback failure, got {other:?}"),
                }
            }
            assert!(job.buffer.is_none());
            let (sender, receiver) = mpsc::channel();
            job.retire(move || {
                let _ = sender.send(());
            });
            while receiver.try_recv().is_err() {
                assert!(
                    start.elapsed() < MAX_CAPTURE_WAIT,
                    "failed map resources did not retire"
                );
                device.poll(wgpu::PollType::Poll).unwrap();
                std::thread::sleep(Duration::from_millis(1));
            }
        });
    }
    #[test]
    fn padded_readback_is_bounded_before_allocating() {
        let limits = wgpu::Limits::default();
        assert_eq!(layout(65, 2, &limits), Ok((512, 1024)));
        for size in [(0, 1), (1, 0), (u32::MAX, 1), (4096, 4096)] {
            assert_eq!(
                layout(size.0, size.1, &limits),
                Err(GpuCaptureError::Dimensions)
            );
        }
        assert_eq!(
            layout(
                65,
                2,
                &wgpu::Limits {
                    max_buffer_size: 1000,
                    ..limits
                }
            ),
            Err(GpuCaptureError::Dimensions)
        );
    }
}
