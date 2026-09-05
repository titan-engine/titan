//! Native window and authenticated inspection of the same collection-room App.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::{
        io::Read,
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    use titan::{
        inspection::Dispatch,
        render::{
            ImageAssets, RenderFrame,
            three_d::{BaseColor, Frame3dError, RenderFrame3d},
        },
    };
    use titan_collection_room::{game, player::PlayerSession};
    use titan_protocol::RunMode;
    use titan_remote::{RequestQueue, Server, ServerConfig};
    use titan_render_wgpu::{SurfaceRenderer3d, wgpu};
    use winit::{
        application::ApplicationHandler,
        dpi::LogicalSize,
        event::{ElementState, WindowEvent},
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        keyboard::{KeyCode, PhysicalKey},
        window::{Window, WindowId},
    };
    struct Player {
        session: PlayerSession,
        queue: Option<RequestQueue>,
        stopped: Arc<AtomicBool>,
        started: Instant,
        duration: Option<Duration>,
        limit: Option<u64>,
        rendered: u64,
        window: Option<Arc<Window>>,
        renderer: Option<SurfaceRenderer3d>,
        previous: Instant,
        accumulated: Duration,
        epoch: u64,
        error: Option<String>,
    }
    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut args = std::env::args().skip(1);
        let (mut limit, mut duration, mut recording) = (None, None, None);
        let (mut inspect, mut allow_control, mut configured, mut start_paused) =
            (false, false, false, false);
        let mut project = std::env::current_dir()?;
        let mut instance = format!("collection-player-{}", std::process::id());
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--frames" => {
                    limit = Some(positive(args.next(), "--frames")?);
                }
                "--run-for-ms" => {
                    duration = Some(Duration::from_millis(positive(
                        args.next(),
                        "--run-for-ms",
                    )?));
                }
                "--recording" => {
                    recording = Some(args.next().ok_or("--recording requires a JSON path")?);
                }
                "--paused" => start_paused = true,
                "--inspect" => inspect = true,
                "--allow-control" => {
                    allow_control = true;
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
                "--help" | "-h" => {
                    println!(
                        "play [--paused] [--recording PATH] [--frames N] [--run-for-ms MS] [--inspect [--allow-control] [--project DIR] [--instance ID]]\nWASD/arrows move; P pause/resume; N single tick while paused; R restart; L leave replay; Escape quit.\nRecordings start paused and replay actual fixed ticks. --inspect attaches authenticated local inspection to this played instance; remote control requires --allow-control. Capture remains unavailable.\n--frames counts successfully presented GPU frames; --run-for-ms bounds wall time."
                    );
                    return Ok(());
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }
        if configured && !inspect {
            return Err("--project, --instance and --allow-control require --inspect".into());
        }
        if instance.is_empty()
            || instance.len() > 128
            || !instance
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(
                "instance ID must contain 1–128 ASCII letters, digits, hyphens, or underscores"
                    .into(),
            );
        }
        let project = project.canonicalize()?;
        let mut session = PlayerSession::new(
            &instance,
            &project.to_string_lossy(),
            RunMode::Interactive,
            allow_control,
        );
        if let Some(path) = recording {
            session
                .load_replay(read_recording(Path::new(&path))?)
                .map_err(|e| e.message)?;
        } else if !start_paused {
            session.resume();
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let signal = stopped.clone();
        ctrlc::set_handler(move || signal.store(true, Ordering::Release))?;
        let (mut server, queue) = if inspect {
            let (server, queue) =
                Server::start(ServerConfig::new(&project, instance, RunMode::Interactive))?;
            eprintln!(
                "inspecting {} at {} ({})",
                server.registration().instance_id,
                server.registration().endpoint,
                if allow_control {
                    "control enabled"
                } else {
                    "read only"
                }
            );
            (Some(server), Some(queue))
        } else {
            (None, None)
        };
        let epoch = session.clock_epoch();
        let mut player = Player {
            session,
            queue,
            stopped,
            started: Instant::now(),
            duration,
            limit,
            rendered: 0,
            window: None,
            renderer: None,
            previous: Instant::now(),
            accumulated: Duration::ZERO,
            epoch,
            error: None,
        };
        let result = EventLoop::new()?.run_app(&mut player);
        if let Some(server) = &mut server {
            server.shutdown();
        }
        result?;
        if let Some(error) = player.error {
            return Err(error.into());
        }
        println!(
            "rendered {} GPU frames; {}",
            player.rendered,
            game::status(player.session.app())
        );
        Ok(())
    }
    fn positive(value: Option<String>, flag: &str) -> Result<u64, Box<dyn std::error::Error>> {
        let n = value
            .ok_or_else(|| format!("{flag} requires a positive integer"))?
            .parse::<u64>()?;
        if n == 0 {
            return Err(format!("{flag} must be positive").into());
        }
        Ok(n)
    }
    fn read_recording(path: &Path) -> Result<game::Recording, Box<dyn std::error::Error>> {
        const MAX: u64 = 2 * 1024 * 1024;
        let metadata = path.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX {
            return Err("recording must be a regular JSON file no larger than 2 MiB".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX {
            return Err("recording exceeds 2 MiB".into());
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
    impl Player {
        fn reset_clock(&mut self) {
            self.accumulated = Duration::ZERO;
            self.previous = Instant::now();
            self.epoch = self.session.clock_epoch();
        }
        fn sync_clock(&mut self) {
            if self.epoch != self.session.clock_epoch() {
                self.reset_clock();
            }
        }
        fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
            self.error = Some(error);
            event_loop.exit();
        }
        fn title(&self) -> String {
            let state = game::status(self.session.app());
            let playback = if self.session.replay_active() {
                format!(
                    " · Replay {}/{}",
                    self.session.replay_status()["position"],
                    self.session.replay_status()["total"]
                )
            } else {
                String::new()
            };
            format!(
                "Titan — Collection Room · {}/3{}{} · P pause · N step · R restart",
                state["collected"],
                playback,
                if self.session.paused() {
                    " · Paused"
                } else {
                    ""
                }
            )
        }
    }
    impl ApplicationHandler for Player {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.renderer.is_some() {
                return;
            }
            let result = (|| -> Result<(), String> {
                let window = match &self.window {
                    Some(w) => w.clone(),
                    None => Arc::new(
                        event_loop
                            .create_window(
                                Window::default_attributes()
                                    .with_title(self.title())
                                    .with_inner_size(LogicalSize::new(960., 540.)),
                            )
                            .map_err(|e| e.to_string())?,
                    ),
                };
                let size = window.inner_size();
                let instance =
                    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
                let surface = instance
                    .create_surface(window.clone())
                    .map_err(|e| e.to_string())?;
                self.renderer = Some(pollster::block_on(SurfaceRenderer3d::new(
                    &instance,
                    surface,
                    size.width,
                    size.height,
                ))?);
                self.window = Some(window);
                self.reset_clock();
                Ok(())
            })();
            if let Err(e) = result {
                self.fail(
                    event_loop,
                    format!("Collection room GPU initialization failed: {e}"),
                );
            }
        }
        fn suspended(&mut self, _: &ActiveEventLoop) {
            self.session.clear_input();
            self.renderer = None;
            self.reset_clock();
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(queue) = &self.queue {
                queue.drain_with_reply(|request, reply| {
                    let started = Instant::now();
                    match self.session.dispatch(request) {
                        Dispatch::Ready(response) => reply.send(response),
                        Dispatch::Pending(mut pending) => {
                            reply.complete_when(started, move |elapsed| pending.poll(elapsed))
                        }
                    }
                });
            }
            self.sync_clock();
            if self.stopped.load(Ordering::Acquire)
                || self.duration.is_some_and(|d| self.started.elapsed() >= d)
            {
                event_loop.exit();
                return;
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
            if let Some(window) = &self.window {
                window.set_title(&self.title());
                if self.renderer.is_some() {
                    window.request_redraw();
                }
            }
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.resize(size.width, size.height);
                    }
                    self.session.clear_input();
                    self.reset_clock();
                }
                WindowEvent::Focused(false) => {
                    self.session.pause();
                    self.reset_clock();
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let PhysicalKey::Code(key) = event.physical_key else {
                        return;
                    };
                    let pressed = event.state == ElementState::Pressed;
                    if pressed && !event.repeat {
                        match key {
                            KeyCode::Escape => {
                                event_loop.exit();
                                return;
                            }
                            KeyCode::KeyP => {
                                if self.session.paused() {
                                    self.session.resume()
                                } else {
                                    self.session.pause()
                                }
                                self.sync_clock();
                                return;
                            }
                            KeyCode::KeyN => {
                                if self.session.paused() {
                                    if let Err(e) = self.session.step() {
                                        eprintln!("step: {}", e.message);
                                    }
                                    self.reset_clock();
                                }
                                return;
                            }
                            KeyCode::KeyR => {
                                self.session.restart();
                                self.sync_clock();
                                return;
                            }
                            KeyCode::KeyL => {
                                self.session.stop_replay();
                                self.sync_clock();
                                return;
                            }
                            _ => {}
                        }
                    }
                    self.session
                        .set_key(&format!("{key:?}"), pressed, event.repeat);
                }
                WindowEvent::RedrawRequested => {
                    let Some(renderer) = &self.renderer else {
                        return;
                    };
                    if renderer.suspended() {
                        self.reset_clock();
                        return;
                    }
                    let now = Instant::now();
                    if !self.session.paused() {
                        self.accumulated += now
                            .duration_since(self.previous)
                            .min(Duration::from_millis(250));
                    }
                    self.previous = now;
                    let tick = Duration::from_nanos(16_666_667);
                    while self.accumulated >= tick && !self.session.paused() {
                        self.session.tick();
                        self.accumulated -= tick;
                    }
                    if self.session.paused() {
                        self.accumulated = Duration::ZERO;
                    }
                    let result = (|| -> Result<bool, String> {
                        let app = self.session.app();
                        let scene = app
                            .extracted::<Result<RenderFrame3d, Frame3dError>>()
                            .ok_or("missing scene extraction")?
                            .as_ref()
                            .map_err(|e| e.to_string())?;
                        let overlay = app
                            .extracted::<RenderFrame>()
                            .ok_or("missing overlay extraction")?;
                        let assets = app
                            .world()
                            .resource::<ImageAssets>()
                            .ok_or("missing overlay assets")?;
                        self.renderer.as_mut().unwrap().render(
                            scene,
                            BaseColor::rgb(17, 28, 41),
                            overlay,
                            assets,
                        )
                    })();
                    match result {
                        Ok(true) => self.rendered += 1,
                        Ok(false) => {}
                        Err(e) => {
                            self.fail(
                                event_loop,
                                format!("Collection room GPU presentation failed: {e}"),
                            );
                            return;
                        }
                    }
                    if self.limit.is_some_and(|n| self.rendered >= n) {
                        event_loop.exit();
                    }
                }
                _ => {}
            }
        }
    }
}
