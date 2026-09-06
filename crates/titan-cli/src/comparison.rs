use crate::output::{CommandData, CommandResult, ComparisonArtifacts};
use std::io::Read;
use std::path::Path;
use titan::render::{Image, ImageDecodeLimits};
use titan_diagnostics::{
    ComparisonOptions, ComparisonReportError, compare_images, write_comparison_report,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn compare_image_files(
    expected_path: &Path,
    actual_path: &Path,
    output: &Path,
    exact: bool,
    maximum_channel_error: Option<u8>,
    minimum_ssim: Option<f64>,
    maximum_linear_rmse: Option<f64>,
) -> CommandResult {
    let started = std::time::Instant::now();
    let failure = |code, message: String| CommandResult {
        command: "compare_images",
        success: false,
        exit_code: Some(1),
        stdout: String::new(),
        stderr: format!("{message}\n"),
        data: None,
        diagnostic_bundle: None,
        diagnostic_error: None,
        error_code: Some(code),
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    };
    let expected_text = match utf8_path(expected_path, "expected PNG") {
        Ok(path) => path.to_owned(),
        Err(error) => return failure("invalid_value", error),
    };
    let actual_text = match utf8_path(actual_path, "actual PNG") {
        Ok(path) => path.to_owned(),
        Err(error) => return failure("invalid_value", error),
    };
    if let Err(error) = utf8_path(output, "output directory") {
        return failure("invalid_value", error);
    }
    let options = if exact {
        ComparisonOptions::exact()
    } else {
        let defaults = ComparisonOptions::default();
        ComparisonOptions {
            maximum_channel_error,
            minimum_ssim: minimum_ssim.unwrap_or(defaults.minimum_ssim),
            maximum_linear_rmse: maximum_linear_rmse.unwrap_or(defaults.maximum_linear_rmse),
        }
    };
    let expected = match read_png(expected_path) {
        Ok(image) => image,
        Err(error) => return failure("invalid_value", error),
    };
    let actual = match read_png(actual_path) {
        Ok(image) => image,
        Err(error) => return failure("invalid_value", error),
    };
    let comparison = match compare_images(&expected, &actual, options) {
        Ok(comparison) => comparison,
        Err(error) => return failure("invalid_value", error.to_string()),
    };
    if let Err(error) = std::fs::create_dir_all(output) {
        return failure(
            "artifact_write_failed",
            format!("cannot create report root `{}`: {error}", output.display()),
        );
    }
    let output = match output.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return failure(
                "artifact_write_failed",
                format!("cannot resolve report root `{}`: {error}", output.display()),
            );
        }
    };
    if utf8_path(&output, "resolved output directory").is_err() {
        return failure(
            "invalid_value",
            "resolved output directory must be representable as UTF-8".into(),
        );
    }
    let written = match write_comparison_report(&output, &expected, &actual, options) {
        Ok(written) => written,
        Err(ComparisonReportError::Comparison(error)) => {
            return failure("invalid_value", error.to_string());
        }
        Err(error) => return failure("artifact_write_failed", error.to_string()),
    };
    let artifact_path = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| "written report path is not representable as UTF-8".to_owned())
    };
    let artifacts = match (
        artifact_path(&written.directory),
        artifact_path(&written.manifest),
        artifact_path(&written.expected),
        artifact_path(&written.actual),
        artifact_path(&written.difference),
    ) {
        (Ok(directory), Ok(manifest), Ok(expected), Ok(actual), Ok(difference)) => {
            ComparisonArtifacts {
                directory,
                manifest,
                expected,
                actual,
                difference,
            }
        }
        _ => {
            return failure(
                "artifact_write_failed",
                "written report paths are not representable as UTF-8".into(),
            );
        }
    };
    let success = comparison.passes;
    CommandResult {
        command: "compare_images",
        success,
        exit_code: Some(if success { 0 } else { 2 }),
        stdout: String::new(),
        stderr: String::new(),
        data: Some(CommandData::ImageComparison {
            expected: expected_text,
            actual: actual_text,
            width: expected.width(),
            height: expected.height(),
            options,
            comparison,
            artifacts,
        }),
        diagnostic_bundle: None,
        diagnostic_error: None,
        error_code: (!success).then_some("visual_mismatch"),
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    }
}

fn utf8_path<'a>(path: &'a Path, label: &str) -> Result<&'a str, String> {
    path.to_str()
        .ok_or_else(|| format!("{label} must be representable as UTF-8"))
}

fn read_png(path: &Path) -> Result<Image, String> {
    let limits = ImageDecodeLimits::default();
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let input = options
        .open(path)
        .map_err(|error| format!("cannot open PNG `{}`: {error}", path.display()))?;
    let metadata = input
        .metadata()
        .map_err(|error| format!("cannot inspect PNG `{}`: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("PNG `{}` must be a regular file", path.display()));
    }
    if metadata.len() > limits.max_encoded_bytes as u64 {
        return Err(format!(
            "PNG `{}` exceeds the {} byte encoded input limit",
            path.display(),
            limits.max_encoded_bytes
        ));
    }
    let mut bytes = Vec::new();
    input
        .take(limits.max_encoded_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read PNG `{}`: {error}", path.display()))?;
    Image::from_png(&bytes, limits)
        .map_err(|error| format!("cannot decode PNG `{}`: {error}", path.display()))
}
