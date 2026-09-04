#[cfg(not(target_arch = "wasm32"))]
use titan::{Startup, inspection::InspectionConfig};
#[cfg(not(target_arch = "wasm32"))]
use titan_game::game::{self, build_game, configured_inspector};
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if run_native_mode()? {
        return Ok(());
    }
    let mut app = build_game();
    app.update_schedule(Startup);
    let mut inspector = configured_inspector(
        "target/titan/capture.ppm".into(),
        InspectionConfig::controlled("minimal-game", "."),
    );
    let response = inspector.handle(
        &mut app,
        &titan_protocol::RequestEnvelope::new("capture", titan_protocol::Request::Capture),
    );
    match response.outcome {
        titan_protocol::ResponseOutcome::Failure { error } => Err(error.message.into()),
        _ => {
            println!("{}", game::status(&app));
            Ok(())
        }
    }
}
#[cfg(target_arch = "wasm32")]
fn main() {}

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
    let mut instance = format!("minimal-game-{}", std::process::id());
    let mut duration = None;
    let mut configured = false;
    let mut allow_mutation = false;
    let mut diagnostic_policy = titan_diagnostics::DiagnosticPolicy::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--serve" => serve = true,
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
                    "titan-game [--serve [--project DIR] [--instance ID] [--run-for-ms MS] [--diagnostics on-failure|always|never] [--allow-mutation]]\nWithout --serve, renders the initial scene and writes target/titan/capture.ppm.\nServe mode starts paused at frame 0; use the titan CLI to inspect and drive it.\nCtrl-C or SIGTERM stops the server and removes its discovery registration."
                );
                return Ok(true);
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !serve {
        if configured {
            return Err(
                "--project, --instance, --run-for-ms, --diagnostics, and --allow-mutation require --serve".into(),
            );
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
    let mut config = InspectionConfig::controlled(&instance, project.to_string_lossy());
    config.mutation_enabled = allow_mutation;
    let mut inspector = configured_inspector(output, config);
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
        queue.drain(|request| {
            let result = diagnostics.handle(&mut inspector, &mut app, request, |app, bundle| {
                bundle.world_state["positions"] = game::diagnostic_positions(app.world());
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
            });
            for error in result.errors {
                eprintln!("{error}");
            }
            if let Some(written) = result.written {
                eprintln!("diagnostic bundle: {}", written.manifest.display());
            }
            result.response
        });
        std::thread::sleep(Duration::from_millis(1));
    }
    server.shutdown();
    Ok(true)
}
