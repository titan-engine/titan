//! One native fixture, deliberately not an engine asset-management API.
pub mod generator;

use generator::{Asset, Generator, Inputs};
use std::{
    io,
    path::{Path, PathBuf},
};
use titan::render::{Image, ImageDecodeLimits};

pub const BUILD_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/generated.png"));
pub const BUILD_OUTCOME: &str = env!("TITAN_ASSET_BUILD_OUTCOME");
pub const BUILD_GENERATIONS: &str = env!("TITAN_ASSET_BUILD_GENERATIONS");

pub fn decode(png: &[u8]) -> io::Result<Image> {
    Image::from_png(
        png,
        ImageDecodeLimits {
            max_encoded_bytes: generator::MAX_PNG_BYTES,
            max_width: generator::SIDE,
            max_height: generator::SIDE,
            max_decoded_bytes: generator::PIXEL_BYTES,
            max_allocation_bytes: 1024 * 1024,
        },
    )
    .map_err(io::Error::other)
}

pub struct LazyTile {
    directory: PathBuf,
    inputs: Inputs,
    asset: Option<Asset>,
    pub generator: Generator,
}

impl LazyTile {
    /// Construction performs no filesystem access or generation.
    pub fn new(directory: &Path, inputs: Inputs) -> Self {
        Self {
            directory: directory.to_owned(),
            inputs,
            asset: None,
            generator: Generator::default(),
        }
    }

    pub fn get(&mut self) -> io::Result<&Asset> {
        if self.asset.is_none() {
            self.asset = Some(generator::load(
                &self.directory,
                self.inputs,
                &mut self.generator,
            )?);
        }
        Ok(self.asset.as_ref().unwrap())
    }
}
