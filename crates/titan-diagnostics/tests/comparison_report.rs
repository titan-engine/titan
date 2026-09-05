#![cfg(not(target_arch = "wasm32"))]

use std::{fs, path::PathBuf};
use titan::render::Image;
use titan_diagnostics::{
    ComparisonError, ComparisonOptions, ComparisonReportError, ImageComparisonReport,
    write_comparison_report,
};

fn temporary_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "titan-comparison-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn decode_rgba(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let output = reader.next_frame(&mut bytes).unwrap();
    assert_eq!(output.color_type, png::ColorType::Rgba);
    bytes.truncate(output.buffer_size());
    (output.width, output.height, bytes)
}

#[test]
fn report_writes_sources_metrics_and_locatable_difference_channels() {
    let root = temporary_path("channels");
    let expected = Image::new(3, 1, vec![10, 20, 30, 255, 255, 0, 0, 0, 255, 0, 0, 0]).unwrap();
    let actual = Image::new(3, 1, vec![10, 20, 30, 255, 255, 0, 0, 255, 0, 0, 255, 0]).unwrap();

    let written =
        write_comparison_report(&root, &expected, &actual, ComparisonOptions::exact()).unwrap();
    let report: ImageComparisonReport =
        serde_json::from_slice(&fs::read(&written.manifest).unwrap()).unwrap();
    assert_eq!(report.options, ComparisonOptions::exact());
    assert_eq!(report.comparison.differing_pixels, 2);
    assert!(!report.comparison.passes);
    assert_eq!(report.artifacts.expected, "expected.png");
    assert_eq!(decode_rgba(&written.expected).2, expected.pixels());
    assert_eq!(decode_rgba(&written.actual).2, actual.pixels());

    let (width, height, difference) = decode_rgba(&written.difference);
    assert_eq!((width, height), (3, 1));
    assert_eq!(
        difference,
        vec![
            0, 0, 0, 255, // identical: black
            255, 255, 0, 255, // alpha change with visible effect: yellow
            0, 0, 255, 255, // invisible RGB change: blue
        ]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn identical_images_produce_a_black_passing_difference() {
    let root = temporary_path("identical");
    let image = Image::new(1, 1, vec![80, 90, 100, 110]).unwrap();
    let written =
        write_comparison_report(&root, &image, &image, ComparisonOptions::default()).unwrap();
    let report: ImageComparisonReport =
        serde_json::from_slice(&fs::read(&written.manifest).unwrap()).unwrap();
    assert!(report.comparison.exact);
    assert!(report.comparison.passes);
    assert_eq!(decode_rgba(&written.difference).2, vec![0, 0, 0, 255]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn comparison_validation_happens_before_creating_output() {
    let root = temporary_path("invalid");
    let one = Image::new(1, 1, vec![0; 4]).unwrap();
    let two = Image::new(2, 1, vec![0; 8]).unwrap();
    assert!(matches!(
        write_comparison_report(&root, &one, &two, ComparisonOptions::default()),
        Err(ComparisonReportError::Comparison(
            ComparisonError::DimensionsMismatch
        ))
    ));
    assert!(!root.exists());
    assert!(matches!(
        write_comparison_report(
            &root,
            &one,
            &one,
            ComparisonOptions {
                maximum_linear_rmse: f64::NAN,
                ..ComparisonOptions::default()
            }
        ),
        Err(ComparisonReportError::Comparison(
            ComparisonError::InvalidThresholds
        ))
    ));
    assert!(!root.exists());
}

#[test]
fn empty_images_and_output_failures_are_explicit() {
    let root = temporary_path("errors");
    let empty = Image::new(0, 0, vec![]).unwrap();
    assert!(matches!(
        write_comparison_report(&root, &empty, &empty, ComparisonOptions::exact()),
        Err(ComparisonReportError::EmptyImage)
    ));
    assert!(!root.exists());

    fs::write(&root, b"not a directory").unwrap();
    let image = Image::new(1, 1, vec![0; 4]).unwrap();
    let error =
        write_comparison_report(&root, &image, &image, ComparisonOptions::exact()).unwrap_err();
    assert!(matches!(error, ComparisonReportError::Io(_)));
    assert!(error.to_string().contains("comparison report I/O failed"));
    fs::remove_file(root).unwrap();
}
