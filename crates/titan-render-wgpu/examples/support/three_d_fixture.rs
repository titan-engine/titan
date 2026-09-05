//! Shared native/browser hardware evidence. Expected images contain only declared
//! probe regions (transparent elsewhere); no portable edge pixel equality claim.
use serde::Serialize;
use titan::render::three_d::*;
use titan_render_wgpu::{Gpu3dError, GpuRenderer3d, wgpu};

/// Fail before GPU resource creation when the chosen backend cannot supply the
/// exact color, depth and readback usages exercised by this fixture.
pub fn validate_adapter(adapter: &wgpu::Adapter) -> Result<Vec<String>, String> {
    let mut details = Vec::new();
    for (format, required) in [
        (
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        ),
        (
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        ),
        (
            wgpu::TextureFormat::Depth24Plus,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ),
    ] {
        let features = adapter.get_texture_format_features(format);
        if !features.allowed_usages.contains(required) {
            return Err(format!(
                "{format:?} requires {required:?}; backend supports {:?}",
                features.allowed_usages
            ));
        }
        details.push(format!("{format:?}: {features:?}"));
    }
    Ok(details)
}

pub const TOLERANCE: u8 = 2;
const CLEAR: BaseColor = BaseColor::rgb(13, 29, 47);
const RED: BaseColor = BaseColor::rgb(201, 43, 71);
const GREEN: BaseColor = BaseColor::rgb(31, 193, 89);

#[derive(Serialize)]
pub struct Probe {
    pub name: String,
    pub rect: [u32; 4],
    pub expected: [u8; 4],
    pub maximum_error: u8,
    pub passed: bool,
}
#[derive(Serialize)]
pub struct ImageEvidence {
    pub name: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub actual: Vec<u8>,
    pub expected: Vec<u8>,
    pub difference: Vec<u8>,
    pub probes: Vec<Probe>,
}
#[derive(Serialize)]
pub struct Evidence {
    pub tolerance: u8,
    pub edge_policy: &'static str,
    pub lifecycle_checks: Vec<String>,
    pub images: Vec<ImageEvidence>,
    pub capture_responses: Vec<titan_protocol::ResponseEnvelope>,
    pub passed: bool,
}
struct Case {
    name: &'static str,
    frame: RenderFrame3d,
    probes: Vec<Probe>,
}
fn rgba(color: BaseColor) -> [u8; 4] {
    [color.red, color.green, color.blue, 255]
}
fn probe(name: &str, rect: [u32; 4], color: BaseColor) -> Probe {
    Probe {
        name: name.into(),
        rect,
        expected: rgba(color),
        maximum_error: 0,
        passed: false,
    }
}
fn quad(positions: Vec<Vec3>, normal: Vec3, reversed: bool) -> Mesh {
    Mesh::new(
        positions,
        vec![normal; 4],
        if reversed {
            vec![0, 2, 1, 0, 3, 2]
        } else {
            vec![0, 1, 2, 0, 2, 3]
        },
    )
    .unwrap()
}
fn plane(normal: Vec3, reversed: bool) -> Mesh {
    quad(
        vec![
            Vec3::new(-1., -1., 0.),
            Vec3::new(1., -1., 0.),
            Vec3::new(1., 1., 0.),
            Vec3::new(-1., 1., 0.),
        ],
        normal,
        reversed,
    )
}
fn transform(x: f32, z: f32) -> Transform3d {
    Transform3d::new(Vec3::new(x, 0., z), Quaternion::IDENTITY, Vec3::ONE).unwrap()
}
fn draw(mesh: MeshHandle, transform: Transform3d, color: BaseColor, order: u64) -> Draw3d {
    Draw3d {
        mesh,
        transform,
        color,
        order,
    }
}
fn frame(assets: &MeshAssets, draws: Vec<Draw3d>, light: Lighting3d) -> RenderFrame3d {
    RenderFrame3d::new(
        PerspectiveCamera::new(
            Vec3::ZERO,
            Quaternion::IDENTITY,
            std::f32::consts::FRAC_PI_2,
            1.,
            1.,
            10.,
        )
        .unwrap(),
        light,
        assets,
        draws,
        Frame3dLimits::default(),
    )
    .unwrap()
}
fn cases() -> Vec<Case> {
    let mut assets = MeshAssets::new();
    let front = assets.insert(plane(Vec3::new(0., 0., 1.), false)).unwrap();
    let back = assets.insert(plane(Vec3::new(0., 0., 1.), true)).unwrap();
    let light = Lighting3d::new(Vec3::new(0., 0., 1.), 1., 0.).unwrap();
    let mut cases = Vec::new();
    let mut add = |name, draws, probes| {
        cases.push(Case {
            name,
            frame: frame(&assets, draws, light),
            probes,
        })
    };
    add(
        "perspective-near",
        vec![draw(front, transform(0., -2.), RED, 0)],
        vec![
            probe("near interior", [18, 28, 22, 36], RED),
            probe("outside near silhouette", [10, 28, 14, 36], CLEAR),
        ],
    );
    add(
        "perspective-far",
        vec![draw(front, transform(0., -4.), RED, 0)],
        vec![
            probe("far interior", [28, 28, 36, 36], RED),
            probe("perspective shrinks", [18, 28, 22, 36], CLEAR),
        ],
    );
    add(
        "clockwise-culled",
        vec![draw(back, transform(0., -2.), RED, 0)],
        vec![probe("backface rejected", [20, 20, 44, 44], CLEAR)],
    );
    for (name, x, z) in [
        ("before-near", 0., -0.5),
        ("beyond-far", 0., -12.),
        ("outside-side", 8., -2.),
    ] {
        add(
            name,
            vec![draw(front, transform(x, z), RED, 0)],
            vec![probe("fully clipped", [4, 4, 60, 60], CLEAR)],
        );
    }
    add(
        "side-crossing",
        vec![draw(front, transform(2., -2.), RED, 0)],
        vec![
            probe("visible clipped portion", [52, 22, 60, 42], RED),
            probe("outside projected polygon", [36, 22, 44, 42], CLEAR),
        ],
    );
    for (name, near_order, far_order) in [("depth-near-first", 0, 1), ("depth-far-first", 1, 0)] {
        add(
            name,
            vec![
                draw(front, transform(0., -2.), RED, near_order),
                draw(front, transform(0., -3.), GREEN, far_order),
            ],
            vec![probe("near wins", [24, 24, 40, 40], RED)],
        );
    }
    for (name, reverse) in [
        ("equal-depth-input-forward", false),
        ("equal-depth-input-reversed", true),
    ] {
        let mut draws = vec![
            draw(front, transform(0., -2.), RED, 3),
            draw(front, transform(0., -2.), GREEN, 9),
        ];
        if reverse {
            draws.reverse();
        }
        add(
            name,
            draws,
            vec![probe(
                "stable lower order wins strict less",
                [24, 24, 40, 40],
                RED,
            )],
        );
    }
    let near = assets
        .insert(quad(
            vec![
                Vec3::new(-1., -1., -0.5),
                Vec3::new(1., -1., -2.),
                Vec3::new(1., 1., -2.),
                Vec3::new(-1., 1., -0.5),
            ],
            Vec3::new(0., 0., 1.),
            false,
        ))
        .unwrap();
    cases.push(Case {
        name: "near-crossing",
        frame: frame(
            &assets,
            vec![draw(near, Transform3d::identity(), RED, 0)],
            light,
        ),
        probes: vec![
            probe("near clipped portion", [8, 28, 16, 36], CLEAR),
            probe("near retained portion", [28, 28, 40, 36], RED),
        ],
    });
    let far = assets
        .insert(quad(
            vec![
                Vec3::new(-8., -8., -8.),
                Vec3::new(8., -8., -12.),
                Vec3::new(8., 8., -12.),
                Vec3::new(-8., 8., -8.),
            ],
            Vec3::new(0., 0., 1.),
            false,
        ))
        .unwrap();
    cases.push(Case {
        name: "far-crossing",
        frame: frame(
            &assets,
            vec![draw(far, Transform3d::identity(), RED, 0)],
            light,
        ),
        probes: vec![
            probe("far retained portion", [12, 28, 24, 36], RED),
            probe("far clipped portion", [40, 28, 48, 36], CLEAR),
        ],
    });
    // Normal (1,0,1) with scale (2,1,0.5), then a +90-degree Z rotation:
    // inverse transpose gives (0,0.5,2)/sqrt(4.25). Against light (0,1,2)/sqrt(5),
    // N dot L = 4.5/sqrt(21.25). Omitting rotation, using the model matrix, or
    // omitting normalization all change the expected intensity substantially.
    let tilted_normal = assets.insert(plane(Vec3::new(1., 0., 1.), false)).unwrap();
    let model = Transform3d::new(
        Vec3::new(0., 0., -4.),
        Quaternion::new(0., 0., 1., 1.).unwrap(),
        Vec3::new(2., 1., 0.5),
    )
    .unwrap();
    let normal_light = Lighting3d::new(Vec3::new(0., 1., 2.), 0.1, 0.7).unwrap();
    let intensity = 0.1 + 0.7 * (4.5f32 / (21.25f32).sqrt());
    let encoded = RED.linear().map(|c| {
        let c = c * intensity;
        let s = if c <= 0.0031308 {
            12.92 * c
        } else {
            1.055 * c.powf(1. / 2.4) - 0.055
        };
        (s * 255.).round() as u8
    });
    cases.push(Case {
        name: "rotated-nonuniform-normal-linear-lighting",
        frame: frame(
            &assets,
            vec![draw(tilted_normal, model, RED, 0)],
            normal_light,
        ),
        probes: vec![probe(
            "inverse transpose and one sRGB encoding",
            [28, 24, 36, 40],
            BaseColor::rgb(encoded[0], encoded[1], encoded[2]),
        )],
    });
    let camera_rotation = Quaternion::new(0., 1., 0., 1.).unwrap();
    let camera = PerspectiveCamera::new(
        Vec3::new(3., 1., 5.),
        camera_rotation,
        std::f32::consts::FRAC_PI_2,
        1.,
        1.,
        10.,
    )
    .unwrap();
    let object = Transform3d::new(Vec3::new(1., 1., 5.), camera_rotation, Vec3::ONE).unwrap();
    cases.push(Case {
        name: "camera-translated-rotated",
        frame: RenderFrame3d::new(
            camera,
            light,
            &assets,
            [draw(front, object, GREEN, 0)],
            Frame3dLimits::default(),
        )
        .unwrap(),
        probes: vec![
            probe("view inverse pose", [20, 24, 44, 40], GREEN),
            probe("camera silhouette", [8, 24, 12, 40], CLEAR),
        ],
    });
    cases
}

async fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &GpuRenderer3d,
) -> Result<Vec<u8>, String> {
    let (width, height) = renderer.size();
    let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("3D evidence readback"),
        size: u64::from(row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.render(&mut encoder).map_err(|e| e.to_string())?;
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
    let submission = queue.submit([encoder.finish()]);
    let (sender, mut receiver) = futures_channel::oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    #[cfg(not(target_arch = "wasm32"))]
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .map_err(|e| e.to_string())?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = submission;
        // WebGL fences cannot complete until control returns to the browser.
        // WebGPU polls itself, while wgpu-core WebGL needs explicit polling.
        let deadline = js_sys::Date::now() + 10_000.0;
        loop {
            device
                .poll(wgpu::PollType::Poll)
                .map_err(|e| e.to_string())?;
            if let Some(result) = receiver.try_recv().map_err(|e| e.to_string())? {
                result.map_err(|e| e.to_string())?;
                break;
            }
            if js_sys::Date::now() >= deadline {
                return Err("GPU buffer mapping exceeded 10 seconds".into());
            }
            let promise = js_sys::Promise::new(&mut |resolve, reject| {
                if let Err(error) = web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 1)
                {
                    let _ = reject.call1(&wasm_bindgen::JsValue::NULL, &error);
                }
            });
            wasm_bindgen_futures::JsFuture::from(promise)
                .await
                .map_err(|e| format!("browser readback timer: {e:?}"))?;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    receiver
        .try_recv()
        .map_err(|e| e.to_string())?
        .ok_or("GPU buffer mapping callback did not complete after poll")?
        .map_err(|e| e.to_string())?;
    let mapped = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| e.to_string())?;
    let pixels = mapped
        .chunks(row as usize)
        .flat_map(|row| row[..width as usize * 4].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}
// Timers drive completion independently of requestAnimationFrame and simulation ticks.
async fn finish_owned(job: &mut titan_render_wgpu::OwnedGpuCapture) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    let start = std::time::Instant::now();
    #[cfg(target_arch = "wasm32")]
    let start = web_sys::window().unwrap().performance().unwrap().now();
    loop {
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed = start.elapsed();
        #[cfg(target_arch = "wasm32")]
        let elapsed = std::time::Duration::from_secs_f64(
            (web_sys::window().unwrap().performance().unwrap().now() - start) / 1000.0,
        );
        if let Some(image) = job.poll(elapsed).map_err(|e| e.to_string())? {
            return Ok(image.pixels().to_vec());
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(std::time::Duration::from_millis(1));
        #[cfg(target_arch = "wasm32")]
        {
            let promise = js_sys::Promise::new(&mut |resolve, reject| {
                if let Err(error) = web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 1)
                {
                    let _ = reject.call1(&wasm_bindgen::JsValue::NULL, &error);
                }
            });
            wasm_bindgen_futures::JsFuture::from(promise)
                .await
                .map_err(|_| "capture timer failed")?;
        }
    }
}

fn check_image(
    name: &str,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    actual: Vec<u8>,
    mut probes: Vec<Probe>,
) -> ImageEvidence {
    let mut expected = vec![0; actual.len()];
    let mut difference = vec![0; actual.len()];
    for probe in &mut probes {
        let [x0, y0, x1, y1] = probe.rect;
        for y in y0..y1 {
            for x in x0..x1 {
                let start = ((y * width + x) * 4) as usize;
                expected[start..start + 4].copy_from_slice(&probe.expected);
                for c in 0..4 {
                    let error = actual[start + c].abs_diff(probe.expected[c]);
                    probe.maximum_error = probe.maximum_error.max(error);
                    difference[start + c] = error;
                }
                difference[start + 3] = 255;
            }
        }
        probe.passed = probe.maximum_error <= TOLERANCE;
    }
    ImageEvidence {
        name: name.into(),
        format: format!("{format:?}"),
        width,
        height,
        actual,
        expected,
        difference,
        probes,
    }
}

pub async fn run(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Evidence, String> {
    let mut evidence = Evidence {
        tolerance: TOLERANCE,
        edge_policy: "Only named interior rectangles are compared; transparent expected/difference pixels are untested. Rasterized edges have no equality requirement.",
        lifecycle_checks: Vec::new(),
        images: Vec::new(),
        capture_responses: Vec::new(),
        passed: true,
    };
    // Host dispatch captures only a CPU snapshot and producer in a Send mailbox.
    // GPU handles stay on the browser thread, outside the App borrow.
    let mailbox = std::sync::Arc::new(std::sync::Mutex::new(None));
    let sink = mailbox.clone();
    let mut app = titan::App::new();
    app.world_mut().insert_resource(cases().remove(1).frame);
    app.advance_fixed(7);
    let mut inspector = titan::inspection::Inspector::new(
        titan::inspection::InspectionConfig::controlled("gpu-fixture", "owned-capture-fixture"),
    );
    inspector.register_async_capture_handler(64, 64, move |app, identity, completion| {
        let frame = app.world().resource::<RenderFrame3d>().unwrap().clone();
        *sink.lock().unwrap() = Some((frame, identity, completion));
        Ok(())
    });
    #[cfg(not(target_arch = "wasm32"))]
    let accepted_at = std::time::Instant::now();
    #[cfg(target_arch = "wasm32")]
    let response_promise = titan::inspection::response_promise(inspector.capture_timeout(), || {
        inspector.dispatch(
            &mut app,
            &titan_protocol::RequestEnvelope::new("gpu-owned-1", titan_protocol::Request::Capture),
        )
    });
    #[cfg(not(target_arch = "wasm32"))]
    let mut pending = match inspector.dispatch(
        &mut app,
        &titan_protocol::RequestEnvelope::new("gpu-owned-1", titan_protocol::Request::Capture),
    ) {
        titan::inspection::Dispatch::Pending(pending) => pending,
        _ => return Err("GPU fixture capture was not deferred".into()),
    };
    let (frozen, identity, completion) = mailbox.lock().unwrap().take().unwrap();
    let mut protocol_job = titan_render_wgpu::OwnedGpuCapture::three_d(
        device.clone(),
        queue.clone(),
        frozen,
        identity.width,
        identity.height,
        CLEAR,
    )
    .map_err(|e| e.to_string())?;
    // Advance after acceptance to prove metadata/image stay tied to the accepted tick.
    app.world_mut().insert_resource(frame(
        &MeshAssets::new(),
        vec![],
        Lighting3d::new(Vec3::ONE, 1., 0.).unwrap(),
    ));
    app.advance_fixed(3);
    let pixels = finish_owned(&mut protocol_job).await?;
    let image = titan::render::Image::new(identity.width, identity.height, pixels.clone()).unwrap();
    completion.complete(titan_diagnostics::png_capture(&image));
    #[cfg(not(target_arch = "wasm32"))]
    let elapsed = accepted_at.elapsed();
    #[cfg(not(target_arch = "wasm32"))]
    let response = pending
        .poll(elapsed)
        .ok_or("GPU protocol completion missing")?;
    #[cfg(target_arch = "wasm32")]
    let response: titan_protocol::ResponseEnvelope = {
        let json = wasm_bindgen_futures::JsFuture::from(response_promise)
            .await
            .map_err(|error| format!("capture Promise failed: {error:?}"))?
            .as_string()
            .ok_or("capture Promise did not return JSON")?;
        serde_json::from_str(&json).map_err(|error| error.to_string())?
    };
    match &response.outcome {
        titan_protocol::ResponseOutcome::Success {
            response: titan_protocol::Response::Capture(capture),
        } if capture.identity == identity
            && identity.observed_frame == 7
            && response.observed_frame == identity.observed_frame
            && capture.artifact.starts_with("data:image/png;base64,") => {}
        _ => return Err("GPU protocol provenance or artifact mismatch".into()),
    }
    evidence.capture_responses.push(response);
    evidence.images.push(check_image(
        "protocol-owned-frame",
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        64,
        pixels,
        cases().remove(1).probes,
    ));
    #[cfg(not(target_arch = "wasm32"))]
    drop(pending);
    let mut canceled_response = match inspector.dispatch(
        &mut app,
        &titan_protocol::RequestEnvelope::new("gpu-cancel", titan_protocol::Request::Capture),
    ) {
        titan::inspection::Dispatch::Pending(pending) => pending,
        _ => return Err("GPU cancellation fixture was not deferred".into()),
    };
    let (frozen, identity, completion) = mailbox.lock().unwrap().take().unwrap();
    let canceled_job = titan_render_wgpu::OwnedGpuCapture::three_d(
        device.clone(),
        queue.clone(),
        frozen,
        identity.width,
        identity.height,
        CLEAR,
    )
    .map_err(|e| e.to_string())?;
    canceled_response
        .cancel()
        .ok_or("GPU cancellation response missing")?;
    drop(canceled_response);
    let blocked = inspector.dispatch(
        &mut app,
        &titan_protocol::RequestEnvelope::new(
            "gpu-cancel-overload",
            titan_protocol::Request::Capture,
        ),
    );
    if !matches!(
        blocked,
        titan::inspection::Dispatch::Ready(titan_protocol::ResponseEnvelope {
            outcome: titan_protocol::ResponseOutcome::Failure { .. },
            ..
        })
    ) {
        return Err("canceled GPU producer released admission before retirement".into());
    }
    canceled_job.retire(move || drop(completion));
    // Freeze a declared source frame, then replace the local source before mapping.
    // The submitted image must still be the original scene, without another tick.
    let case = cases().remove(1);
    let mut source = case.frame;
    let mut owned = titan_render_wgpu::OwnedGpuCapture::three_d(
        device.clone(),
        queue.clone(),
        source.clone(),
        64,
        64,
        CLEAR,
    )
    .map_err(|e| e.to_string())?;
    source = frame(
        &MeshAssets::new(),
        vec![],
        Lighting3d::new(Vec3::ONE, 1., 0.).unwrap(),
    );
    let actual = finish_owned(&mut owned).await?;
    // Subsequent completed GPU work has driven retirement, including admission cleanup.
    let admitted = inspector.dispatch(
        &mut app,
        &titan_protocol::RequestEnvelope::new(
            "gpu-after-retirement",
            titan_protocol::Request::Capture,
        ),
    );
    if !matches!(admitted, titan::inspection::Dispatch::Pending(_)) {
        return Err("GPU retirement did not release admission".into());
    }
    drop(admitted);
    drop(mailbox.lock().unwrap().take());

    evidence.images.push(check_image(
        "owned-frozen-frame",
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        64,
        actual,
        case.probes,
    ));
    if !matches!(
        owned.poll(std::time::Duration::ZERO),
        Err(titan_render_wgpu::GpuCaptureError::Finished)
    ) {
        return Err("capture completed twice".into());
    }
    let mut canceled = titan_render_wgpu::OwnedGpuCapture::three_d(
        device.clone(),
        queue.clone(),
        source.clone(),
        65,
        31,
        GREEN,
    )
    .map_err(|e| e.to_string())?;
    canceled.cancel();
    if !matches!(
        canceled.poll(std::time::Duration::ZERO),
        Err(titan_render_wgpu::GpuCaptureError::Finished)
    ) {
        return Err("canceled capture produced output".into());
    }
    let mut timeout = titan_render_wgpu::OwnedGpuCapture::three_d(
        device.clone(),
        queue.clone(),
        source.clone(),
        65,
        31,
        GREEN,
    )
    .map_err(|e| e.to_string())?;
    if !matches!(
        timeout.poll(titan_render_wgpu::MAX_CAPTURE_WAIT),
        Err(titan_render_wgpu::GpuCaptureError::Timeout)
    ) {
        return Err("capture deadline not enforced".into());
    }
    let retired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let notify = retired.clone();
    canceled.retire(move || notify.store(true, std::sync::atomic::Ordering::Release));
    // A new padded/changed-size capture polls late callbacks from canceled jobs too.
    let mut resized = titan_render_wgpu::OwnedGpuCapture::three_d(
        device.clone(),
        queue.clone(),
        source,
        65,
        31,
        GREEN,
    )
    .map_err(|e| e.to_string())?;
    let actual = finish_owned(&mut resized).await?;
    evidence.images.push(check_image(
        "owned-new-size-after-cancel",
        wgpu::TextureFormat::Rgba8Unorm,
        65,
        31,
        actual,
        vec![probe(
            "padded rows retain dimensions",
            [0, 0, 65, 31],
            GREEN,
        )],
    ));
    if !retired.load(std::sync::atomic::Ordering::Acquire) {
        device
            .poll(wgpu::PollType::Poll)
            .map_err(|_| "retirement polling failed")?;
    }
    if !retired.load(std::sync::atomic::Ordering::Acquire) {
        return Err("canceled submission did not retire before subsequent readback".into());
    }
    evidence.lifecycle_checks.push("owned fresh submission: frozen source, one completion, cancel, deadline, queue retirement callback, late-map cleanup, changed dimensions, paused timer polling".into());
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut renderer =
            GpuRenderer3d::new(device.clone(), 64, 64, format).map_err(|e| e.to_string())?;
        let mut encoder = device.create_command_encoder(&Default::default());
        if renderer.render(&mut encoder) != Err(Gpu3dError::NotPrepared) {
            return Err("new renderer accepted render before prepare".into());
        }
        for case in cases() {
            renderer
                .prepare(&case.frame, CLEAR)
                .map_err(|e| e.to_string())?;
            let actual = readback(device, queue, &renderer).await?;
            evidence
                .images
                .push(check_image(case.name, format, 64, 64, actual, case.probes));
        }
        renderer.resize(48, 32).map_err(|e| e.to_string())?;
        let assets = MeshAssets::new();
        let empty = frame(&assets, vec![], Lighting3d::new(Vec3::ONE, 1., 0.).unwrap());
        renderer.prepare(&empty, GREEN).map_err(|e| e.to_string())?;
        let actual = readback(device, queue, &renderer).await?;
        evidence.images.push(check_image(
            "resize-empty-clear",
            format,
            48,
            32,
            actual,
            vec![probe("entire resized target clears", [0, 0, 48, 32], GREEN)],
        ));
        renderer.resize(64, 64).map_err(|e| e.to_string())?;
        let case = cases().remove(1);
        renderer
            .prepare(&case.frame, CLEAR)
            .map_err(|e| e.to_string())?;
        let actual = readback(device, queue, &renderer).await?;
        evidence.images.push(check_image(
            "resize-depth-clear",
            format,
            64,
            64,
            actual,
            case.probes,
        ));
        if renderer.resize(0, 32).is_ok() {
            return Err("zero size accepted".into());
        }
        if renderer.resize(u32::MAX, 32).is_ok() {
            return Err("unbounded size accepted".into());
        }
        // The CPU frame permits finite geometry whose vertex-stage products would
        // overflow f32; preparation must reject it and invalidate the old frame.
        renderer
            .prepare(&case.frame, CLEAR)
            .map_err(|e| e.to_string())?;
        let mut overflow_assets = MeshAssets::new();
        let huge = overflow_assets
            .insert(quad(
                vec![
                    Vec3::new(-f32::MAX, -1., 0.),
                    Vec3::new(f32::MAX, -1., 0.),
                    Vec3::new(f32::MAX, 1., 0.),
                    Vec3::new(-f32::MAX, 1., 0.),
                ],
                Vec3::new(0., 0., 1.),
                false,
            ))
            .unwrap();
        let huge_model = Transform3d::new(
            Vec3::new(0., 0., -4.),
            Quaternion::IDENTITY,
            Vec3::new(2., 1., 1.),
        )
        .unwrap();
        let invalid = frame(
            &overflow_assets,
            vec![draw(huge, huge_model, RED, 0)],
            Lighting3d::new(Vec3::ONE, 1., 0.).unwrap(),
        );
        if renderer.prepare(&invalid, CLEAR) != Err(Gpu3dError::Math(MathError::Unrepresentable)) {
            return Err("overflowing vertex-stage preparation was not rejected".into());
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        if renderer.render(&mut encoder) != Err(Gpu3dError::NotPrepared) {
            return Err("failed preparation retained a usable stale frame".into());
        }
        renderer.prepare(&empty, GREEN).map_err(|e| e.to_string())?;
        let actual = readback(device, queue, &renderer).await?;
        evidence.images.push(check_image(
            "recovery-after-invalid-prepare",
            format,
            64,
            64,
            actual,
            vec![probe("valid prepare recovers", [0, 0, 64, 64], GREEN)],
        ));
        evidence.lifecycle_checks.push(format!("{format:?}: render-before-prepare rejected; 64x64 -> 48x32 -> 64x64 color/depth clears; zero and unbounded resize rejected; overflowing prepare invalidates frame; valid preparation recovers"));
    }
    evidence.passed = evidence
        .images
        .iter()
        .flat_map(|image| &image.probes)
        .all(|probe| probe.passed);
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_probes_are_bounded_and_expected_colors_independent_of_readback() {
        let cases = cases();
        assert!(cases.len() >= 14);
        for case in cases {
            for probe in case.probes {
                let [x0, y0, x1, y1] = probe.rect;
                assert!(x0 < x1 && y0 < y1 && x1 <= 64 && y1 <= 64);
                assert_eq!(probe.expected[3], 255);
            }
        }
    }
}
