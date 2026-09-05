//! Fixture-owned generator/cache shared verbatim by build.rs and the native runner.
//! Version changes are explicit: bump GENERATOR_VERSION when generation changes.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const GENERATOR_VERSION: u32 = 1;
pub const SIDE: u32 = 16;
pub const PIXEL_BYTES: usize = (SIDE * SIDE * 4) as usize;
pub const MAX_PNG_BYTES: usize = 8 * 1024;
pub const HEADER_BYTES: usize = 24;
const MAGIC: &[u8; 8] = b"TITANFX1";
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inputs {
    pub seed: u32,
    pub version: u32,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            seed: 7,
            version: GENERATOR_VERSION,
        }
    }
}

impl Inputs {
    // Fixed dimensions, PNG encoding and envelope layout belong to version.
    // Numeric fields are encoded directly, so cache identity has no hash collisions.
    pub fn key(self) -> String {
        format!("tile-v{}-seed{}", self.version, self.seed)
    }

    pub fn cache_path(self, directory: &Path) -> PathBuf {
        directory.join(format!("{}.cache", self.key()))
    }
}

#[derive(Default)]
pub struct Generator {
    pub generation_count: u64,
}

impl Generator {
    pub fn generate(&mut self, inputs: Inputs) -> io::Result<Vec<u8>> {
        self.generation_count += 1;
        let mut pixels = Vec::with_capacity(PIXEL_BYTES);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let value = inputs.seed.wrapping_add(x * 17).wrapping_add(y * 31);
                let border = x == 0 || y == 0 || x == SIDE - 1 || y == SIDE - 1;
                pixels.extend_from_slice(&[
                    (value & 255) as u8,
                    ((value >> 4) & 255) as u8,
                    if (x / 4 + y / 4) % 2 == 0 { 210 } else { 90 },
                    if border { 0 } else { 255 },
                ]);
            }
        }
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, SIDE, SIDE);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&pixels)?;
        }
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Generated,
    Reused,
    Recovered,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Reused => "reused",
            Self::Recovered => "recovered",
        }
    }
}

pub struct Asset {
    pub png: Vec<u8>,
    pub outcome: Outcome,
}

// Integrity checksum for accidental corruption, not authentication. Cache directories
// are owned by the fixture runner; this is not a hostile-content trust boundary.
pub fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn valid_png(bytes: &[u8]) -> bool {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: 1024 * 1024 });
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };
    let info = reader.info();
    if info.width != SIDE
        || info.height != SIDE
        || info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || info.animation_control.is_some()
    {
        return false;
    }
    let mut pixels = [0; PIXEL_BYTES];
    reader.next_frame(&mut pixels).is_ok() && reader.finish().is_ok()
}

fn read_entry(path: &Path, inputs: Inputs) -> io::Result<Option<Vec<u8>>> {
    let file = File::open(path)?;
    // Bound both the initial allocation and bytes read, including concurrent growth.
    if file.metadata()?.len() > (HEADER_BYTES + MAX_PNG_BYTES) as u64 {
        return Ok(None);
    }
    let mut entry = Vec::new();
    file.take((HEADER_BYTES + MAX_PNG_BYTES + 1) as u64)
        .read_to_end(&mut entry)?;
    if entry.len() < HEADER_BYTES
        || entry.len() > HEADER_BYTES + MAX_PNG_BYTES
        || &entry[..8] != MAGIC
        || entry[8..12] != inputs.seed.to_le_bytes()
        || entry[12..16] != inputs.version.to_le_bytes()
        || entry[16..24] != checksum(&entry[HEADER_BYTES..]).to_le_bytes()
        || !valid_png(&entry[HEADER_BYTES..])
    {
        return Ok(None);
    }
    Ok(Some(entry.split_off(HEADER_BYTES)))
}

// Same-directory temporary + rename publishes only complete entries. Concurrent
// misses may duplicate generation, but all writers produce the same bytes.
fn write_entry(path: &Path, inputs: Inputs, png: &[u8]) -> io::Result<()> {
    let (temporary, mut file) = loop {
        let candidate = path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(MAGIC)?;
        file.write_all(&inputs.seed.to_le_bytes())?;
        file.write_all(&inputs.version.to_le_bytes())?;
        file.write_all(&checksum(png).to_le_bytes())?;
        file.write_all(png)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn load(directory: &Path, inputs: Inputs, generator: &mut Generator) -> io::Result<Asset> {
    let path = inputs.cache_path(directory);
    let outcome = match read_entry(&path, inputs) {
        Ok(Some(png)) => {
            return Ok(Asset {
                png,
                outcome: Outcome::Reused,
            });
        }
        Ok(None) => Outcome::Recovered,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Outcome::Generated,
        Err(error) => return Err(error),
    };
    let png = generator.generate(inputs)?;
    fs::create_dir_all(directory)?;
    write_entry(&path, inputs, &png)?;
    Ok(Asset { png, outcome })
}
