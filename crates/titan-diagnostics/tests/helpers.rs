use titan::render::{Color, Image};
use titan_diagnostics::*;
use titan_protocol::{
    ErrorCode, InputValue, ProtocolError, Request, RequestEnvelope, Response, ResponseEnvelope,
};
fn pair(id: &str, success: bool) -> (RequestEnvelope, ResponseEnvelope) {
    let request = RequestEnvelope::new(
        id,
        Request::InjectInput {
            frame: 9,
            actions: [("right".into(), InputValue::Button(true))].into(),
        },
    );
    let response = if success {
        ResponseEnvelope::success(
            &request,
            "game",
            8,
            12,
            Response::Applied { applied_frame: 9 },
        )
    } else {
        ResponseEnvelope::failure(
            &request,
            "game",
            8,
            12,
            ProtocolError::new(ErrorCode::InvalidValue, "bad input"),
        )
    };
    (request, response)
}
#[test]
fn failure_policy_is_default_and_explicit_modes_are_honored() {
    let (_, ok) = pair("ok", true);
    let (_, failed) = pair("failed", false);
    assert!(!DiagnosticPolicy::default().should_capture(&ok));
    assert!(DiagnosticPolicy::default().should_capture(&failed));
    assert!(DiagnosticPolicy::Always.should_capture(&ok));
    assert!(!DiagnosticPolicy::Never.should_capture(&failed));
}
#[test]
fn history_evicts_oldest_and_distinguishes_rejected_input() {
    let mut history = RequestHistory::new(2, 4096);
    for (id, success) in [("first", true), ("second", false), ("third", true)] {
        let (request, response) = pair(id, success);
        assert!(history.record(&request, &response, 7).unwrap());
    }
    let snapshot = history.snapshot();
    assert_eq!(
        snapshot
            .requests
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(snapshot.dropped_entries, 1);
    assert_eq!(snapshot.accepted_inputs.len(), 1);
    assert_eq!(snapshot.accepted_inputs[0].request_id, "third");
    assert_eq!(snapshot.accepted_inputs[0].target_frame, 9);
    assert_eq!(history.snapshot(), snapshot);
}
#[test]
fn history_byte_limit_and_disabled_ring_are_bounded() {
    let (request, response) = pair("one", true);
    let mut history = RequestHistory::new(10, 1000);
    assert!(history.record(&request, &response, 0).unwrap());
    let size = history.serialized_bytes();
    let mut bounded = RequestHistory::new(10, size);
    assert!(bounded.record(&request, &response, 0).unwrap());
    assert!(bounded.record(&request, &response, 0).unwrap());
    assert_eq!(bounded.len(), 1);
    assert!(bounded.serialized_bytes() <= size);
    let mut tiny = RequestHistory::new(10, 1);
    assert!(!tiny.record(&request, &response, 0).unwrap());
    assert!(tiny.is_empty());
    assert_eq!(tiny.snapshot().dropped_entries, 1);
    let mut disabled = RequestHistory::new(0, usize::MAX);
    assert!(!disabled.record(&request, &response, 0).unwrap());
}
#[test]
fn bundles_round_trip_and_local_errors_do_not_invent_runtime_state() {
    let (request, response) = pair("failed", false);
    let bundle = DiagnosticBundle::new(request, response);
    assert_eq!(
        serde_json::from_str::<DiagnosticBundle>(&serde_json::to_string(&bundle).unwrap()).unwrap(),
        bundle
    );
    let local = DiagnosticBundle::local_failure(
        serde_json::json!({"code":"not_found","message":"no runtime"}),
    );
    assert!(local.request.is_none());
    assert!(local.response.is_none());
    assert!(local.local_error.is_some());
}
#[test]
fn api_summary_is_sorted_and_preserves_field_constraints() {
    let field = titan_protocol::FieldMetadata {
        type_name: "u32".into(),
        description: "grid coordinate".into(),
        writable: true,
        minimum: Some(0.),
        maximum: Some(19.),
        unit: Some("tile".into()),
    };
    let summary = ApiSummary::new(
        vec![
            ApiComponent {
                name: "Z".into(),
                fields: Default::default(),
            },
            ApiComponent {
                name: "A".into(),
                fields: [("x".into(), field.clone())].into(),
            },
        ],
        vec![titan_protocol::CommandMetadata {
            name: "spawn".into(),
            description: "Spawn a shard".into(),
            arguments: [("x".into(), field)].into(),
        }],
    );
    let text = summary.compact_text();
    assert!(text.find("component A").unwrap() < text.find("component Z").unwrap());
    assert!(text.contains("x: u32 [writable] (min=0, max=19, unit=tile, grid coordinate)"));
    assert!(text.contains("command spawn — Spawn a shard"));
}
#[test]
fn image_checks_distinguish_exact_perceptual_and_large_errors() {
    let reference = Image::from_fn(16, 16, |x, y| {
        Color::rgb((x * 8 + 20) as u8, (y * 8 + 20) as u8, 100)
    })
    .unwrap();
    let slight = Image::from_fn(16, 16, |x, y| {
        Color::rgb((x * 8 + 21) as u8, (y * 8 + 20) as u8, 100)
    })
    .unwrap();
    assert!(
        compare_images(&reference, &reference, ComparisonOptions::exact())
            .unwrap()
            .passes
    );
    let comparison = compare_images(&reference, &slight, ComparisonOptions::default()).unwrap();
    assert!(!comparison.exact);
    assert!(comparison.passes);
    assert_eq!(comparison.maximum_channel_error, 1);
    assert!(
        !compare_images(&reference, &slight, ComparisonOptions::exact())
            .unwrap()
            .passes
    );
    let inverted = Image::from_fn(16, 16, |x, y| {
        Color::rgb(255 - (x * 8 + 20) as u8, 255 - (y * 8 + 20) as u8, 155)
    })
    .unwrap();
    assert!(
        !compare_images(&reference, &inverted, ComparisonOptions::default())
            .unwrap()
            .passes
    );
}
#[test]
fn perceptual_comparison_handles_transparency_and_invalid_inputs() {
    let transparent_red = Image::from_fn(1, 1, |_, _| Color::rgba(255, 0, 0, 0)).unwrap();
    let transparent_blue = Image::from_fn(1, 1, |_, _| Color::rgba(0, 0, 255, 0)).unwrap();
    let invisible = compare_images(
        &transparent_red,
        &transparent_blue,
        ComparisonOptions::default(),
    )
    .unwrap();
    assert!(!invisible.exact);
    assert!(invisible.passes);
    assert_eq!(invisible.linear_rmse, 0.);
    let opaque = Image::from_fn(1, 1, |_, _| Color::BLACK).unwrap();
    assert!(
        !compare_images(&transparent_red, &opaque, ComparisonOptions::default())
            .unwrap()
            .passes
    );
    let empty = Image::new(0, 0, vec![]).unwrap();
    assert_eq!(
        compare_images(&empty, &opaque, ComparisonOptions::default()),
        Err(ComparisonError::DimensionsMismatch)
    );
    assert!(
        compare_images(&empty, &empty, ComparisonOptions::exact())
            .unwrap()
            .passes
    );
    assert_eq!(
        compare_images(
            &opaque,
            &opaque,
            ComparisonOptions {
                minimum_ssim: f64::NAN,
                ..Default::default()
            }
        ),
        Err(ComparisonError::InvalidThresholds)
    );
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn writer_creates_self_contained_png_and_manifest_and_attaches_only_failures() {
    let root = std::env::temp_dir().join(format!(
        "titan-bundle-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (request, mut response) = pair("../../untrusted-id", false);
    let mut bundle = DiagnosticBundle::new(request, response.clone());
    bundle.world_state = serde_json::json!({"entities":[]});
    bundle.api_summary = Some(ApiSummary::default());
    let image = Image::from_fn(2, 1, |x, _| {
        if x == 0 {
            Color::rgba(12, 34, 56, 78)
        } else {
            Color::WHITE
        }
    })
    .unwrap();
    let written = write_bundle(&root, &bundle, Some(&image)).unwrap();
    assert!(written.directory.starts_with(root.canonicalize().unwrap()));
    let saved: DiagnosticBundle =
        serde_json::from_slice(&std::fs::read(&written.manifest).unwrap()).unwrap();
    assert_eq!(saved.capture.as_ref().unwrap().artifact, "capture.png");
    assert!(written.directory.join("api.txt").exists());
    assert!(!written.directory.join("bundle.json.part").exists());
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(written.capture.unwrap()).unwrap(),
    ));
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut bytes).unwrap();
    assert_eq!(bytes, image.pixels());
    assert!(attach_failure_path(&mut response, &written.manifest));
    let (_, mut success) = pair("success", true);
    assert!(!attach_failure_path(&mut success, &written.manifest));
    let second = write_bundle(&root, &bundle, None).unwrap();
    assert_ne!(written.directory, second.directory);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&written.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&written.manifest)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
