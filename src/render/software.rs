use super::{Color, Image, ImageAssets, RenderError, RenderFrame, SpriteDraw, pixel_byte_len};

/// Exact CPU reference renderer for tests, headless runs, and diagnostics.
pub struct SoftwareRenderer;

impl SoftwareRenderer {
    /// Renders a frame using deterministic integer pixel operations.
    pub fn render(frame: &RenderFrame, assets: &ImageAssets) -> Result<Image, RenderError> {
        let byte_len = pixel_byte_len(frame.width(), frame.height())
            .map_err(|_| RenderError::InvalidFrameDimensions)?;
        let mut pixels = Vec::with_capacity(byte_len);
        for _ in 0..u64::from(frame.width()) * u64::from(frame.height()) {
            pixels.extend_from_slice(&frame.clear_color().channels());
        }

        let mut sprites: Vec<(usize, &SpriteDraw)> = frame.sprites().iter().enumerate().collect();
        sprites.sort_by_key(|(insertion_order, sprite)| {
            (sprite.layer, sprite.order, *insertion_order)
        });

        for (_, sprite) in sprites {
            let image = assets
                .get(sprite.image)
                .ok_or(RenderError::MissingImage(sprite.image))?;
            draw_sprite(&mut pixels, frame.width(), frame.height(), image, sprite);
        }

        Image::new(frame.width(), frame.height(), pixels)
            .map_err(|_| RenderError::InvalidFrameDimensions)
    }
}

fn draw_sprite(
    target: &mut [u8],
    target_width: u32,
    target_height: u32,
    image: &Image,
    sprite: &SpriteDraw,
) {
    let scale = i64::from(sprite.pixel_scale);
    for source_y in 0..image.height() {
        for source_x in 0..image.width() {
            let source = tint(
                image
                    .pixel(source_x, source_y)
                    .expect("source coordinates are in bounds"),
                sprite.tint,
            );
            for scale_y in 0..sprite.pixel_scale {
                for scale_x in 0..sprite.pixel_scale {
                    let target_x =
                        i64::from(sprite.x) + i64::from(source_x) * scale + i64::from(scale_x);
                    let target_y =
                        i64::from(sprite.y) + i64::from(source_y) * scale + i64::from(scale_y);
                    if target_x < 0
                        || target_y < 0
                        || target_x >= i64::from(target_width)
                        || target_y >= i64::from(target_height)
                    {
                        continue;
                    }
                    let offset =
                        ((target_y as usize * target_width as usize) + target_x as usize) * 4;
                    let destination = Color::rgba(
                        target[offset],
                        target[offset + 1],
                        target[offset + 2],
                        target[offset + 3],
                    );
                    let output = blend(source, destination).channels();
                    target[offset..offset + 4].copy_from_slice(&output);
                }
            }
        }
    }
}

fn tint(source: Color, tint: Color) -> Color {
    Color::rgba(
        multiply_channel(source.red, tint.red),
        multiply_channel(source.green, tint.green),
        multiply_channel(source.blue, tint.blue),
        multiply_channel(source.alpha, tint.alpha),
    )
}

const fn multiply_channel(left: u8, right: u8) -> u8 {
    ((left as u16 * right as u16 + 127) / 255) as u8
}

fn blend(source: Color, destination: Color) -> Color {
    let source_alpha = u32::from(source.alpha);
    let destination_alpha = u32::from(destination.alpha);
    let inverse_source_alpha = 255 - source_alpha;
    let output_alpha_scaled = source_alpha * 255 + destination_alpha * inverse_source_alpha;

    if output_alpha_scaled == 0 {
        return Color::TRANSPARENT;
    }

    let channel = |source_channel: u8, destination_channel: u8| {
        let numerator = u32::from(source_channel) * source_alpha * 255
            + u32::from(destination_channel) * destination_alpha * inverse_source_alpha;
        ((numerator + output_alpha_scaled / 2) / output_alpha_scaled) as u8
    };

    Color::rgba(
        channel(source.red, destination.red),
        channel(source.green, destination.green),
        channel(source.blue, destination.blue),
        ((output_alpha_scaled + 127) / 255) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::SoftwareRenderer;
    use crate::render::{Color, Image, ImageAssets, RenderFrame, SpriteDraw};

    #[test]
    fn renders_scaled_clipped_tinted_sprites_in_layer_order() {
        let mut assets = ImageAssets::new();
        let red = assets.insert(Image::from_fn(1, 1, |_, _| Color::rgb(255, 0, 0)).unwrap());
        let white = assets.insert(Image::from_fn(1, 1, |_, _| Color::WHITE).unwrap());
        let mut frame = RenderFrame::new(3, 2, Color::BLACK);
        frame.push(
            SpriteDraw::new(white, 0, 0)
                .with_tint(Color::rgba(0, 0, 255, 128))
                .with_layer(2),
        );
        frame.push(
            SpriteDraw::new(red, -1, 0)
                .with_pixel_scale(2)
                .with_layer(1),
        );

        let rendered = SoftwareRenderer::render(&frame, &assets).unwrap();

        assert_eq!(rendered.pixel(0, 0), Some(Color::rgb(127, 0, 128)));
        assert_eq!(rendered.pixel(0, 1), Some(Color::rgb(255, 0, 0)));
        assert_eq!(rendered.pixel(1, 0), Some(Color::BLACK));
    }

    #[test]
    fn reports_missing_assets() {
        let mut source_assets = ImageAssets::new();
        let missing = source_assets.insert(Image::from_fn(1, 1, |_, _| Color::WHITE).unwrap());
        let mut frame = RenderFrame::new(1, 1, Color::BLACK);
        frame.push(SpriteDraw::new(missing, 0, 0));

        let error = SoftwareRenderer::render(&frame, &ImageAssets::new()).unwrap_err();

        assert_eq!(error, crate::render::RenderError::MissingImage(missing));
    }
}
