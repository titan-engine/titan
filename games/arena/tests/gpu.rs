//! Optional hardware integration: cargo test --test gpu -- --ignored
#![cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use titan::{
    Startup,
    render::{ImageAssets, RenderFrame},
};
use titan_game::game;
use titan_render_wgpu::{GpuRenderer, wgpu};

fn pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &GpuRenderer,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen reference target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("offscreen readback"),
        size: u64::from(row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer
        .render(&mut encoder, &target.create_view(&Default::default()))
        .unwrap();
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
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
    let submission = queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap()
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(10)),
        })
        .unwrap();
    receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let result = mapped
        .chunks(row as usize)
        .flat_map(|row| row[..width as usize * 4].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    result
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn arena_frames_match_software_pixels() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("native GPU required");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .unwrap();
        let mut app = game::build_game();
        app.update_schedule(Startup);
        // Initial art/HUD, active pursuit/contact, and settled loss cover distinct
        // scene compositions. Simulation assertions belong to the game tests.
        for ticks in [0, 120, 1200] {
            app.advance_fixed(ticks);
            let frame = app.extracted::<RenderFrame>().unwrap();
            let assets = app.world().resource::<ImageAssets>().unwrap();
            let reference = game::render_image(app.world()).unwrap();
            for format in [
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ] {
                let mut renderer = GpuRenderer::new(device.clone(), queue.clone(), format);
                renderer.prepare(frame, assets).unwrap();
                let actual = pixels(
                    &device,
                    &queue,
                    &renderer,
                    reference.width(),
                    reference.height(),
                    format,
                );
                assert_eq!(actual.len(), reference.pixels().len());
                let worst = actual
                    .iter()
                    .zip(reference.pixels())
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                assert_eq!(worst, 0, "GPU differs at advance {ticks}, {format:?}");
            }
        }
    });
}
