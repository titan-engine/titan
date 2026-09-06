//! Native GPU acceptance: cargo test --manifest-path games/factory/Cargo.toml --test render -- --ignored
#![cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use titan::render::{ImageAssets, RenderFrame};
use titan_factory::game;
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
        label: Some("factory readback target"),
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
        label: Some("factory readback"),
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
#[ignore = "requires a native GPU adapter; run explicitly on a graphics host"]
fn populated_factory_gpu_matches_software_after_pan_and_zoom() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("native GPU adapter required for factory readback");
        eprintln!("factory GPU adapter: {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .unwrap();
        let mut app = game::build_game();
        app.update_schedule(titan::Startup);
        for (kind, x, y, facing) in [
            ("extractor", 1, 3, "E"),
            ("conveyor", 2, 3, "E"),
            ("conveyor", 3, 3, "N"),
            ("conveyor", 3, 2, "E"),
            ("processor", 4, 2, "S"),
            ("conveyor", 4, 3, "E"),
            ("conveyor", 0, 0, "W"),
            ("processor", 11, 7, "N"),
        ] {
            game::player_command(
                &mut app,
                &serde_json::json!({"op":"place","kind":kind,"x":x,"y":y,"facing":facing})
                    .to_string(),
            )
            .unwrap();
        }
        let mut previous_checksum = None;
        // Fractional zoom, clipping and selection outlines all use the played scene.
        for (dx, dy, zoom) in [
            (0., 0., 1.),
            (19., -11., 1.7),
            (-47., 31., 0.5),
            (9., -9., 3.),
        ] {
            game::camera(&mut app, dx, dy, zoom).unwrap();
            game::pointer(&mut app, 192., 128., "hover").unwrap();
            let before = game::status(&app);
            let reference = game::render_image(app.world()).unwrap();
            let checksum = game::image_checksum(&reference);
            assert_ne!(
                previous_checksum,
                Some(checksum),
                "camera change must alter the reference scene"
            );
            previous_checksum = Some(checksum);
            for format in [
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ] {
                let mut renderer = GpuRenderer::new(device.clone(), queue.clone(), format);
                renderer
                    .prepare(
                        app.extracted::<RenderFrame>().unwrap(),
                        app.world().resource::<ImageAssets>().unwrap(),
                    )
                    .unwrap();
                let actual = pixels(
                    &device,
                    &queue,
                    &renderer,
                    game::WIDTH as u32,
                    game::HEIGHT as u32,
                    format,
                );
                assert_eq!(actual.len(), reference.pixels().len());
                let maximum_error = actual
                    .iter()
                    .zip(reference.pixels())
                    .map(|(actual, expected)| actual.abs_diff(*expected))
                    .max()
                    .unwrap();
                assert!(
                    maximum_error <= 1,
                    "{format:?}, camera delta ({dx},{dy},{zoom}): channel error {maximum_error} exceeds 1"
                );
                eprintln!(
                    "factory readback {format:?} camera ({dx},{dy},{zoom}): software {checksum:016x}, max channel error {maximum_error}"
                );
            }
            assert_eq!(
                game::status(&app),
                before,
                "capture must not advance or edit construction state"
            );
        }
    });
}
