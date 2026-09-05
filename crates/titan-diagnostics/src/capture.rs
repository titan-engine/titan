//! PNG artifacts for native diagnostics and owned protocol captures.

use base64::{Engine, engine::general_purpose::STANDARD};
use titan::render::Image;
use titan_protocol::{CaptureResult, ErrorCode, ProtocolError};

/// Encode the image's exact RGBA8 pixels to a PNG destination.
/// The caller owns destination creation, permissions and artifact size policy.
pub fn write_png(
    image: &Image,
    destination: impl std::io::Write,
) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(destination, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(image.pixels())?;
    writer.finish()
}

/// Encode an image as an inline PNG protocol capture, with an FNV-1a checksum
/// of its unencoded RGBA8 pixels. Rendering and capture selection remain game-owned.
pub fn png_capture(image: &Image) -> Result<CaptureResult, ProtocolError> {
    titan::inspection::CaptureLimits::default()
        .validate_dimensions(image.width(), image.height())?;
    struct BoundedPng(Vec<u8>);
    impl std::io::Write for BoundedPng {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > (2 * 1024 * 1024usize).saturating_sub(self.0.len()) {
                return Err(std::io::Error::other(
                    "encoded PNG exceeds inline capture limit",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut encoded = BoundedPng(Vec::new());
    write_png(image, &mut encoded).map_err(|error| {
        ProtocolError::new(ErrorCode::Internal, format!("PNG capture failed: {error}"))
    })?;
    let bytes = encoded.0;
    let checksum = image
        .pixels()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
        });
    Ok(CaptureResult {
        identity: Default::default(),
        width: image.width(),
        height: image.height(),
        format: "png".into(),
        artifact: format!("data:image/png;base64,{}", STANDARD.encode(bytes)),
        checksum: format!("{checksum:016x}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_capture_preserves_rgba_and_checksum() {
        let pixels = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let image = Image::new(2, 1, pixels.clone()).unwrap();
        let capture = png_capture(&image).unwrap();
        assert_eq!((capture.width, capture.height), (2, 1));
        assert_eq!(capture.checksum, "e5d5ef76aa248952");
        let bytes = STANDARD
            .decode(
                capture
                    .artifact
                    .strip_prefix("data:image/png;base64,")
                    .unwrap(),
            )
            .unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut decoded).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn encoding_reports_destination_failure() {
        struct Failed;
        impl std::io::Write for Failed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("destination unavailable"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let image = Image::new(1, 1, vec![0; 4]).unwrap();
        assert!(write_png(&image, Failed).is_err());
    }
}
