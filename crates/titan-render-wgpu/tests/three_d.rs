//! Opt-in native evidence: cargo test -p titan-render-wgpu --test three_d -- --ignored
#![cfg(not(target_arch = "wasm32"))]
#[path = "../examples/support/three_d_fixture.rs"]
mod fixture;
use titan_render_wgpu::wgpu;

#[test]
#[ignore = "requires native GPU; browser uses the same scene/probe fixture"]
fn opaque_3d_projection_clipping_depth_lighting_and_resize() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("native GPU adapter required");
        let formats = fixture::validate_adapter(&adapter).expect("required GPU formats/usages");
        let info = adapter.get_info();
        eprintln!("3D evidence adapter: {info:?}");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .unwrap();
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let result = fixture::run(&device, &queue).await;
        assert!(validation.pop().await.is_none(), "GPU validation failed");
        let evidence = result.unwrap();
        let report = serde_json::json!({ "adapter": format!("{info:?}"), "formats": formats, "evidence": evidence });
        let directory = std::env::var_os("TITAN_3D_EVIDENCE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("titan-3d-evidence"));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("native.json"),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        for image in &evidence.images {
            for (kind, bytes) in [
                ("actual", &image.actual),
                ("expected", &image.expected),
                ("difference", &image.difference),
            ] {
                let image_data =
                    titan::render::Image::new(image.width, image.height, bytes.clone()).unwrap();
                titan_diagnostics::write_png(
                    &image_data,
                    std::fs::File::create(
                        directory.join(format!("{}-{}-{kind}.png", image.name, image.format)),
                    )
                    .unwrap(),
                )
                .unwrap();
            }
            for probe in &image.probes {
                eprintln!(
                    "{} {} {}: error {}, pass {}",
                    image.name, image.format, probe.name, probe.maximum_error, probe.passed
                );
            }
        }
        eprintln!("Evidence: {}", directory.display());
        assert!(
            evidence.passed,
            "3D GPU probes failed; inspect native.json and PNG triples"
        );
    });
}
