use serde::{Deserialize, Serialize};
use titan::render::Image;
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComparisonOptions {
    pub maximum_channel_error: Option<u8>,
    pub minimum_ssim: f64,
    pub maximum_linear_rmse: f64,
}
impl Default for ComparisonOptions {
    fn default() -> Self {
        Self {
            maximum_channel_error: None,
            minimum_ssim: 0.99,
            maximum_linear_rmse: 0.01,
        }
    }
}
impl ComparisonOptions {
    pub const fn exact() -> Self {
        Self {
            maximum_channel_error: Some(0),
            minimum_ssim: 1.0,
            maximum_linear_rmse: 0.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageComparison {
    pub exact: bool,
    pub passes: bool,
    pub differing_pixels: u64,
    pub maximum_channel_error: u8,
    pub mean_absolute_channel_error: f64,
    pub linear_rmse: f64,
    pub ssim: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComparisonError {
    DimensionsMismatch,
    InvalidThresholds,
}
impl std::fmt::Display for ComparisonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::DimensionsMismatch => "image dimensions differ",
                Self::InvalidThresholds =>
                    "comparison thresholds must be finite, SSIM in [-1,1], and RMSE in [0,1]",
            }
        )
    }
}
impl std::error::Error for ComparisonError {}
/// Compare exact RGBA bytes and perceptual appearance over both black and white.
/// SSIM uses non-overlapping 8×8 windows, population moments, K1=.01/K2=.03,
/// and normalized linear-sRGB luminance. This is a documented block variant,
/// not the original paper's Gaussian-window implementation. RGB RMSE detects
/// color changes that a luminance-only structural metric would miss.
pub fn compare_images(
    left: &Image,
    right: &Image,
    options: ComparisonOptions,
) -> Result<ImageComparison, ComparisonError> {
    if !options.minimum_ssim.is_finite()
        || !(-1.0..=1.0).contains(&options.minimum_ssim)
        || !options.maximum_linear_rmse.is_finite()
        || !(0.0..=1.0).contains(&options.maximum_linear_rmse)
    {
        return Err(ComparisonError::InvalidThresholds);
    }
    if left.width() != right.width() || left.height() != right.height() {
        return Err(ComparisonError::DimensionsMismatch);
    }
    let mut differing_pixels = 0;
    let mut maximum_channel_error = 0;
    let mut absolute = 0u64;
    for (a, b) in left
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(right.pixels().as_chunks::<4>().0.iter())
    {
        if a != b {
            differing_pixels += 1;
        }
        for (&a, &b) in a.iter().zip(b) {
            let error = a.abs_diff(b);
            maximum_channel_error = maximum_channel_error.max(error);
            absolute += u64::from(error);
        }
    }
    if differing_pixels == 0 {
        return Ok(ImageComparison {
            exact: true,
            passes: true,
            differing_pixels: 0,
            maximum_channel_error: 0,
            mean_absolute_channel_error: 0.,
            linear_rmse: 0.,
            ssim: 1.,
        });
    }
    let mut squared = 0.;
    let mut weighted_ssim = 0.;
    let mut samples = 0u64;
    let width = left.width() as usize;
    for top in (0..left.height() as usize).step_by(8) {
        for x0 in (0..width).step_by(8) {
            for background in [0., 1.] {
                let mut a_values = Vec::with_capacity(64);
                let mut b_values = Vec::with_capacity(64);
                for y in top..(top + 8).min(left.height() as usize) {
                    for x in x0..(x0 + 8).min(width) {
                        let offset = (y * width + x) * 4;
                        let a = composite(&left.pixels()[offset..offset + 4], background);
                        let b = composite(&right.pixels()[offset..offset + 4], background);
                        for i in 0..3 {
                            squared += (a[i] - b[i]).powi(2);
                        }
                        a_values.push(luminance(a));
                        b_values.push(luminance(b));
                    }
                }
                let n = a_values.len() as f64;
                let mean_a = a_values.iter().sum::<f64>() / n;
                let mean_b = b_values.iter().sum::<f64>() / n;
                let var_a = a_values.iter().map(|v| (v - mean_a).powi(2)).sum::<f64>() / n;
                let var_b = b_values.iter().map(|v| (v - mean_b).powi(2)).sum::<f64>() / n;
                let covariance = a_values
                    .iter()
                    .zip(&b_values)
                    .map(|(a, b)| (a - mean_a) * (b - mean_b))
                    .sum::<f64>()
                    / n;
                let score = ((2. * mean_a * mean_b + 0.0001) * (2. * covariance + 0.0009))
                    / ((mean_a * mean_a + mean_b * mean_b + 0.0001) * (var_a + var_b + 0.0009));
                weighted_ssim += score * n;
                samples += a_values.len() as u64;
            }
        }
    }
    let ssim = (weighted_ssim / samples as f64).clamp(-1., 1.);
    let linear_rmse = (squared / (samples as f64 * 3.)).sqrt();
    let passes = options
        .maximum_channel_error
        .is_none_or(|max| maximum_channel_error <= max)
        && ssim >= options.minimum_ssim
        && linear_rmse <= options.maximum_linear_rmse;
    Ok(ImageComparison {
        exact: false,
        passes,
        differing_pixels,
        maximum_channel_error,
        mean_absolute_channel_error: absolute as f64 / left.pixels().len() as f64,
        linear_rmse,
        ssim,
    })
}
pub(crate) fn composite(pixel: &[u8], background: f64) -> [f64; 3] {
    let alpha = f64::from(pixel[3]) / 255.;
    [pixel[0], pixel[1], pixel[2]].map(|byte| {
        let encoded = f64::from(byte) / 255.;
        let linear = if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        };
        linear * alpha + background * (1. - alpha)
    })
}
fn luminance(rgb: [f64; 3]) -> f64 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
