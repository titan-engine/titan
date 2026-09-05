//! Verify a bounded saved live-player recording without a window or server.
use std::{fs::File, io::Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: replay RECORDING.json")?;
    if args.next().is_some() {
        return Err("usage: replay RECORDING.json".into());
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(titan_game::live::MAX_RECORDING_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > titan_game::live::MAX_RECORDING_BYTES {
        return Err("recording file exceeds 2 MiB limit".into());
    }
    let result = titan_game::live::verify_recording(serde_json::from_slice(&bytes)?)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
