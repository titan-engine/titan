//! Bounded byte decoding. Asset paths and transport remain game/host concerns.
use super::Image;
use std::{error::Error, fmt, io::Cursor};

/// Limits checked before allocating image buffers. The allocation budget covers
/// our two worst-case RGBA buffers plus the PNG decoder's best-effort internal
/// allocation accounting; it is not a process-wide memory limit. Encoded input
/// is borrowed and is bounded separately. Text and color-profile chunks are ignored.
#[derive(Clone, Copy, Debug)]
pub struct ImageDecodeLimits {
    pub max_encoded_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub max_decoded_bytes: usize,
    pub max_allocation_bytes: usize,
}

impl Default for ImageDecodeLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: 8 * 1024 * 1024,
            max_width: 4096,
            max_height: 4096,
            max_decoded_bytes: 64 * 1024 * 1024,
            max_allocation_bytes: 160 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub enum ImageDecodeError {
    EncodedLimit { actual: usize, maximum: usize },
    InvalidHeader,
    DimensionLimit { width: u32, height: u32 },
    DecodedLimit,
    AllocationLimit,
    Animated,
    UnsupportedColor,
    Png(::png::DecodingError),
}

impl fmt::Display for ImageDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncodedLimit { actual, maximum } => write!(
                formatter,
                "PNG encoded size {actual} exceeds {maximum} bytes"
            ),
            Self::InvalidHeader => write!(
                formatter,
                "PNG signature or IHDR header is missing or invalid"
            ),
            Self::DimensionLimit { width, height } => write!(
                formatter,
                "PNG dimensions {width}x{height} are zero or exceed configured limits"
            ),
            Self::DecodedLimit => write!(
                formatter,
                "PNG RGBA byte count exceeds decoded size limit or address space"
            ),
            Self::AllocationLimit => write!(
                formatter,
                "PNG exceeds allocation budget or buffer allocation failed"
            ),
            Self::Animated => write!(formatter, "animated PNG assets are unsupported"),
            Self::UnsupportedColor => write!(
                formatter,
                "PNG did not decode to a supported eight-bit color format"
            ),
            Self::Png(error) => write!(formatter, "PNG decode failed: {error}"),
        }
    }
}

impl Error for ImageDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Png(error) => Some(error),
            _ => None,
        }
    }
}

impl From<::png::DecodingError> for ImageDecodeError {
    fn from(error: ::png::DecodingError) -> Self {
        match error {
            ::png::DecodingError::LimitsExceeded => Self::AllocationLimit,
            error => Self::Png(error),
        }
    }
}

fn zeroed(length: usize) -> Result<Vec<u8>, ImageDecodeError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| ImageDecodeError::AllocationLimit)?;
    bytes.resize(length, 0);
    Ok(bytes)
}

impl Image {
    /// Decodes a static PNG into straight-alpha RGBA8, without filesystem or
    /// network access. Palette/transparency and grayscale are expanded; sixteen
    /// bit samples retain their high byte. No gamma/color-profile conversion is
    /// applied. Invalid checksums, truncated streams and APNG are rejected.
    pub fn from_png(bytes: &[u8], limits: ImageDecodeLimits) -> Result<Self, ImageDecodeError> {
        if bytes.len() > limits.max_encoded_bytes {
            return Err(ImageDecodeError::EncodedLimit {
                actual: bytes.len(),
                maximum: limits.max_encoded_bytes,
            });
        }
        // Preflight IHDR before read_info can allocate decoder working storage.
        // The decoder subsequently validates the full chunk, including its CRC.
        if bytes.len() < 33
            || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
            || bytes[8..12] != 13u32.to_be_bytes()
            || &bytes[12..16] != b"IHDR"
        {
            return Err(ImageDecodeError::InvalidHeader);
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if width == 0 || height == 0 || width > limits.max_width || height > limits.max_height {
            return Err(ImageDecodeError::DimensionLimit { width, height });
        }
        let rgba_len = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|count| count.checked_mul(4))
            .and_then(|count| usize::try_from(count).ok())
            .filter(|count| *count <= limits.max_decoded_bytes)
            .ok_or(ImageDecodeError::DecodedLimit)?;
        let internal_budget = rgba_len
            .checked_mul(2)
            .and_then(|buffers| limits.max_allocation_bytes.checked_sub(buffers))
            .ok_or(ImageDecodeError::AllocationLimit)?;
        let mut decoder = ::png::Decoder::new_with_limits(
            Cursor::new(bytes),
            ::png::Limits {
                bytes: internal_budget,
            },
        );
        decoder
            .set_transformations(::png::Transformations::EXPAND | ::png::Transformations::STRIP_16);
        decoder.set_ignore_text_chunk(true);
        decoder.set_ignore_iccp_chunk(true);
        let mut reader = decoder.read_info()?;
        if reader.info().animation_control.is_some() {
            return Err(ImageDecodeError::Animated);
        }
        let output_len = reader
            .output_buffer_size()
            .filter(|length| *length <= rgba_len)
            .ok_or(ImageDecodeError::DecodedLimit)?;
        let mut decoded = zeroed(output_len)?;
        let output = reader.next_frame(&mut decoded)?;
        // Read IEND and validate remaining chunks, rather than accepting a stream
        // truncated immediately after the image's compressed data.
        reader.finish()?;
        if reader.info().animation_control.is_some() {
            return Err(ImageDecodeError::Animated);
        }
        if output.bit_depth != ::png::BitDepth::Eight {
            return Err(ImageDecodeError::UnsupportedColor);
        }
        let mut rgba = zeroed(rgba_len)?;
        let samples = &decoded[..output.buffer_size()];
        match output.color_type {
            ::png::ColorType::Rgba => rgba.copy_from_slice(samples),
            ::png::ColorType::Rgb => {
                for (pixel, rgb) in rgba
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(samples.as_chunks::<3>().0.iter())
                {
                    pixel.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
            ::png::ColorType::Grayscale => {
                for (pixel, gray) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(samples) {
                    pixel.copy_from_slice(&[*gray, *gray, *gray, 255]);
                }
            }
            ::png::ColorType::GrayscaleAlpha => {
                for (pixel, gray) in rgba
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(samples.as_chunks::<2>().0.iter())
                {
                    pixel.copy_from_slice(&[gray[0], gray[0], gray[0], gray[1]]);
                }
            }
            ::png::ColorType::Indexed => return Err(ImageDecodeError::UnsupportedColor),
        }
        Ok(Self {
            width,
            height,
            pixels: rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Color;

    fn encode(color: ::png::ColorType, depth: ::png::BitDepth, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = ::png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(color);
            encoder.set_depth(depth);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
        bytes
    }

    #[test]
    fn exact_rgba_and_common_color_formats() {
        let rgba = [255, 2, 3, 0, 4, 5, 6, 128];
        assert_eq!(
            Image::from_png(
                &encode(::png::ColorType::Rgba, ::png::BitDepth::Eight, &rgba),
                ImageDecodeLimits::default()
            )
            .unwrap(),
            Image::new(2, 1, rgba.to_vec()).unwrap()
        );
        for (color, depth, encoded, expected) in [
            (
                ::png::ColorType::Rgb,
                ::png::BitDepth::Eight,
                vec![1, 2, 3, 4, 5, 6],
                vec![1, 2, 3, 255, 4, 5, 6, 255],
            ),
            (
                ::png::ColorType::Grayscale,
                ::png::BitDepth::Eight,
                vec![1, 2],
                vec![1, 1, 1, 255, 2, 2, 2, 255],
            ),
            (
                ::png::ColorType::GrayscaleAlpha,
                ::png::BitDepth::Eight,
                vec![1, 2, 3, 4],
                vec![1, 1, 1, 2, 3, 3, 3, 4],
            ),
            (
                ::png::ColorType::Grayscale,
                ::png::BitDepth::Sixteen,
                vec![1, 255, 2, 0],
                vec![1, 1, 1, 255, 2, 2, 2, 255],
            ),
            (
                ::png::ColorType::Grayscale,
                ::png::BitDepth::One,
                vec![0b01000000],
                vec![0, 0, 0, 255, 255, 255, 255, 255],
            ),
        ] {
            assert_eq!(
                Image::from_png(
                    &encode(color, depth, &encoded),
                    ImageDecodeLimits::default()
                )
                .unwrap()
                .pixels(),
                expected
            );
        }
    }

    #[test]
    fn palette_transparency_and_apng_rejection() {
        let mut bytes = Vec::new();
        {
            let mut encoder = ::png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(::png::ColorType::Indexed);
            encoder.set_depth(::png::BitDepth::Eight);
            encoder.set_palette(vec![1, 2, 3, 4, 5, 6]);
            encoder.set_trns(vec![0, 128]);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[0, 1])
                .unwrap();
        }
        let image = Image::from_png(&bytes, ImageDecodeLimits::default()).unwrap();
        assert_eq!(image.pixel(0, 0), Some(Color::rgba(1, 2, 3, 0)));
        assert_eq!(image.pixel(1, 0), Some(Color::rgba(4, 5, 6, 128)));
        let mut animated = Vec::new();
        {
            let mut encoder = ::png::Encoder::new(&mut animated, 2, 1);
            encoder.set_animated(1, 0).unwrap();
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[0, 1])
                .unwrap();
        }
        assert!(matches!(
            Image::from_png(&animated, ImageDecodeLimits::default()),
            Err(ImageDecodeError::Animated)
        ));
    }

    #[test]
    fn rejects_corruption_truncation_and_all_limit_boundaries() {
        let bytes = encode(::png::ColorType::Rgba, ::png::BitDepth::Eight, &[0; 8]);
        for end in 0..bytes.len() {
            assert!(
                Image::from_png(&bytes[..end], ImageDecodeLimits::default()).is_err(),
                "accepted prefix {end}"
            );
        }
        let mut corrupt = bytes.clone();
        corrupt[29] ^= 1; // IHDR CRC
        assert!(Image::from_png(&corrupt, ImageDecodeLimits::default()).is_err());
        let defaults = ImageDecodeLimits::default();
        for limits in [
            ImageDecodeLimits {
                max_encoded_bytes: bytes.len() - 1,
                ..defaults
            },
            ImageDecodeLimits {
                max_width: 1,
                ..defaults
            },
            ImageDecodeLimits {
                max_height: 0,
                ..defaults
            },
            ImageDecodeLimits {
                max_decoded_bytes: 7,
                ..defaults
            },
            ImageDecodeLimits {
                max_allocation_bytes: 15,
                ..defaults
            },
            ImageDecodeLimits {
                max_allocation_bytes: 16,
                ..defaults
            },
        ] {
            assert!(Image::from_png(&bytes, limits).is_err());
        }
        assert!(
            Image::from_png(
                &bytes,
                ImageDecodeLimits {
                    max_encoded_bytes: bytes.len(),
                    max_width: 2,
                    max_height: 1,
                    max_decoded_bytes: 8,
                    ..defaults
                }
            )
            .is_ok()
        );
        let mut huge = bytes.clone();
        huge[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        huge[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            Image::from_png(&huge, defaults),
            Err(ImageDecodeError::DimensionLimit { .. })
        ));
        assert!(matches!(
            Image::from_png(
                &huge,
                ImageDecodeLimits {
                    max_width: u32::MAX,
                    max_height: u32::MAX,
                    max_decoded_bytes: usize::MAX,
                    ..defaults
                }
            ),
            Err(ImageDecodeError::DecodedLimit)
        ));
    }
}
