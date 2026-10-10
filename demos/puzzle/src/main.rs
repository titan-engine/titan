//! Windowed launcher; simulation and content remain available without rendering.

mod ux;
mod visuals;

extern crate alloc;

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use std::{error::Error, path::PathBuf};

use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured},
};
use titan_puzzle::{content::starter_levels, level::Level, Game, GameplayPlugin, FIXED_HZ};

#[derive(Resource, Default)]
struct Capture {
    path: Option<PathBuf>,
    frames: u32,
    saved: Arc<AtomicBool>,
}

fn main() -> Result<AppExit, Box<dyn Error>> {
    let mut levels = starter_levels();
    let mut capture = Capture::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--level" => {
                let path = args.next().ok_or("--level requires a file path")?;
                levels = vec![Level::parse(&path, &std::fs::read_to_string(&path)?)?];
            }
            "--capture" => {
                let path = PathBuf::from(args.next().ok_or("--capture requires a PNG path")?);
                if path.extension().is_none_or(|extension| extension != "png") {
                    return Err("--capture requires a .png extension".into());
                }
                capture.path = Some(path);
            }
            _ => {
                return Err(format!(
                    "unknown argument {arg:?}; use --level FILE or --capture FILE.png"
                )
                .into())
            }
        }
    }
    let saved = capture.path.as_ref().map(|_| Arc::clone(&capture.saved));
    let exit = App::new()
        .insert_resource(Game::new(levels)?)
        .insert_resource(capture)
        .insert_resource(Time::<Fixed>::from_hz(FIXED_HZ))
        .insert_resource(ClearColor(Color::srgb(0.045, 0.075, 0.1)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Titan — Puzzle foundation".into(),
                resolution: (960, 600).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((GameplayPlugin, ux::UxPlugin, visuals::VisualsPlugin))
        .add_systems(Update, capture_frame)
        .run();
    if saved.is_some_and(|saved| !saved.load(Ordering::Relaxed)) {
        return Err("capture ended before the screenshot was saved".into());
    }
    Ok(exit)
}

fn capture_frame(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut capture: ResMut<Capture>,
) {
    capture.frames += 1;
    if capture.frames == 120
        && let Some(path) = capture.path.take()
    {
        commands.spawn(Screenshot::primary_window()).observe(
            move |captured: On<ScreenshotCaptured>,
                  capture: Res<Capture>,
                  mut exit: MessageWriter<AppExit>| {
                let result = captured
                    .image
                    .clone()
                    .try_into_dynamic()
                    .map_err(|error| error.to_string())
                    .and_then(|image| {
                        image
                            .to_rgb8()
                            .save(&path)
                            .map_err(|error| error.to_string())
                    });
                match result {
                    Ok(()) => {
                        capture.saved.store(true, Ordering::Relaxed);
                        info!("Screenshot saved to {}", path.display());
                        exit.write(AppExit::Success);
                    }
                    Err(error) => {
                        error!("Cannot save screenshot to {}: {error}", path.display());
                        exit.write(AppExit::error());
                    }
                }
            },
        );
    }
    if keys.just_pressed(KeyCode::F12) {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("titan-puzzle.png"));
    }
}
