//! RPG startup asset policy. Hosts deliver bytes; the engine decodes bounded images.
use titan::render::{Image, ImageDecodeLimits};

pub const MAX_PLAYER_PNG_BYTES: usize = 256 * 1024;

pub fn decode_player_png(bytes: &[u8]) -> Result<Image, String> {
    Image::from_png(
        bytes,
        ImageDecodeLimits {
            max_encoded_bytes: MAX_PLAYER_PNG_BYTES,
            max_width: 64,
            max_height: 64,
            max_decoded_bytes: 64 * 64 * 4,
            max_allocation_bytes: 2 * 1024 * 1024,
        },
    )
    .map_err(|error| {
        format!("player.png: {error}; provide a valid static PNG up to 64x64 and 256 KiB")
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_player(directory: Option<&std::path::Path>, generated: bool) -> Result<Image, String> {
    use std::{fs::File, io::Read};
    if generated {
        if directory.is_some() {
            return Err("--generated-assets conflicts with --assets-dir".into());
        }
        return Ok(super::generated_player());
    }
    let directory = match directory {
        Some(path) => path.to_owned(),
        None => {
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            let parent = executable
                .parent()
                .ok_or("executable has no parent directory")?;
            if parent.file_name().is_some_and(|n| n == "MacOS")
                && parent
                    .parent()
                    .and_then(|p| p.file_name())
                    .is_some_and(|n| n == "Contents")
            {
                parent.parent().unwrap().join("Resources/assets")
            } else {
                std::env::current_dir()
                    .map_err(|error| error.to_string())?
                    .join("assets")
            }
        }
    };
    let path = directory.join("player.png");
    let failure = |message: String| {
        format!(
            "asset {}: {message}; restore player.png or pass --assets-dir DIR (use --generated-assets for the procedural comparison)",
            path.display()
        )
    };
    // Reject obvious non-files before opening (a FIFO could block in open).
    let before = std::fs::metadata(&path).map_err(|error| failure(error.to_string()))?;
    if !before.is_file() || before.len() > MAX_PLAYER_PNG_BYTES as u64 {
        return Err(failure(
            "expected a regular PNG file no larger than 256 KiB".into(),
        ));
    }
    let mut file = File::open(&path).map_err(|error| failure(error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| failure(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > MAX_PLAYER_PNG_BYTES as u64 {
        return Err(failure(
            "expected a regular PNG file no larger than 256 KiB".into(),
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_PLAYER_PNG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| failure(error.to_string()))?;
    decode_player_png(&bytes).map_err(failure)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_sprite_matches_procedural_pixels_and_reference() {
        let loaded = decode_player_png(include_bytes!("../../assets/player.png")).unwrap();
        assert_eq!(loaded, super::super::generated_player());
        let mut app = super::super::build_game_with_player(loaded);
        super::super::replay(&mut app, &super::super::recorded_walk());
        assert_eq!(
            super::super::image_checksum(&super::super::render_image(app.world()).unwrap()),
            0xf7a298f62ad75c1c
        );
    }
}
