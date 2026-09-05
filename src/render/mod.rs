//! Renderer-neutral 2D/3D data and deterministic 2D software rendering.

pub mod three_d;

#[cfg(feature = "image-png")]
mod png;
mod software;

#[cfg(feature = "image-png")]
pub use png::{ImageDecodeError, ImageDecodeLimits};

pub use software::SoftwareRenderer;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

/// An eight-bit, straight-alpha RGBA color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::rgba(red, green, blue, 255)
    }

    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub(crate) const fn channels(self) -> [u8; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }
}

/// An error produced while constructing an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    InvalidPixelCount {
        width: u32,
        height: u32,
        actual: usize,
    },
    DimensionsTooLarge,
}

impl fmt::Display for ImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPixelCount {
                width,
                height,
                actual,
            } => write!(
                formatter,
                "{width}x{height} RGBA image needs {} bytes, received {actual}",
                u64::from(*width) * u64::from(*height) * 4
            ),
            Self::DimensionsTooLarge => write!(formatter, "image dimensions exceed address space"),
        }
    }
}

impl Error for ImageError {}

/// CPU-resident RGBA8 image data shared by procedural and imported assets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Image {
    /// Creates an image from row-major RGBA8 pixels.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        let expected = pixel_byte_len(width, height)?;
        if pixels.len() != expected {
            return Err(ImageError::InvalidPixelCount {
                width,
                height,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Generates an image entirely from code.
    pub fn from_fn(
        width: u32,
        height: u32,
        mut pixel: impl FnMut(u32, u32) -> Color,
    ) -> Result<Self, ImageError> {
        let mut pixels = Vec::with_capacity(pixel_byte_len(width, height)?);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&pixel(x, y).channels());
            }
        }
        Self::new(width, height, pixels)
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = ((y as usize * self.width as usize) + x as usize) * 4;
        Some(Color::rgba(
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ))
    }
}

fn pixel_byte_len(width: u32, height: u32) -> Result<usize, ImageError> {
    let length = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ImageError::DimensionsTooLarge)?;
    usize::try_from(length).map_err(|_| ImageError::DimensionsTooLarge)
}

/// Stable process-local handle to an image asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageId(u64);

impl ImageId {
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Deterministically allocated CPU image assets.
#[derive(Clone, Default)]
pub struct ImageAssets {
    next_id: u64,
    images: BTreeMap<ImageId, Image>,
}

impl ImageAssets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, image: Image) -> ImageId {
        let id = ImageId(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("image ID overflowed");
        self.images.insert(id, image);
        id
    }

    pub fn get(&self, id: ImageId) -> Option<&Image> {
        self.images.get(&id)
    }

    pub fn remove(&mut self, id: ImageId) -> Option<Image> {
        self.images.remove(&id)
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// One nearest-neighbor image draw in framebuffer pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteDraw {
    pub image: ImageId,
    pub x: i32,
    pub y: i32,
    pub layer: i32,
    pub order: i32,
    pub tint: Color,
    pub pixel_scale: u32,
}

impl SpriteDraw {
    pub const fn new(image: ImageId, x: i32, y: i32) -> Self {
        Self {
            image,
            x,
            y,
            layer: 0,
            order: 0,
            tint: Color::WHITE,
            pixel_scale: 1,
        }
    }

    pub const fn with_layer(mut self, layer: i32) -> Self {
        self.layer = layer;
        self
    }

    pub const fn with_order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    pub const fn with_tint(mut self, tint: Color) -> Self {
        self.tint = tint;
        self
    }

    /// Sets an integer nearest-neighbor scale.
    ///
    /// # Panics
    ///
    /// Panics when `pixel_scale` is zero.
    pub const fn with_pixel_scale(mut self, pixel_scale: u32) -> Self {
        assert!(pixel_scale > 0, "pixel scale must be non-zero");
        self.pixel_scale = pixel_scale;
        self
    }
}

/// An owned, renderer-neutral description of one 2D frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderFrame {
    width: u32,
    height: u32,
    clear: Color,
    sprites: Vec<SpriteDraw>,
}

impl RenderFrame {
    pub const fn new(width: u32, height: u32, clear: Color) -> Self {
        Self {
            width,
            height,
            clear,
            sprites: Vec::new(),
        }
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn clear_color(&self) -> Color {
        self.clear
    }

    pub fn push(&mut self, sprite: SpriteDraw) {
        self.sprites.push(sprite);
    }

    pub fn sprites(&self) -> &[SpriteDraw] {
        &self.sprites
    }
}

/// A rendering failure expressed independently of a renderer backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    MissingImage(ImageId),
    InvalidFrameDimensions,
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingImage(id) => {
                write!(formatter, "image asset {} does not exist", id.value())
            }
            Self::InvalidFrameDimensions => {
                write!(formatter, "framebuffer dimensions exceed address space")
            }
        }
    }
}

impl Error for RenderError {}
