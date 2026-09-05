use crate::{ComparisonError, ComparisonOptions, ImageComparison, compare_images, write_png};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use titan::render::Image;

static NEXT_REPORT: AtomicU64 = AtomicU64::new(0);
const MAX_REPORT_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonReportArtifacts {
    pub expected: String,
    pub actual: String,
    pub difference: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferenceVisualization {
    pub encoding: String,
    pub red: String,
    pub green: String,
    pub blue: String,
    pub alpha: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageComparisonReport {
    pub report_version: u32,
    pub width: u32,
    pub height: u32,
    pub options: ComparisonOptions,
    pub comparison: ImageComparison,
    pub artifacts: ComparisonReportArtifacts,
    pub difference_visualization: DifferenceVisualization,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrittenComparisonReport {
    pub directory: PathBuf,
    pub manifest: PathBuf,
    pub expected: PathBuf,
    pub actual: PathBuf,
    pub difference: PathBuf,
}

#[derive(Debug)]
pub enum ComparisonReportError {
    Comparison(ComparisonError),
    EmptyImage,
    TooLarge,
    Io(io::Error),
    Json(serde_json::Error),
    Png(png::EncodingError),
}

impl std::fmt::Display for ComparisonReportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Comparison(error) => write!(formatter, "image comparison failed: {error}"),
            Self::EmptyImage => write!(formatter, "comparison reports cannot encode empty images"),
            Self::TooLarge => write!(
                formatter,
                "comparison report image or encoded artifact exceeds 64 MiB limit"
            ),
            Self::Io(error) => write!(formatter, "comparison report I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "comparison report JSON failed: {error}"),
            Self::Png(error) => write!(formatter, "comparison report PNG failed: {error}"),
        }
    }
}

impl std::error::Error for ComparisonReportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Comparison(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Png(error) => Some(error),
            Self::EmptyImage | Self::TooLarge => None,
        }
    }
}

impl From<io::Error> for ComparisonReportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ComparisonReportError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<png::EncodingError> for ComparisonReportError {
    fn from(error: png::EncodingError) -> Self {
        Self::Png(error)
    }
}

/// Write a unique, self-contained offline comparison report below `root`.
///
/// The report contains exact copies of both inputs, a difference visualization,
/// and `report.json` with the unchanged [`compare_images`] metrics and options.
/// Each raw image and each encoded artifact is limited to 64 MiB. Failed writes
/// remove only the newly created report directory.
pub fn write_comparison_report(
    root: &Path,
    expected: &Image,
    actual: &Image,
    options: ComparisonOptions,
) -> Result<WrittenComparisonReport, ComparisonReportError> {
    let comparison =
        compare_images(expected, actual, options).map_err(ComparisonReportError::Comparison)?;
    if expected.width() == 0 || expected.height() == 0 {
        return Err(ComparisonReportError::EmptyImage);
    }
    ensure_bounded(expected.pixels().len())?;

    let difference_image = difference_image(expected, actual);
    let report = ImageComparisonReport {
        report_version: 1,
        width: expected.width(),
        height: expected.height(),
        options,
        comparison,
        artifacts: ComparisonReportArtifacts {
            expected: "expected.png".into(),
            actual: "actual.png".into(),
            difference: "difference.png".into(),
        },
        difference_visualization: DifferenceVisualization {
            encoding: "straight-alpha RGBA8".into(),
            red: "visible linear-RGB error after compositing over black and white; ceil(max error * 255)".into(),
            green: "absolute alpha-byte error".into(),
            blue: "maximum raw RGB-byte error only when the composited visible error is zero".into(),
            alpha: "always 255".into(),
        },
    };

    fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let directory = create_report_directory(&root)?;
    let result = write_report_files(&directory, expected, actual, &difference_image, &report);
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result
}

fn difference_image(expected: &Image, actual: &Image) -> Image {
    let mut pixels = Vec::with_capacity(expected.pixels().len());
    for (expected, actual) in expected
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(actual.pixels().as_chunks::<4>().0.iter())
    {
        let visible = [0.0, 1.0]
            .into_iter()
            .flat_map(|background| {
                crate::compare::composite(expected, background)
                    .into_iter()
                    .zip(crate::compare::composite(actual, background))
                    .map(|(expected, actual)| (expected - actual).abs())
            })
            .fold(0.0_f64, f64::max);
        let visible = (visible * 255.0).ceil().clamp(0.0, 255.0) as u8;
        let alpha = expected[3].abs_diff(actual[3]);
        let raw_rgb = expected[..3]
            .iter()
            .zip(&actual[..3])
            .map(|(&expected, &actual)| expected.abs_diff(actual))
            .max()
            .unwrap_or(0);
        let invisible_rgb = if visible == 0 { raw_rgb } else { 0 };
        pixels.extend_from_slice(&[visible, alpha, invisible_rgb, 255]);
    }
    Image::new(expected.width(), expected.height(), pixels)
        .expect("difference pixels retain the input image dimensions")
}

fn create_report_directory(root: &Path) -> Result<PathBuf, ComparisonReportError> {
    loop {
        let nonce = NEXT_REPORT.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = root.join(format!("comparison-{}-{now}-{nonce}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

fn write_report_files(
    directory: &Path,
    expected_image: &Image,
    actual_image: &Image,
    difference_image: &Image,
    report: &ImageComparisonReport,
) -> Result<WrittenComparisonReport, ComparisonReportError> {
    let expected = directory.join(&report.artifacts.expected);
    let actual = directory.join(&report.artifacts.actual);
    let difference = directory.join(&report.artifacts.difference);
    write_bounded_png(expected_image, &expected)?;
    write_bounded_png(actual_image, &actual)?;
    write_bounded_png(difference_image, &difference)?;

    let bytes = serde_json::to_vec_pretty(report)?;
    ensure_bounded(bytes.len())?;
    let manifest = directory.join("report.json");
    let temporary = directory.join("report.json.part");
    private_file(&temporary)?.write_all(&bytes)?;
    fs::rename(temporary, &manifest)?;
    Ok(WrittenComparisonReport {
        directory: directory.to_path_buf(),
        manifest,
        expected,
        actual,
        difference,
    })
}

fn write_bounded_png(image: &Image, path: &Path) -> Result<(), ComparisonReportError> {
    let mut bytes = Vec::new();
    write_png(image, &mut bytes)?;
    ensure_bounded(bytes.len())?;
    private_file(path)?.write_all(&bytes)?;
    Ok(())
}

fn ensure_bounded(length: usize) -> Result<(), ComparisonReportError> {
    if length > MAX_REPORT_IMAGE_BYTES {
        Err(ComparisonReportError::TooLarge)
    } else {
        Ok(())
    }
}

fn private_file(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_size_limit_includes_its_boundary() {
        assert!(ensure_bounded(MAX_REPORT_IMAGE_BYTES).is_ok());
        assert!(matches!(
            ensure_bounded(MAX_REPORT_IMAGE_BYTES + 1),
            Err(ComparisonReportError::TooLarge)
        ));
    }
}
