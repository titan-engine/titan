//! Native window and authenticated inspection of the same adventure App.
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
            three_d::{Frame3dError, RenderFrame3d},
        },
    };
    use titan_adventure::{game, player::PlayerSession};
    use titan_protocol::RunMode;
    use titan_remote::{RequestQueue, Server, ServerConfig};
    use titan_render_wgpu::{SurfaceRenderer3d, wgpu};
    use winit::{
        application::ApplicationHandler,
        dpi::{LogicalSize, PhysicalSize},
        event::{ElementState, WindowEvent},
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        keyboard::{KeyCode, PhysicalKey},
        window::{Window, WindowId},
    };
    struct Player {
        session: PlayerSession,
        captures: titan_adventure::capture::CaptureQueue,
        queue: Option<RequestQueue>,
        stopped: Arc<AtomicBool>,
        started: Instant,
        duration: Option<Duration>,
        limit: Option<u64>,
        rendered: u64,
        window: Option<Arc<Window>>,
        renderer: Option<SurfaceRenderer3d>,
        capture_device: Option<(wgpu::Device, wgpu::Queue)>,
        previous: Instant,
        accumulated: Duration,
        epoch: u64,
        error: Option<String>,
        verify_surface_lifecycle: bool,
        lifecycle_resize_observed: bool,
        lifecycle_verified: bool,
    }
    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut args = std::env::args().skip(1);
        let mut verify_surface_lifecycle = false;
        let (mut limit, mut duration, mut recording) = (None, None, None);
        let (mut inspect, mut allow_control, mut configured, mut start_paused) =
            (false, false, false, false);
        let mut project = std::env::current_dir()?;
        let mut instance = format!("adventure-player-{}", std::process::id());
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
                "--verify-surface-lifecycle" => verify_surface_lifecycle = true,
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
                        "play [--paused] [--verify-surface-lifecycle] [--recording PATH] [--frames N] [--run-for-ms MS] [--inspect [--allow-control] [--project DIR] [--instance ID]]\nWASD/arrows move; Q switch; P pause/resume; N single tick while paused; R restart; L leave replay; Escape quit.\nRecordings start paused and replay actual fixed ticks. --inspect attaches authenticated local inspection to this played instance; remote control requires --allow-control. Captures freeze a fresh 960x540 scene and ECS overlay without advancing a tick.\n--frames counts successfully presented GPU frames; --run-for-ms bounds wall time."
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
        let captures = titan_adventure::capture::CaptureQueue::install(&mut session);
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
            captures,
            queue,
            stopped,
            started: Instant::now(),
            duration,
            limit,
            rendered: 0,
            window: None,
            renderer: None,
            capture_device: None,
            previous: Instant::now(),
            accumulated: Duration::ZERO,
            epoch,
            error: None,
            verify_surface_lifecycle,
            lifecycle_resize_observed: false,
            lifecycle_verified: false,
        };
        let result = EventLoop::new()?.run_app(&mut player);
        if let Some(server) = &mut server {
            server.shutdown();
        }
        result?;
        if let Some(error) = player.error {
            return Err(error.into());
        }
        if player.verify_surface_lifecycle && !player.lifecycle_verified {
            return Err("surface lifecycle verification did not finish before shutdown".into());
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
        fn present(&mut self) -> Result<bool, String> {
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
            self.renderer
                .as_mut()
                .ok_or("GPU renderer unavailable")?
                .render(
                    scene,
                    titan_adventure::player::CAPTURE_CLEAR,
                    overlay,
                    assets,
                )
        }
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
            let size_hint = self
                .window
                .as_ref()
                .filter(|w| {
                    let s = w.inner_size();
                    s.width < 960 || s.height < 540
                })
                .map(|_| " · Recommended 960x540+")
                .unwrap_or("");
            format!(
                "Titan — Adventure · {}{}{} · Q switch · P pause · R restart{size_hint}",
                state["active_character"],
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
                self.renderer
                    .as_mut()
                    .unwrap()
                    .set_aspect_ratio(Some((16, 9)))?;
                eprintln!(
                    "native GPU adapter: {:?}",
                    self.renderer.as_ref().unwrap().adapter_info()
                );
                self.capture_device = Some(self.renderer.as_ref().unwrap().capture_device());
                self.window = Some(window);
                if self.verify_surface_lifecycle && !self.lifecycle_verified {
                    let before = game::status(self.session.app());
                    self.renderer.as_mut().unwrap().resize(0, 0);
                    if !self.renderer.as_ref().unwrap().suspended() || self.present()? {
                        return Err("zero-size surface must suspend and skip presentation".into());
                    }
                    self.renderer
                        .as_mut()
                        .unwrap()
                        .resize(size.width, size.height);
                    if self.renderer.as_ref().unwrap().suspended() {
                        return Err("nonzero surface did not resume".into());
                    }
                    if game::status(self.session.app()) != before {
                        return Err("surface lifecycle changed simulation state".into());
                    }
                    eprintln!(
                        "surface lifecycle: zero-size presentation skipped; nonzero restored; simulation unchanged"
                    );
                    let _ = self
                        .window
                        .as_ref()
                        .unwrap()
                        .request_inner_size(PhysicalSize::new(800, 500));
                }
                self.reset_clock();
                Ok(())
            })();
            if let Err(e) = result {
                self.fail(
                    event_loop,
                    format!("Adventure GPU initialization failed: {e}"),
                );
            }
        }
        fn suspended(&mut self, _: &ActiveEventLoop) {
            self.session.pause();
            self.renderer = None;
            self.reset_clock();
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(queue) = &self.queue {
                queue.drain_with_reply(|request, reply| {
                    let started = Instant::now();
                    let dispatch = self.session.dispatch(request);
                    if let Some((device, queue)) = &self.capture_device {
                        self.captures.start(device.clone(), queue.clone());
                    }
                    match dispatch {
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
                    if self.verify_surface_lifecycle && size.width == 800 && size.height == 500 {
                        self.lifecycle_resize_observed = true;
                    }
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
                            KeyCode::KeyR if self.session.paused() => {
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
                    let result = self.present();
                    match result {
                        Ok(true) => {
                            self.rendered += 1;
                            if self.verify_surface_lifecycle
                                && self.lifecycle_resize_observed
                                && !self.lifecycle_verified
                            {
                                self.lifecycle_verified = true;
                                eprintln!(
                                    "surface lifecycle verified: OS resize 800x500 presented successfully"
                                );
                            }
                        }
                        Ok(false) => {}
                        Err(e) => {
                            self.fail(
                                event_loop,
                                format!("Adventure GPU presentation failed: {e}"),
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
