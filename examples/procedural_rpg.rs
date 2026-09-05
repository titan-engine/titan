#[path = "support/procedural_rpg.rs"]
pub mod game;

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
    render_reference(build_game());
}

fn render_reference(mut app: titan::App) {
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
    let mut assets_dir: Option<PathBuf> = None;
    let mut generated_assets = false;
    let mut export_png: Option<PathBuf> = None;
    let mut export_tree = false;
    let mut project = std::env::current_dir()?;
    let mut instance = format!("procedural-rpg-{}", std::process::id());
    let mut duration = None;
    let mut configured = false;
    let mut allow_mutation = false;
    let mut diagnostic_policy = titan_diagnostics::DiagnosticPolicy::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--serve" => serve = true,
            "--assets-dir" => {
                assets_dir = Some(
                    args.next()
                        .ok_or("--assets-dir requires a directory")?
                        .into(),
                )
            }
            "--generated-assets" => generated_assets = true,
            "--export-player-png" | "--export-tree-png" => {
                if export_png.is_some() {
                    return Err("only one export option is allowed".into());
                }
                export_tree = arg == "--export-tree-png";
                export_png = Some(args.next().ok_or("PNG export requires a path")?.into())
            }
            "--allow-mutation" => {
                allow_mutation = true;
                configured = true;
            }
            "--project" => {
                project = args.next().ok_or("--project requires a directory")?.into();
                configured = true;
            }
            "--instance" => {
                instance = args.next().ok_or("--instance requires an ID")?;
                configured = true;
            }
            "--diagnostics" => {
                diagnostic_policy = match args.next().as_deref() {
                    Some("on-failure") => titan_diagnostics::DiagnosticPolicy::OnFailure,
                    Some("always") => titan_diagnostics::DiagnosticPolicy::Always,
                    Some("never") => titan_diagnostics::DiagnosticPolicy::Never,
                    _ => return Err("--diagnostics requires on-failure, always, or never".into()),
                };
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
                    "procedural_rpg [--assets-dir DIR | --generated-assets] [--export-player-png PATH | --export-tree-png PATH] [--serve [--project DIR] [--instance ID] [--run-for-ms MS] [--diagnostics on-failure|always|never] [--allow-mutation]]\nWithout --serve, replays the reference walk and writes target/titan/procedural-rpg.ppm.\nServe mode starts paused at frame 0; use the titan CLI to inspect and drive it.\nCtrl-C or SIGTERM stops the server and removes its discovery registration."
                );
                return Ok(true);
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if let Some(path) = export_png {
        if serve || configured || assets_dir.is_some() || generated_assets {
            return Err("PNG export must be used alone".into());
        }
        let image = if export_tree {
            game::generated_tree()
        } else {
            game::generated_player()
        };
        titan_diagnostics::write_png(&image, std::fs::File::create(&path)?)?;
        println!("exported procedural sprite to {}", path.display());
        return Ok(true);
    }
    let image = game::assets::load_images(assets_dir.as_deref(), generated_assets)?;
    if !serve {
        if configured {
            return Err(
                "--project, --instance, --run-for-ms, --diagnostics, and --allow-mutation require --serve".into(),
            );
        }
        render_reference(game::build_game_with_images(image));
        return Ok(true);
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

    let mut app = game::build_game_with_images(image);
    app.update_schedule(Startup);
    let output = project
        .join("target/titan")
        .join(format!("{instance}-{}", std::process::id()))
        .join("capture.ppm");
    let mut config = InspectionConfig::controlled(&instance, project.to_string_lossy());
    config.mutation_enabled = allow_mutation;
    let inspector = configured_inspector(output, config);
    let mut session = game::live::RpgSession::new(app, inspector, true);
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
    let mut diagnostics =
        titan_diagnostics::DiagnosticInspector::new(project.join("target/titan/diagnostics"));
    diagnostics.policy = diagnostic_policy;
    let started = Instant::now();
    while !stopped.load(Ordering::Acquire)
        && duration.is_none_or(|duration| started.elapsed() < duration)
    {
        // This thread alone owns the game, and fixed time advances only on Step.
        queue.drain_with_reply(|request, reply| {
            let request_started = Instant::now();
            let response = match session.dispatch(request) {
                titan::inspection::Dispatch::Ready(response) => response,
                titan::inspection::Dispatch::Pending(mut capture) => {
                    reply.complete_when(request_started, move |elapsed| capture.poll(elapsed));
                    return;
                }
            };
            let elapsed_us =
                u64::try_from(request_started.elapsed().as_micros()).unwrap_or(u64::MAX);
            let result = diagnostics.record_response(
                session.inspector(),
                session.app(),
                request,
                response,
                elapsed_us,
                |app, bundle| {
                    bundle.world_state["positions"] = game::diagnostic_positions(app.world());
                    if let Some(quest) = app.world().resource::<QuestState>() {
                        bundle.world_state["quest"] = serde_json::json!({
                            "collected_shards": quest.collected_shards,
                            "shrine_active": quest.shrine_active,
                        });
                    }
                    match game::render_image(app.world()) {
                        Ok(image) => Some(image),
                        Err(error) => {
                            bundle.logs.push(titan_diagnostics::DiagnosticLog {
                                level: "warning".into(),
                                message: format!("diagnostic capture failed: {}", error.message),
                                frame: bundle
                                    .response
                                    .as_ref()
                                    .map(|response| response.observed_frame),
                            });
                            None
                        }
                    }
                },
            );
            for error in result.errors {
                eprintln!("{error}");
            }
            if let Some(written) = result.written {
                eprintln!("diagnostic bundle: {}", written.manifest.display());
            }
            reply.send(result.response);
        });
        std::thread::sleep(Duration::from_millis(1));
    }
    server.shutdown();
    Ok(true)
}
