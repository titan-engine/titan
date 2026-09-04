#[path = "support/procedural_rpg.rs"]
mod game;

use game::{
    QuestState, build_game, build_inspector, configured_inspector, image_checksum, recorded_walk,
    replay,
};
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use titan::Startup;
use titan::inspection::InspectionConfig;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    match run_native_mode() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    let mut app = build_game();

    replay(&mut app, &recorded_walk());

    let image = game::render_image(app.world()).expect("render RPG frame");
    let output_path = PathBuf::from("target/titan/procedural-rpg.ppm");
    let mut inspector = build_inspector(output_path.clone());
    let capture = inspector.handle(
        &mut app,
        &titan_protocol::RequestEnvelope::new("capture", titan_protocol::Request::Capture),
    );
    if let titan_protocol::ResponseOutcome::Failure { error } = capture.outcome {
        eprintln!("capture failed: {}", error.message);
        std::process::exit(1);
    }

    let quest = app.world().resource::<QuestState>().unwrap();
    println!(
        "wrote {} ({} shards, shrine active: {}, checksum: {:016x})",
        output_path.display(),
        quest.collected_shards,
        quest.shrine_active,
        image_checksum(&image)
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn run_native_mode() -> Result<bool, Box<dyn std::error::Error>> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};
    use titan_remote::{Server, ServerConfig};

    let mut args = std::env::args().skip(1);
    let mut serve = false;
    let mut project = std::env::current_dir()?;
    let mut instance = format!("procedural-rpg-{}", std::process::id());
    let mut duration = None;
    let mut configured = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--serve" => serve = true,
            "--project" => {
                project = args.next().ok_or("--project requires a directory")?.into();
                configured = true;
            }
            "--instance" => {
                instance = args.next().ok_or("--instance requires an ID")?;
                configured = true;
            }
            "--run-for-ms" => {
                let millis: u64 = args
                    .next()
                    .ok_or("--run-for-ms requires milliseconds")?
                    .parse()?;
                duration = Some(Duration::from_millis(millis));
                configured = true;
            }
            "--help" | "-h" => {
                println!(
                    "procedural_rpg [--serve [--project DIR] [--instance ID] [--run-for-ms MS]]\nWithout --serve, replays the reference walk and writes target/titan/procedural-rpg.ppm.\nServe mode starts paused at frame 0; use titan attach and protocol requests to drive it.\nCtrl-C or SIGTERM stops the server and removes its discovery registration."
                );
                return Ok(true);
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !serve {
        if configured {
            return Err("--project, --instance, and --run-for-ms require --serve".into());
        }
        return Ok(false);
    }
    if instance.is_empty()
        || instance.len() > 128
        || !instance
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(
            "instance ID must contain 1–128 ASCII letters, digits, hyphens, or underscores".into(),
        );
    }
    let project = project.canonicalize()?;
    let stopped = Arc::new(AtomicBool::new(false));
    let stop_signal = stopped.clone();
    ctrlc::set_handler(move || stop_signal.store(true, Ordering::Release))?;

    let mut app = build_game();
    app.update_schedule(Startup);
    let output = project
        .join("target/titan")
        .join(format!("{instance}-{}", std::process::id()))
        .join("capture.ppm");
    let mut inspector = configured_inspector(
        output,
        InspectionConfig::controlled(&instance, project.to_string_lossy()),
    );
    let (mut server, queue) = Server::start(ServerConfig::new(
        &project,
        instance,
        titan_protocol::RunMode::Headless,
    ))?;
    eprintln!(
        "serving {} at {} (paused at frame 0)",
        server.registration().instance_id,
        server.registration().endpoint
    );
    let started = Instant::now();
    while !stopped.load(Ordering::Acquire)
        && duration.is_none_or(|duration| started.elapsed() < duration)
    {
        // This thread alone owns the game, and fixed time advances only on Step.
        queue.drain(|request| inspector.handle(&mut app, request));
        std::thread::sleep(Duration::from_millis(1));
    }
    server.shutdown();
    Ok(true)
}
