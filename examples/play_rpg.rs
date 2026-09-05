//! Native interactive runner; the engine and game remain independent of winit.

#[cfg(not(target_arch = "wasm32"))]
#[path = "support/procedural_rpg.rs"]
pub mod game;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::game;
    use game::live::RpgSession;
    use std::{
        collections::HashSet,
        io::Read,
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    use titan::render::{ImageAssets, RenderFrame};
    use titan::{Startup, inspection::InspectionConfig};
    use titan_protocol::RunMode;
    use titan_remote::{RequestQueue, Server, ServerConfig};
    use titan_render_wgpu::{SurfaceRenderer, wgpu};
    use winit::{
        application::ApplicationHandler,
        dpi::LogicalSize,
        event::{ElementState, MouseButton, WindowEvent},
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        keyboard::{KeyCode, PhysicalKey},
        window::{Window, WindowId},
    };

    struct Player {
        session: RpgSession,
        queue: Option<RequestQueue>,
        stopped: Arc<AtomicBool>,
        started: Instant,
        duration: Option<Duration>,
        clock_epoch: u64,
        held_keys: HashSet<KeyCode>,
        pointer: Option<(f64, f64)>,
        pointer_down: bool,
        shift: bool,
        window: Option<Arc<Window>>,
        renderer: Option<SurfaceRenderer>,
        previous: Instant,
        accumulated: Duration,
        rendered: u64,
        limit: Option<u64>,
        error: Option<String>,
        last_title: String,
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut args = std::env::args().skip(1);
        let mut limit = None;
        let mut assets_dir: Option<std::path::PathBuf> = None;
        let mut generated_assets = false;
        let mut duration = None;
        let mut recording_path = None;
        let mut reference = false;
        let mut inspect = false;
        let mut enable_control = false;
        let mut configured_inspection = false;
        let mut project = std::env::current_dir()?;
        let mut instance = format!("rpg-player-{}", std::process::id());
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--assets-dir" => {
                    assets_dir = Some(
                        args.next()
                            .ok_or("--assets-dir requires a directory")?
                            .into(),
                    )
                }
                "--generated-assets" => generated_assets = true,
                "--frames" => {
                    let count = args
                        .next()
                        .ok_or("--frames requires a positive count")?
                        .parse::<u64>()?;
                    if count == 0 {
                        return Err("--frames must be positive".into());
                    }
                    limit = Some(count);
                }
                "--replay" => reference = true,
                "--recording" => {
                    recording_path = Some(args.next().ok_or("--recording requires a JSON file")?);
                }
                "--inspect" => inspect = true,
                "--allow-control" => {
                    enable_control = true;
                    configured_inspection = true;
                }
                "--instance" => {
                    instance = args.next().ok_or("--instance requires an ID")?;
                    configured_inspection = true;
                }
                "--project" => {
                    project = args.next().ok_or("--project requires a directory")?.into();
                    configured_inspection = true;
                }
                "--run-for-ms" => {
                    let millis = args
                        .next()
                        .ok_or("--run-for-ms requires milliseconds")?
                        .parse::<u64>()?;
                    if millis == 0 {
                        return Err("--run-for-ms must be positive".into());
                    }
                    duration = Some(Duration::from_millis(millis));
                }
                "--help" | "-h" => {
                    println!(
                        "play_rpg [--assets-dir DIR | --generated-assets] [--replay] [--recording PATH] [--frames N] [--run-for-ms MS] [--inspect [--project DIR] [--instance ID] [--allow-control]]\nMove with arrow keys or WASD; J opens the quest journal; arrows/Tab navigate, Enter selects, Escape closes. P pauses/resumes; R restarts; Escape exits when the journal is closed.\nPlayback: P pauses/resumes; N steps one recorded tick while paused; R restarts playback; L exits to a fresh live game.\n--replay renders the completed reference walk.\n--recording loads a bounded JSON recording and starts paused.\n--inspect exposes this player through authenticated local inspection; --allow-control permits remote changes.\n--frames exits after N presented GPU frames; --run-for-ms bounds wall time."
                    );
                    return Ok(());
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }
        if configured_inspection && !inspect {
            return Err("--instance, --project and --allow-control require --inspect".into());
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
        let image = game::assets::load_player(assets_dir.as_deref(), generated_assets)?;
        let recording = recording_path
            .as_deref()
            .map(|path| read_recording(Path::new(path)))
            .transpose()?;
        let project = project.canonicalize()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_signal = stopped.clone();
        ctrlc::set_handler(move || stop_signal.store(true, Ordering::Release))?;
        let (mut server, queue) = if inspect {
            let (server, queue) =
                Server::start(ServerConfig::new(&project, &instance, RunMode::Interactive))?;
            eprintln!(
                "inspecting {} at {} ({})",
                server.registration().instance_id,
                server.registration().endpoint,
                if enable_control {
                    "control enabled"
                } else {
                    "read only"
                }
            );
            (Some(server), Some(queue))
        } else {
            (None, None)
        };
        let mut app = game::build_game_with_player(image);
        app.update_schedule(Startup);
        if reference {
            game::replay(&mut app, &game::recorded_walk());
        }
        let mut config = InspectionConfig::controlled(&instance, project.to_string_lossy());
        config.run_mode = RunMode::Interactive;
        config.mutation_enabled = enable_control;
        let inspector = game::inspector_with_capture(config, |app| {
            titan_diagnostics::png_capture(&game::render_image(app.world())?)
        });
        let mut session = RpgSession::new(app, inspector, enable_control);
        if let Some(recording) = recording {
            session
                .load_replay(recording)
                .map_err(|error| error.message)?;
        } else if !reference {
            session.resume();
        }
        let clock_epoch = session.clock_epoch();
        let mut player = Player {
            session,
            queue,
            stopped,
            started: Instant::now(),
            duration,
            clock_epoch,
            held_keys: HashSet::new(),
            pointer: None,
            pointer_down: false,
            shift: false,
            window: None,
            renderer: None,
            previous: Instant::now(),
            accumulated: Duration::ZERO,
            rendered: 0,
            limit,
            error: None,
            last_title: String::new(),
        };
        EventLoop::new()?.run_app(&mut player)?;
        if let Some(server) = &mut server {
            server.shutdown();
        }
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

    impl ApplicationHandler for Player {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let result = (|| -> Result<(), String> {
                let window = Arc::new(
                    event_loop
                        .create_window(
                            Window::default_attributes()
                                .with_title(self.window_title())
                                .with_inner_size(LogicalSize::new(800.0, 560.0)),
                        )
                        .map_err(|error| error.to_string())?,
                );
                let size = window.inner_size();
                let instance =
                    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
                let surface = instance
                    .create_surface(window.clone())
                    .map_err(|error| error.to_string())?;
                self.renderer = Some(pollster::block_on(SurfaceRenderer::new(
                    &instance,
                    surface,
                    size.width,
                    size.height,
                ))?);
                self.previous = Instant::now();
                window.request_redraw();
                self.window = Some(window);
                Ok(())
            })();
            if let Err(error) = result {
                self.error = Some(error);
                event_loop.exit();
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            // Only this window thread touches the game. Requests run between
            // complete ticks, including while paused or the window is minimized.
            if let Some(queue) = &self.queue {
                queue.drain(|request| self.session.handle(request));
            }
            self.sync_clock();
            self.update_title();
            if self.stopped.load(Ordering::Acquire)
                || self
                    .duration
                    .is_some_and(|limit| self.started.elapsed() >= limit)
            {
                event_loop.exit();
                return;
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => {
                    self.cancel_gestures();
                    if let Some(renderer) = &mut self.renderer {
                        renderer.resize(size.width, size.height);
                    }
                }
                WindowEvent::Focused(false) => {
                    self.cancel_gestures();
                    self.shift = false;
                }
                WindowEvent::ModifiersChanged(modifiers) => {
                    self.shift = modifiers.state().shift_key();
                }
                WindowEvent::CursorMoved { position, .. } => {
                    self.pointer = Some((position.x, position.y));
                    self.send_pointer();
                }
                WindowEvent::CursorLeft { .. } => {
                    self.pointer = None;
                    self.send_pointer();
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    self.pointer_down = state == ElementState::Pressed;
                    self.send_pointer();
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let PhysicalKey::Code(key) = event.physical_key else {
                        return;
                    };
                    if event.state == ElementState::Pressed {
                        let was_open = self.session.journal_open();
                        let journal_key = match key {
                            KeyCode::KeyJ => "toggle",
                            KeyCode::ArrowDown => "next",
                            KeyCode::ArrowUp => "previous",
                            KeyCode::Tab if self.shift => "previous",
                            KeyCode::Tab => "next",
                            KeyCode::Enter | KeyCode::Space => "activate",
                            KeyCode::Escape => "close",
                            _ => "",
                        };
                        if (!event.repeat && self.session.journal_key(journal_key)) || was_open {
                            self.sync_clock();
                            return;
                        }
                    }
                    if key == KeyCode::Escape
                        && event.state == ElementState::Pressed
                        && !event.repeat
                    {
                        event_loop.exit();
                        return;
                    }
                    if event.state == ElementState::Pressed && !event.repeat {
                        if key == KeyCode::KeyP {
                            if self.session.paused() {
                                self.session.resume();
                            } else {
                                self.session.pause();
                            }
                            self.sync_clock();
                            return;
                        }
                        if self.session.replay_active()
                            && key == KeyCode::KeyN
                            && self.session.paused()
                        {
                            if !self.session.replay_status()["complete"]
                                .as_bool()
                                .unwrap_or(false)
                                && let Err(error) = self.session.step_replay()
                            {
                                eprintln!("playback step failed: {}", error.message);
                            }
                            self.sync_clock();
                            return;
                        }
                        if self.session.replay_active() && key == KeyCode::KeyL {
                            self.session.pause();
                            if let Err(error) = self.session.stop_replay() {
                                eprintln!("exit playback failed: {}", error.message);
                            }
                            self.sync_clock();
                            return;
                        }
                        if key == KeyCode::KeyR {
                            if self.session.replay_active() {
                                self.session.pause();
                                if let Err(error) = self.session.restart_replay() {
                                    eprintln!("restart playback failed: {}", error.message);
                                }
                            } else {
                                self.session.restart();
                            }
                            self.sync_clock();
                            return;
                        }
                    }
                    // A key still physically down across pause/focus loss must
                    // not be resurrected by the operating system's repeat event.
                    if self.session.paused()
                        || self.session.replay_active()
                        || self.session.journal_open()
                        || event.repeat
                    {
                        return;
                    }
                    if let Some((action, pressed)) = update_key(
                        &mut self.held_keys,
                        key,
                        event.state == ElementState::Pressed,
                    ) {
                        self.session
                            .set_action(action, pressed)
                            .expect("known game action");
                    }
                }
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    if !self.session.paused() {
                        self.accumulated += now
                            .duration_since(self.previous)
                            .min(Duration::from_millis(250));
                    }
                    self.previous = now;
                    let tick = Duration::from_nanos(16_666_667);
                    while self.accumulated >= tick {
                        self.session.tick();
                        self.accumulated -= tick;
                    }
                    match (|| {
                        let frame = self
                            .session
                            .app()
                            .extracted::<RenderFrame>()
                            .ok_or("game render extraction unavailable")?;
                        let assets = self
                            .session
                            .app()
                            .world()
                            .resource::<ImageAssets>()
                            .ok_or("game image assets unavailable")?;
                        self.renderer.as_mut().unwrap().render(frame, assets)
                    })() {
                        Ok(true) => self.rendered += 1,
                        Ok(false) => {}
                        Err(error) => {
                            self.error = Some(error);
                            event_loop.exit();
                            return;
                        }
                    }
                    if self.limit.is_some_and(|limit| self.rendered >= limit) {
                        event_loop.exit();
                    }
                }
                _ => {}
            }
        }
    }

    impl Player {
        fn cancel_gestures(&mut self) {
            self.held_keys.clear();
            self.pointer_down = false;
            self.session.cancel_journal_input();
        }

        fn send_pointer(&mut self) {
            let point = self.pointer.and_then(|(x, y)| {
                let size = self.window.as_ref()?.inner_size();
                let frame = self.session.app().extracted::<RenderFrame>()?;
                titan::ui::point_from_surface(
                    x,
                    y,
                    f64::from(size.width),
                    f64::from(size.height),
                    frame.width(),
                    frame.height(),
                )
            });
            self.session.journal_pointer(point, self.pointer_down);
            self.sync_clock();
        }

        fn window_title(&self) -> String {
            if self.session.journal_open() {
                "Titan — Quest journal · Arrows/Tab navigate · Enter selects · Escape closes".into()
            } else if self.session.replay_active() {
                let replay = self.session.replay_status();
                let state = if replay["complete"].as_bool().unwrap_or(false) {
                    if replay["verified"].as_bool() == Some(true) {
                        "Complete · MATCH"
                    } else {
                        "Complete · MISMATCH"
                    }
                } else if self.session.paused() {
                    "Paused"
                } else {
                    "Playing"
                };
                format!(
                    "Titan — Playback {}/{} · {state} · P pause · N step · R restart · L live",
                    replay["position"], replay["total"]
                )
            } else if self.session.paused() {
                "Titan — procedural RPG · Paused · P resumes · R restarts".into()
            } else {
                "Titan — procedural RPG · P pauses · R restarts".into()
            }
        }

        fn update_title(&mut self) {
            let title = self.window_title();
            if title != self.last_title {
                if let Some(window) = &self.window {
                    window.set_title(&title);
                }
                self.last_title = title;
            }
        }

        fn sync_clock(&mut self) {
            let epoch = self.session.clock_epoch();
            if epoch != self.clock_epoch {
                self.clock_epoch = epoch;
                self.held_keys.clear();
                self.pointer_down = false;
                self.accumulated = Duration::ZERO;
                self.previous = Instant::now();
            }
        }
    }

    fn read_recording(path: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        const MAX_BYTES: u64 = 2 * 1024 * 1024;
        let metadata = path.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Err("recording must be a regular JSON file no larger than 2 MiB".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("recording exceeds the 2 MiB size bound".into());
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn action_for_key(key: KeyCode) -> Option<&'static str> {
        match key {
            KeyCode::ArrowUp | KeyCode::KeyW => Some("up"),
            KeyCode::ArrowDown | KeyCode::KeyS => Some("down"),
            KeyCode::ArrowLeft | KeyCode::KeyA => Some("left"),
            KeyCode::ArrowRight | KeyCode::KeyD => Some("right"),
            _ => None,
        }
    }

    fn update_key(
        held: &mut HashSet<KeyCode>,
        key: KeyCode,
        pressed: bool,
    ) -> Option<(&'static str, bool)> {
        titan::input::update_button_alias(held, key, pressed, action_for_key)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn idempotent_pause_cancels_native_pointer_before_motion_and_release() {
            let mut app = game::build_game();
            app.update_schedule(Startup);
            let inspector = game::inspector_with_capture(
                InspectionConfig::controlled("native-test", "rpg"),
                |app| titan_diagnostics::png_capture(&game::render_image(app.world())?),
            );
            let session = RpgSession::new(app, inspector, true);
            let mut player = Player {
                clock_epoch: session.clock_epoch(),
                session,
                queue: None,
                stopped: Arc::new(AtomicBool::new(false)),
                started: Instant::now(),
                duration: None,
                held_keys: HashSet::new(),
                pointer: Some((5.0, 5.0)),
                pointer_down: true,
                shift: false,
                window: None,
                renderer: None,
                previous: Instant::now(),
                accumulated: Duration::from_millis(12),
                rendered: 0,
                limit: None,
                error: None,
                last_title: String::new(),
            };
            assert!(player.session.paused());
            player.session.journal_pointer(Some((5, 5)), true);
            player.session.pause();
            player.sync_clock();
            assert!(!player.pointer_down);
            assert_eq!(player.accumulated, Duration::ZERO);
            // A motion sample uses the host's synchronized held state. It must
            // not recreate the canceled press before the physical release.
            player
                .session
                .journal_pointer(Some((5, 5)), player.pointer_down);
            player.session.journal_pointer(Some((5, 5)), false);
            assert!(!player.session.journal_open());
        }

        #[test]
        fn recording_reader_rejects_non_files_and_oversized_files() {
            assert!(read_recording(&std::env::temp_dir()).is_err());
            let path = std::env::temp_dir().join(format!(
                "titan-player-recording-{}.json",
                std::process::id()
            ));
            std::fs::write(&path, b"{}").unwrap();
            assert_eq!(read_recording(&path).unwrap(), serde_json::json!({}));
            std::fs::File::create(&path)
                .unwrap()
                .set_len(2 * 1024 * 1024 + 1)
                .unwrap();
            assert!(read_recording(&path).is_err());
            std::fs::remove_file(path).unwrap();
        }

        #[test]
        fn releasing_one_keyboard_alias_keeps_the_other_held() {
            let mut held = HashSet::new();
            assert_eq!(
                update_key(&mut held, KeyCode::KeyW, true),
                Some(("up", true))
            );
            assert_eq!(
                update_key(&mut held, KeyCode::ArrowUp, true),
                Some(("up", true))
            );
            assert_eq!(
                update_key(&mut held, KeyCode::KeyW, false),
                Some(("up", true))
            );
            assert_eq!(
                update_key(&mut held, KeyCode::ArrowUp, false),
                Some(("up", false))
            );
            assert_eq!(update_key(&mut held, KeyCode::KeyQ, true), None);
            assert!(held.is_empty());
        }
    }
}
