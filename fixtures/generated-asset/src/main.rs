use std::{io, path::PathBuf, time::Instant};
use titan_generated_asset::{
    BUILD_GENERATIONS, BUILD_OUTCOME, BUILD_PNG, LazyTile, decode,
    generator::{self, Generator, Inputs},
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut directory = None;
    let mut inputs = Inputs::default();
    let mut args = std::env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--cache-dir") => {
                directory = Some(PathBuf::from(args.next().ok_or("missing cache directory")?))
            }
            Some("--seed") => {
                inputs.seed = args
                    .next()
                    .ok_or("missing seed")?
                    .to_str()
                    .ok_or("invalid seed")?
                    .parse()?
            }
            Some("--generator-version") => {
                inputs.version = args
                    .next()
                    .ok_or("missing generator version")?
                    .to_str()
                    .ok_or("invalid version")?
                    .parse()?
            }
            Some("--help") => {
                println!(
                    "titan-generated-asset --cache-dir DIR [--seed U32] [--generator-version U32]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {}", argument.to_string_lossy()).into()),
        }
    }
    let directory = directory.ok_or("--cache-dir DIR is required")?;
    let mut uncached = Generator::default();
    let start = Instant::now();
    let baseline = uncached.generate(inputs)?;
    let uncached_us = start.elapsed().as_micros();
    let mut lazy = LazyTile::new(&directory, inputs);
    let before_access = lazy.generator.generation_count;
    let start = Instant::now();
    let first = lazy.get()?;
    let outcome = first.outcome.as_str();
    let cached = first.png.clone();
    let lazy_first_us = start.elapsed().as_micros();
    let start = Instant::now();
    let memory_parity = lazy.get()?.png == cached;
    let lazy_second_us = start.elapsed().as_micros();
    let mut startup = Generator::default();
    let start = Instant::now();
    let startup_asset = generator::load(&directory, inputs, &mut startup)?;
    let startup_us = start.elapsed().as_micros();
    // A loose PNG is decoded by the same Titan API as cached and embedded bytes.
    // Give each process its own export so concurrent differing inputs cannot race.
    let loose_path = directory.join(format!("{}-{}.png", inputs.key(), std::process::id()));
    std::fs::write(&loose_path, &cached)?;
    let from_file = decode(&std::fs::read(&loose_path)?)?;
    std::fs::remove_file(&loose_path)?;
    let mut build_baseline_generator = Generator::default();
    let build_baseline = build_baseline_generator.generate(Inputs::default())?;
    let decoded = decode(&baseline)?;
    let parity = cached == baseline
        && startup_asset.png == baseline
        && memory_parity
        && decode(&cached)? == decoded
        && from_file == decoded
        && build_baseline == BUILD_PNG
        && decode(BUILD_PNG)? == decode(&build_baseline)?;
    if !parity {
        return Err(io::Error::other("asset path parity failed").into());
    }
    println!(
        "{}",
        serde_json::json!({
            "schema_version": 1,
            "cache_key": inputs.key(), "cache_path": inputs.cache_path(&directory),
            "cache_outcome": outcome, "parity": parity,
            "seed": inputs.seed, "generator_version": inputs.version,
            "png_bytes": cached.len(), "pixel_checksum": format!("{:016x}", generator::checksum(decoded.pixels())),
            "generation_count": lazy.generator.generation_count,
            "lazy_generation_count": lazy.generator.generation_count,
            "before_access_generation_count": before_access,
            "uncached_generation_count": uncached.generation_count,
            "startup_generation_count": startup.generation_count,
            "startup_cache_outcome": startup_asset.outcome.as_str(),
            "build_cache_outcome": BUILD_OUTCOME,
            "build_generation_count": BUILD_GENERATIONS.parse::<u64>()?,
            "timings_us": { "uncached": uncached_us, "lazy_first_access": lazy_first_us,
                "lazy_second_access": lazy_second_us, "startup_warm": startup_us }
        })
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("generated asset fixture: {error}");
        std::process::exit(1);
    }
}
