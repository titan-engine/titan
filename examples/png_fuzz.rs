//! Deterministic generated-input exerciser; invoke through scripts/fuzz-png.py
//! for process-level resource/time bounds. This is not a coverage-guided fuzzer.
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use titan::render::{Image, ImageDecodeLimits};

const MAX_BYTES: usize = 64 * 1024;
const MAX_CASE_FILE: u64 = 512 * 1024;

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Limits {
    max_encoded_bytes: usize,
    max_width: u32,
    max_height: u32,
    max_decoded_bytes: usize,
    max_allocation_bytes: usize,
}
impl Limits {
    fn generous() -> Self {
        Self {
            max_encoded_bytes: MAX_BYTES,
            max_width: 4096,
            max_height: 4096,
            max_decoded_bytes: 1024 * 1024,
            max_allocation_bytes: 4 * 1024 * 1024,
        }
    }
    fn decode(self) -> ImageDecodeLimits {
        ImageDecodeLimits {
            max_encoded_bytes: self.max_encoded_bytes,
            max_width: self.max_width,
            max_height: self.max_height,
            max_decoded_bytes: self.max_decoded_bytes,
            max_allocation_bytes: self.max_allocation_bytes,
        }
    }
}
#[derive(Serialize, Deserialize)]
struct Case {
    bytes: Vec<u8>,
    limits: Limits,
    label: String,
    #[serde(default)]
    expected_success: Option<bool>,
}
fn read_case(path: &Path) -> Case {
    assert!(
        fs::metadata(path).unwrap().len() <= MAX_CASE_FILE,
        "oversized corpus JSON"
    );
    let case: Case = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(case.bytes.len() <= MAX_BYTES, "oversized input");
    case
}
fn exercise(case: &Case, artifact: &Path, counts: &mut [usize; 2]) {
    assert!(case.bytes.len() <= MAX_BYTES);
    // Commit the exact input and limits before entering the decoder, including on
    // abort, signal, timeout or allocation failure (not merely unwind panics).
    fs::write(artifact, serde_json::to_vec(case).unwrap()).unwrap();
    let result = Image::from_png(&case.bytes, case.limits.decode());
    if let Some(expected) = case.expected_success {
        assert_eq!(result.is_ok(), expected, "{}: {result:?}", case.label);
    }
    match result {
        Ok(image) => {
            counts[0] += 1;
            let (width, height) = (image.width(), image.height());
            assert!(width > 0 && height > 0);
            assert!(width <= case.limits.max_width && height <= case.limits.max_height);
            let len = usize::try_from(u64::from(width) * u64::from(height) * 4).unwrap();
            assert_eq!(image.pixels().len(), len);
            assert!(len <= case.limits.max_decoded_bytes);
            assert!(len.checked_mul(2).unwrap() <= case.limits.max_allocation_bytes);
            assert!(case.bytes.len() <= case.limits.max_encoded_bytes);
        }
        Err(_) => counts[1] += 1,
    }
}
fn crc(bytes: &[u8]) -> u32 {
    let mut value = !0u32;
    for &byte in bytes {
        value ^= u32::from(byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(value & 1)));
        }
    }
    !value
}
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = (data.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(&crc(&bytes[4..]).to_be_bytes());
    bytes
}
fn chunks(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut output = Vec::new();
    let mut at = 8;
    while at + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if length > bytes.len() - at - 12 {
            break;
        }
        output.push(bytes[at..at + length + 12].to_vec());
        at += length + 12;
    }
    output
}
fn assemble(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    for part in parts {
        bytes.extend_from_slice(part);
    }
    bytes
}
fn seeds() -> Vec<Vec<u8>> {
    let mut seeds = Vec::new();
    for color in [
        png::ColorType::Grayscale,
        png::ColorType::GrayscaleAlpha,
        png::ColorType::Rgb,
        png::ColorType::Rgba,
        png::ColorType::Indexed,
    ] {
        for depth in [
            png::BitDepth::One,
            png::BitDepth::Two,
            png::BitDepth::Four,
            png::BitDepth::Eight,
            png::BitDepth::Sixteen,
        ] {
            if (color == png::ColorType::Indexed && depth == png::BitDepth::Sixteen)
                || (!matches!(color, png::ColorType::Grayscale | png::ColorType::Indexed)
                    && (depth as usize) < 8)
            {
                continue;
            }
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 3, 2);
                encoder.set_color(color);
                encoder.set_depth(depth);
                if color == png::ColorType::Indexed {
                    encoder.set_palette(vec![11, 22, 33, 44, 55, 66]);
                    encoder.set_trns(vec![0, 128]);
                }
                let row = (3 * color.samples() * depth as usize).div_ceil(8);
                let pixels: Vec<u8> = (0..row * 2)
                    .map(|index| {
                        if color == png::ColorType::Indexed {
                            // Only palette indices 0 and 1, including packed samples.
                            match depth {
                                png::BitDepth::One => 0b0100_0000,
                                png::BitDepth::Two => 0b0001_0000,
                                png::BitDepth::Four => 0b0000_0001,
                                _ => (index % 2) as u8,
                            }
                        } else {
                            (index as u8).wrapping_mul(73).wrapping_add(19)
                        }
                    })
                    .collect();
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&pixels)
                    .unwrap();
            }
            seeds.push(bytes);
        }
    }
    seeds
}
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn index(&mut self, len: usize) -> usize {
        (self.next() % len as u64) as usize
    }
}
fn mutate(seed: &[u8], iteration: usize, rng: &mut Rng) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    match iteration % 8 {
        0 => {
            bytes.truncate(rng.index(bytes.len() + 1));
        }
        1 => {
            for _ in 0..1 + rng.index(8) {
                let at = rng.index(bytes.len());
                bytes[at] ^= 1 << rng.index(8);
            }
        }
        2 => {
            let at = 16 + rng.index(13);
            bytes[at] = rng.next() as u8;
            let checksum = crc(&bytes[12..29]);
            bytes[29..33].copy_from_slice(&checksum.to_be_bytes());
        }
        3..=6 => {
            let mut parts = chunks(seed);
            let at = rng.index(parts.len());
            match iteration % 8 {
                3 => {
                    parts.remove(at);
                }
                4 => {
                    parts.insert(at, parts[at].clone());
                }
                5 => {
                    let other = rng.index(parts.len());
                    parts.swap(at, other);
                }
                _ => {
                    let kinds = [
                        *b"tEXt", *b"iCCP", *b"acTL", *b"fcTL", *b"tRNS", *b"PLTE", *b"IDAT",
                        *b"IEND", *b"zzZZ", *b"ZZZZ",
                    ];
                    let length = rng.index(64);
                    let data: Vec<_> = (0..length).map(|_| rng.next() as u8).collect();
                    let kind = kinds[rng.index(kinds.len())];
                    parts.insert(at, chunk(&kind, &data));
                }
            }
            bytes = assemble(&parts);
        }
        _ => {
            // Mutate compressed payload while repairing the PNG CRC so zlib
            // validation, rather than only chunk checksums, gets exercised.
            let mut parts = chunks(seed);
            for part in &mut parts {
                if &part[4..8] == b"IDAT" && part.len() > 12 {
                    let at = 8 + rng.index(part.len() - 12);
                    part[at] ^= rng.next() as u8;
                    let end = part.len() - 4;
                    let checksum = crc(&part[4..end]);
                    part[end..].copy_from_slice(&checksum.to_be_bytes());
                }
            }
            bytes = assemble(&parts);
        }
    }
    bytes
}
fn variants(bytes: &[u8], rng: &mut Rng) -> [Limits; 8] {
    let mut values = [Limits::generous(); 8];
    values[1].max_encoded_bytes = bytes.len().saturating_sub(1);
    values[2].max_width = rng.index(5) as u32;
    values[2].max_height = rng.index(4) as u32;
    values[3].max_decoded_bytes = rng.index(26);
    values[4].max_allocation_bytes = rng.index(50);
    values[5].max_encoded_bytes = bytes.len();
    values[5].max_width = 3;
    values[5].max_height = 2;
    values[5].max_decoded_bytes = 24;
    values[6].max_encoded_bytes = rng.index(bytes.len() + 1);
    values[6].max_width = rng.index(8) as u32;
    values[6].max_height = rng.index(8) as u32;
    values[6].max_decoded_bytes = rng.index(100);
    values[6].max_allocation_bytes = rng.index(200);
    values[7].max_width = u32::MAX;
    values[7].max_height = u32::MAX;
    values
}
fn main() {
    let mut seed = 69u64;
    let mut iterations = 1024usize;
    let mut corpus = "fixtures/png-corpus".to_string();
    let mut artifact = "png-fuzz-current.json".to_string();
    let mut replay = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().expect("every flag needs a value");
        match arg.as_str() {
            "--seed" => seed = value.parse().unwrap(),
            "--iterations" => iterations = value.parse().unwrap(),
            "--corpus" => corpus = value,
            "--artifact" => artifact = value,
            "--replay" => replay = Some(value),
            _ => panic!("unknown argument: {arg}"),
        }
    }
    assert!(iterations <= 1_000_000);
    let artifact = Path::new(&artifact);
    let mut counts = [0; 2];
    if let Some(replay) = replay {
        exercise(&read_case(Path::new(&replay)), artifact, &mut counts);
    } else {
        let mut paths = Vec::new();
        for entry in fs::read_dir(corpus).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                assert!(paths.len() < 64, "corpus exceeds 64 cases");
                paths.push(path);
            }
        }
        paths.sort();
        for path in paths {
            exercise(&read_case(&path), artifact, &mut counts);
        }
        let seeds = seeds();
        let mut rng = Rng(seed);
        for (index, bytes) in seeds.iter().enumerate() {
            exercise(
                &Case {
                    bytes: bytes.clone(),
                    limits: Limits::generous(),
                    label: format!("valid-seed-{index}"),
                    expected_success: Some(true),
                },
                artifact,
                &mut counts,
            );
            // Known-valid seeds prove rejection gates as well as the successful
            // boundary, avoiding a harness that silently accepts all errors.
            let mut boundaries = [Limits::generous(); 7];
            boundaries[0].max_encoded_bytes = bytes.len() - 1;
            boundaries[1].max_width = 2;
            boundaries[2].max_height = 1;
            boundaries[3].max_decoded_bytes = 23;
            boundaries[4].max_allocation_bytes = 47;
            boundaries[5].max_allocation_bytes = 48;
            boundaries[6].max_encoded_bytes = bytes.len();
            boundaries[6].max_width = 3;
            boundaries[6].max_height = 2;
            boundaries[6].max_decoded_bytes = 24;
            for (variant, limits) in boundaries.into_iter().enumerate() {
                exercise(
                    &Case {
                        bytes: bytes.clone(),
                        limits,
                        label: format!("valid-seed-{index}-boundary-{variant}"),
                        expected_success: Some(variant == 6),
                    },
                    artifact,
                    &mut counts,
                );
            }
            for end in 0..bytes.len() {
                exercise(
                    &Case {
                        bytes: bytes[..end].to_vec(),
                        limits: Limits::generous(),
                        label: format!("truncation-{index}-{end}"),
                        expected_success: Some(false),
                    },
                    artifact,
                    &mut counts,
                );
            }
        }
        for index in 0..iterations {
            let bytes = mutate(&seeds[rng.index(seeds.len())], index, &mut rng);
            for (variant, limits) in variants(&bytes, &mut rng).into_iter().enumerate() {
                exercise(
                    &Case {
                        bytes: bytes.clone(),
                        limits,
                        label: format!("seed-{seed}-iteration-{index}-limits-{variant}"),
                        expected_success: None,
                    },
                    artifact,
                    &mut counts,
                );
            }
        }
    }
    println!(
        "PNG corpus passed: seed={seed} iterations={iterations} accepted={} rejected={}",
        counts[0], counts[1]
    );
}
