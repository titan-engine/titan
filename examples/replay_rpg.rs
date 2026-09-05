//! Verify a snapshot-backed RPG recording without a window or GPU.
#[path = "support/procedural_rpg.rs"]
pub mod game;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs::File, io::Read, path::Path};
    const MAX_BYTES: u64 = 2 * 1024 * 1024;
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: replay_rpg RECORDING.json [--assets-dir DIR | --generated-assets]")?;
    let mut assets_dir: Option<std::path::PathBuf> = None;
    let mut generated = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--assets-dir" => {
                assets_dir = Some(
                    args.next()
                        .ok_or("--assets-dir requires a directory")?
                        .into(),
                )
            }
            "--generated-assets" => generated = true,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let image = game::assets::load_player(assets_dir.as_deref(), generated)?;
    let metadata = std::fs::metadata(Path::new(&path))?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("recording must be a regular JSON file no larger than 2 MiB".into());
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("recording exceeds the 2 MiB limit".into());
    }
    let value = serde_json::from_slice(&bytes)?;
    let result = game::live::verify_recording_with_player(value, image)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
