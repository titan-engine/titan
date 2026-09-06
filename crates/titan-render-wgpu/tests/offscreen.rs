//! Opt-in hardware verification: cargo test -p titan-render-wgpu --test offscreen -- --ignored
#![cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use titan::render::{Color, Image, ImageAssets, RenderFrame, SoftwareRenderer, SpriteDraw};
use titan_render_wgpu::{GpuError, GpuRenderer, wgpu};

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
fn compare(actual: &[u8], reference: &[u8], tolerance: u8) {
    assert_eq!(actual.len(), reference.len());
    let worst = actual
        .iter()
        .zip(reference)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(
        worst <= tolerance,
        "maximum channel error {worst} exceeds tolerance {tolerance}"
    );
}

#[test]
#[ignore = "requires a native GPU adapter; exercised manually or in GPU-enabled CI"]
fn textured_quads_match_software_reference_and_resize() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("GPU adapter required for opt-in test");
        eprintln!("offscreen adapter: {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .unwrap();
        let mut assets = ImageAssets::new();
        let checker = assets.insert(
            Image::from_fn(2, 2, |x, y| match (x, y) {
                (0, 0) => Color::rgb(255, 0, 0),
                (1, 0) => Color::rgb(0, 255, 0),
                (0, 1) => Color::rgb(0, 0, 255),
                _ => Color::WHITE,
            })
            .unwrap(),
        );
        let white = assets.insert(Image::from_fn(1, 1, |_, _| Color::WHITE).unwrap());
        let translucent = assets.insert(
            Image::from_fn(2, 1, |x, _| {
                if x == 0 {
                    Color::rgba(139, 67, 211, 91)
                } else {
                    Color::rgba(233, 125, 22, 1)
                }
            })
            .unwrap(),
        );
        let tolerance = std::env::var("TITAN_GPU_TOLERANCE")
            .map(|s| s.parse::<u8>().expect("TITAN_GPU_TOLERANCE must be a u8"))
            .unwrap_or(2);
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let mut renderer = GpuRenderer::new(device.clone(), queue.clone(), format);
            let mut empty_encoder = device.create_command_encoder(&Default::default());
            let dummy = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            assert_eq!(
                renderer.render(&mut empty_encoder, &dummy.create_view(&Default::default())),
                Err(GpuError::NotPrepared)
            );
            let mut opaque = RenderFrame::new(4, 4, Color::rgb(11, 27, 39));
            opaque.push(SpriteDraw::new(checker, 0, 0).with_pixel_scale(2));
            renderer.prepare(&opaque, &assets).unwrap();
            let exact = SoftwareRenderer::render(&opaque, &assets).unwrap();
            let actual = pixels(&device, &queue, &renderer, 4, 4, format);
            compare(&actual, exact.pixels(), 0);
            // Presentation scales the already-rasterized logical image with nearest sampling.
            let scaled = pixels(&device, &queue, &renderer, 8, 8, format);
            for y in 0..8 {
                for x in 0..8 {
                    let actual_offset = (y * 8 + x) * 4;
                    let reference_offset = ((y / 2) * 4 + x / 2) * 4;
                    assert_eq!(
                        &scaled[actual_offset..actual_offset + 4],
                        &exact.pixels()[reference_offset..reference_offset + 4]
                    );
                }
            }
            let mut frame = RenderFrame::new(7, 5, Color::rgb(13, 29, 47));
            frame.push(
                SpriteDraw::new(white, 0, 0)
                    .with_tint(Color::rgba(12, 178, 245, 128))
                    .with_layer(5),
            );
            frame.push(
                SpriteDraw::new(checker, -1, -1)
                    .with_pixel_scale(2)
                    .with_layer(-2),
            );
            frame.push(
                SpriteDraw::new(translucent, 3, 1)
                    .with_pixel_scale(2)
                    .with_order(10),
            );
            frame.push(SpriteDraw::new(white, 6, 4).with_tint(Color::rgb(100, 0, 0)));
            frame.push(SpriteDraw::new(white, 6, 4).with_tint(Color::rgb(0, 100, 0)));
            frame.push(SpriteDraw::new(white, 99, 99));
            renderer.prepare(&frame, &assets).unwrap();
            let reference = SoftwareRenderer::render(&frame, &assets).unwrap();
            let actual = pixels(&device, &queue, &renderer, 7, 5, format);
            compare(&actual, reference.pixels(), tolerance);
            assert_eq!(&actual[(4 * 7 + 6) * 4..(4 * 7 + 7) * 4], &[0, 100, 0, 255]);
            let mut transparent = RenderFrame::new(3, 2, Color::TRANSPARENT);
            transparent.push(SpriteDraw::new(translucent, 0, 0));
            transparent
                .push(SpriteDraw::new(white, 0, 1).with_tint(Color::rgba(91, 132, 197, 100)));
            renderer.prepare(&transparent, &assets).unwrap();
            compare(
                &pixels(&device, &queue, &renderer, 3, 2, format),
                SoftwareRenderer::render(&transparent, &assets)
                    .unwrap()
                    .pixels(),
                tolerance,
            );
            // Reusing numeric ImageIds from a different world must not reuse stale textures.
            let mut other = ImageAssets::new();
            let replacement =
                other.insert(Image::from_fn(1, 1, |_, _| Color::rgb(17, 31, 59)).unwrap());
            let mut replaced = RenderFrame::new(1, 1, Color::BLACK);
            replaced.push(SpriteDraw::new(replacement, 0, 0));
            renderer.prepare(&replaced, &other).unwrap();
            compare(
                &pixels(&device, &queue, &renderer, 1, 1, format),
                &[17, 31, 59, 255],
                0,
            );
            assert_eq!(
                renderer.prepare(&RenderFrame::new(0, 1, Color::BLACK), &other),
                Err(GpuError::InvalidDimensions)
            );
            let mut missing = RenderFrame::new(1, 1, Color::BLACK);
            missing.push(SpriteDraw::new(white, 0, 0));
            assert_eq!(
                renderer.prepare(&missing, &other),
                Err(GpuError::MissingImage(white))
            );
            compare(
                &pixels(&device, &queue, &renderer, 1, 1, format),
                &[17, 31, 59, 255],
                0,
            );
        }
    });
}

// Exercise the same renderer-neutral game extraction used by both real players.
use titan_rpg as game;

#[test]
#[ignore = "requires a native GPU adapter; exercised manually or in GPU-enabled CI"]
fn completed_rpg_replay_matches_software_capture() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("GPU adapter required for opt-in test");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .unwrap();
        let images = game::assets::decode_images(
            include_bytes!("../../../assets/player.png"),
            include_bytes!("../../../assets/tree.png"),
        )
        .expect("packaged RPG PNGs");
        let mut app = game::build_game_with_images(images);
        game::replay(&mut app, &game::recorded_walk());
        let state: serde_json::Value = serde_json::from_str(&game::status(&app)).unwrap();
        assert_eq!(state["shrine_active"], true);
        assert_eq!(state["collected_shards"], 3);
        for journal_open in [false, true] {
            game::journal::set_open(app.world_mut(), journal_open);
            app.refresh_extracted();
            let frame = app
                .extracted::<RenderFrame>()
                .expect("RPG extracted render frame");
            let assets = app.world().resource::<ImageAssets>().unwrap();
            let reference = SoftwareRenderer::render(frame, assets).unwrap();
            let reference_checksum = game::image_checksum(&reference);
            if !journal_open {
                assert_eq!(reference_checksum, 0xf7a298f62ad75c1c);
            } else {
                assert_ne!(reference_checksum, 0xf7a298f62ad75c1c);
            }
            let tolerance = std::env::var("TITAN_GPU_TOLERANCE")
                .map(|s| s.parse::<u8>().expect("TITAN_GPU_TOLERANCE must be a u8"))
                .unwrap_or(2);
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
                    frame.width(),
                    frame.height(),
                    format,
                );
                compare(&actual, reference.pixels(), tolerance);
                let image = Image::new(frame.width(), frame.height(), actual).unwrap();
                eprintln!(
                    "RPG {format:?}: GPU checksum {:016x}, software checksum {reference_checksum:016x}, tolerance {tolerance}",
                    game::image_checksum(&image)
                );
                if tolerance == 0 {
                    assert_eq!(game::image_checksum(&image), reference_checksum);
                }
            }
        }
    });
}
