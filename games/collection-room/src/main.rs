#[cfg(not(target_arch = "wasm32"))]
use titan::{Startup, inspection::InspectionConfig};
#[cfg(not(target_arch = "wasm32"))]
use titan_collection_room::game::{self, build_game, configured_inspector};
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if run_native_mode()? {
        return Ok(());
    }
    let mut app = build_game();
    app.update_schedule(Startup);
    app.refresh_extracted();
    println!("{}", game::status(&app));
    Ok(())
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
    let mut instance = format!("collection-room-{}", std::process::id());
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
                    "titan-collection-room [--serve [--project DIR] [--instance ID] [--run-for-ms MS] [--diagnostics on-failure|always|never] [--allow-mutation]]\nWithout --serve, prints the initial semantic state. No GPU or image capture is used.\nServe mode starts paused at frame 0; use the titan CLI to inspect and drive it.\nCtrl-C or SIGTERM stops the server and removes its discovery registration."
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
    app.refresh_extracted();
    let mut config = InspectionConfig::controlled(&instance, project.to_string_lossy());
    config.mutation_enabled = allow_mutation;
    let mut inspector = configured_inspector(config);
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
            let response = match inspector.dispatch(&mut app, request) {
                titan::inspection::Dispatch::Ready(response) => response,
                titan::inspection::Dispatch::Pending(mut capture) => {
                    reply.complete_when(request_started, move |elapsed| capture.poll(elapsed));
                    return;
                }
            };
            let elapsed_us =
                u64::try_from(request_started.elapsed().as_micros()).unwrap_or(u64::MAX);
            let result = diagnostics.record_response(
                &inspector,
                &app,
                request,
                response,
                elapsed_us,
                |app, bundle| {
                    bundle.world_state["game"] = game::status(app);
                    None
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
