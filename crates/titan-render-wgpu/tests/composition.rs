//! Explicit native GPU checks for the shared player/capture composition path.
#![cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use titan::render::{Color, Image, ImageAssets, RenderFrame, SpriteDraw, three_d::*};
use titan_render_wgpu::{GpuSceneRenderer3d, wgpu};
fn pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &GpuSceneRenderer3d,
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
#[ignore = "requires native GPU"]
fn scene_overlay_color_conversion_resize_and_failed_preparation() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        eprintln!(
            "Composition adapter: {:?}; tolerance: 2/channel",
            adapter.get_info()
        );
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .unwrap();
        let directory = std::env::var_os("TITAN_COMPOSITION_EVIDENCE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("titan-composition-evidence"));
        std::fs::create_dir_all(&directory).unwrap();
        let mut cases = Vec::new();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let scene = RenderFrame3d::new(
            PerspectiveCamera::new(Vec3::ZERO, Quaternion::IDENTITY, 1.0, 1.0, 0.1, 10.0).unwrap(),
            Lighting3d::new(Vec3::new(0.0, 1.0, 0.0), 1.0, 0.0).unwrap(),
            &MeshAssets::default(),
            [],
            Frame3dLimits::default(),
        )
        .unwrap();
        let clear = BaseColor::rgb(31, 97, 163);
        let mut assets = ImageAssets::default();
        let image = assets.insert(Image::new(1, 1, vec![211, 73, 129, 128]).unwrap());
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ] {
            let mut renderer =
                GpuSceneRenderer3d::new(device.clone(), queue.clone(), 4, 2, format).unwrap();
            for (width, height) in [(4, 2), (8, 4)] {
                renderer.resize(width, height).unwrap();
                for alpha in [0, 128, 255] {
                    let mut overlay = RenderFrame::new(2, 1, Color::TRANSPARENT);
                    // A solid clear on the left would hide scene coverage; use a
                    // sprite for translucent/opaque left and transparent right.
                    let sprite = if alpha == 128 {
                        image
                    } else {
                        assets.insert(Image::new(1, 1, vec![211, 73, 129, alpha]).unwrap())
                    };
                    overlay.push(SpriteDraw::new(sprite, 0, 0));
                    renderer.prepare(&scene, clear, &overlay, &assets).unwrap();
                    let actual = pixels(&device, &queue, &renderer, width, height, format);
                    let foreground = BaseColor::rgb(211, 73, 129).linear();
                    let background = clear.linear();
                    let mut expected = [0u8; 4];
                    for channel in 0..3 {
                        let linear = background[channel] * (1.0 - f32::from(alpha) / 255.0)
                            + foreground[channel] * f32::from(alpha) / 255.0;
                        expected[channel] = ((if linear <= 0.0031308 {
                            linear * 12.92
                        } else {
                            1.055 * linear.powf(1.0 / 2.4) - 0.055
                        }) * 255.0)
                            .round() as u8;
                    }
                    expected[3] = 255;
                    let mut expected_pixels = Vec::with_capacity(actual.len());
                    for (index, pixel) in actual.as_chunks::<4>().0.iter().enumerate() {
                        let mut want = if (index as u32) % width < width / 2 {
                            expected
                        } else {
                            [31, 97, 163, 255]
                        };
                        if matches!(
                            format,
                            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
                        ) {
                            want.swap(0, 2);
                        }
                        expected_pixels.extend_from_slice(&want);
                        assert!(
                            pixel.iter().zip(want).all(|(a, e)| a.abs_diff(e) <= 2),
                            "{format:?}, alpha={alpha}, pixel={pixel:?}, expected={want:?}"
                        );
                    }
                    let name = format!("{format:?}-{width}x{height}-alpha{alpha}");
                    let difference: Vec<u8> = actual
                        .iter()
                        .zip(&expected_pixels)
                        .map(|(a, e)| a.abs_diff(*e))
                        .collect();
                    cases.push(serde_json::json!({"name":name,"width":width,"height":height,"maximum_error":difference.iter().max(),"passed":true}));
                    for (kind, mut bytes) in [
                        ("actual", actual),
                        ("expected", expected_pixels),
                        ("difference", difference),
                    ] {
                        for pixel in bytes.as_chunks_mut::<4>().0 {
                            if matches!(
                                format,
                                wgpu::TextureFormat::Bgra8Unorm
                                    | wgpu::TextureFormat::Bgra8UnormSrgb
                            ) {
                                pixel.swap(0, 2);
                            }
                            if kind == "difference" {
                                pixel[3] = 255;
                            }
                        }
                        titan_diagnostics::write_png(
                            &Image::new(width, height, bytes).unwrap(),
                            std::fs::File::create(directory.join(format!("{name}-{kind}.png")))
                                .unwrap(),
                        )
                        .unwrap();
                    }
                    eprintln!("{format:?} {width}x{height} alpha={alpha}: passed");
                }
            }
            assert!(renderer.resize(0, 4).is_err());
            assert_eq!(renderer.size(), (8, 4));
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 8,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            assert!(
                renderer
                    .render(&mut encoder, &target.create_view(&Default::default()))
                    .is_err()
            );
            renderer
                .prepare(
                    &scene,
                    clear,
                    &RenderFrame::new(2, 1, Color::TRANSPARENT),
                    &assets,
                )
                .unwrap();
            assert!(
                renderer
                    .prepare(
                        &scene,
                        clear,
                        &RenderFrame::new(0, 1, Color::TRANSPARENT),
                        &assets
                    )
                    .is_err()
            );
            assert!(
                renderer
                    .render(&mut encoder, &target.create_view(&Default::default()))
                    .is_err()
            );
        }
        assert!(scope.pop().await.is_none());
        std::fs::write(directory.join("native.json"),serde_json::to_vec_pretty(&serde_json::json!({"adapter":format!("{:?}",adapter.get_info()),"tolerance":2,"cases":cases,"lifecycle":["zero resize rejected, allocation retained, preparation invalidated","invalid UI preparation invalidates composition"],"passed":true})).unwrap()).unwrap();
        eprintln!("Composition artifacts: {}", directory.display());
    });
}
