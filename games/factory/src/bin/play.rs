//! Native interactive runner; the engine and game remain independent of winit.

#[cfg(not(target_arch = "wasm32"))]
use titan_factory::game;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::game;
    use std::{
        collections::HashSet,
        sync::Arc,
        time::{Duration, Instant},
    };
    use titan::{
        App, Startup,
        render::{ImageAssets, RenderFrame},
    };
    use titan_render_wgpu::{SurfaceRenderer, wgpu};
    use winit::{
        application::ApplicationHandler,
        dpi::LogicalSize,
        event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
        event_loop::{ActiveEventLoop, EventLoop},
        keyboard::{KeyCode, PhysicalKey},
        window::{Window, WindowId},
    };

    struct Player {
        app: App,
        input: game::InteractiveInput,
        held_keys: HashSet<KeyCode>,
        window: Option<Arc<Window>>,
        renderer: Option<SurfaceRenderer>,
        previous: Instant,
        accumulated: Duration,
        rendered: u64,
        limit: Option<u64>,
        error: Option<String>,
        cursor: Option<(f64, f64)>,
        feedback: String,
        test_transport: bool,
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut args = std::env::args().skip(1);
        let mut limit = None;
        let mut test_construction = false;
        let mut test_transport = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--test-transport" => test_transport = true,
                "--test-construction" => {
                    test_construction = true;
                }
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
                "--help" | "-h" => {
                    println!(
                        "play [--frames N] [--test-construction | --test-transport] \nWASD/arrows pan; wheel zoom; 1/2/3 conveyor/extractor/processor; Q facing; E rotate; X remove; click place; right click inspect; R restart; Escape exits.\n--frames exits after N presented GPU frames."
                    );
                    return Ok(());
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }
        if test_transport && test_construction {
            return Err("choose one acceptance fixture".into());
        }
        let mut app = if test_transport {
            game::build_transport_fixture("cycle_partial")?
        } else {
            game::build_game()
        };
        app.update_schedule(Startup);
        if test_construction {
            construction_acceptance(&mut app)?;
            limit = limit.or(Some(2));
        }
        if test_transport {
            limit = limit.or(Some(240));
        }
        let input = game::InteractiveInput::for_app(&app);
        let mut player = Player {
            app,
            input,
            held_keys: HashSet::new(),
            window: None,
            renderer: None,
            previous: Instant::now(),
            accumulated: Duration::ZERO,
            rendered: 0,
            limit,
            error: None,
            cursor: None,
            feedback: String::new(),
            test_transport,
        };
        EventLoop::new()?.run_app(&mut player)?;
        if let Some(error) = player.error {
            return Err(error.into());
        }
        println!(
            "rendered {} GPU frames; {}",
            player.rendered,
            game::status(&player.app)
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
                                .with_title(
                                    "Titan Factory | ore (1,3) → processor → 10 plates at (10,3)",
                                )
                                .with_inner_size(LogicalSize::new(1152.0, 768.0)),
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

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.resize(size.width, size.height);
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    self.cursor = Some((position.x, position.y));
                    self.pointer("hover");
                }
                WindowEvent::CursorLeft { .. } => {
                    self.cursor = None;
                    let _ = game::pointer(&mut self.app, -1.0, -1.0, "hover");
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button,
                    ..
                } => match button {
                    MouseButton::Left => self.pointer("place"),
                    MouseButton::Right => self.pointer("inspect"),
                    _ => {}
                },
                WindowEvent::MouseWheel { delta, .. } => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                        MouseScrollDelta::PixelDelta(p) => p.y / 100.0,
                    };
                    if let Err(error) = game::camera(&mut self.app, 0.0, 0.0, (dy * 0.12).exp()) {
                        self.feedback = error;
                    }
                }
                WindowEvent::Focused(false) => {
                    self.held_keys.clear();
                    self.input = game::InteractiveInput::for_app(&self.app);
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let PhysicalKey::Code(key) = event.physical_key else {
                        return;
                    };
                    if key == KeyCode::Escape && event.state == ElementState::Pressed {
                        event_loop.exit();
                        return;
                    }
                    if event.state == ElementState::Pressed && !event.repeat {
                        let kind = match key {
                            KeyCode::Digit1 => Some("conveyor"),
                            KeyCode::Digit2 => Some("extractor"),
                            KeyCode::Digit3 => Some("processor"),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            self.command(serde_json::json!({"op":"select","kind":kind}));
                        }
                        match key {
                            KeyCode::KeyQ => {
                                let status: serde_json::Value =
                                    serde_json::from_str(&game::status(&self.app)).unwrap();
                                let facing = match status["selection"]["facing"].as_str() {
                                    Some("N") => "E",
                                    Some("E") => "S",
                                    Some("S") => "W",
                                    _ => "N",
                                };
                                self.command(serde_json::json!({"op":"select","facing":facing}));
                            }
                            KeyCode::KeyE => self.pointer("rotate"),
                            KeyCode::KeyX | KeyCode::Delete => self.pointer("remove"),
                            _ => {}
                        }
                    }
                    if key == KeyCode::KeyR && event.state == ElementState::Pressed && !event.repeat
                    {
                        game::restart(&mut self.app);
                        self.held_keys.clear();
                        self.input = game::InteractiveInput::for_app(&self.app);
                        self.accumulated = Duration::ZERO;
                        self.feedback.clear();
                    }
                    if let Some((action, pressed)) = update_key(
                        &mut self.held_keys,
                        key,
                        event.state == ElementState::Pressed,
                        event.repeat,
                    ) {
                        self.input
                            .set_action(&self.app, action, pressed)
                            .expect("known movement action");
                    }
                }
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    self.accumulated += now
                        .duration_since(self.previous)
                        .min(Duration::from_millis(250));
                    self.previous = now;
                    let tick = Duration::from_nanos(16_666_667);
                    if self.test_transport {
                        // Deliberately hold each state for 30 presented frames so a
                        // reviewer can inspect movement around all four corners.
                        self.accumulated = Duration::ZERO;
                    }
                    while self.accumulated >= tick {
                        self.input.tick(&mut self.app);
                        self.accumulated -= tick;
                    }
                    if let Some(window) = &self.window {
                        let status: serde_json::Value =
                            serde_json::from_str(&game::status(&self.app)).unwrap();
                        window.set_title(&format!(
                            "Ore(1,3) → processor → 10 plates(10,3) | {} {} | {}",
                            status["selection"]["kind"].as_str().unwrap_or(""),
                            status["selection"]["facing"].as_str().unwrap_or(""),
                            self.feedback
                        ));
                    }
                    match (|| {
                        let frame = self
                            .app
                            .extracted::<RenderFrame>()
                            .ok_or("game render extraction unavailable")?;
                        let assets = self
                            .app
                            .world()
                            .resource::<ImageAssets>()
                            .ok_or("game image assets unavailable")?;
                        self.renderer.as_mut().unwrap().render(frame, assets)
                    })() {
                        Ok(true) => {
                            if self.test_transport && self.rendered.is_multiple_of(30) {
                                println!(
                                    "{}",
                                    serde_json::json!({"native_transport_presented": true, "state": serde_json::from_str::<serde_json::Value>(&game::status(&self.app)).unwrap()})
                                );
                            }
                            self.rendered += 1;
                            if self.test_transport
                                && self.rendered.is_multiple_of(30)
                                && self.limit.is_none_or(|limit| self.rendered < limit)
                            {
                                self.input.tick(&mut self.app);
                            }
                        }
                        Ok(false) => {}
                        Err(error) => {
                            self.error = Some(error);
                            event_loop.exit();
                            return;
                        }
                    }
                    if self.limit.is_some_and(|limit| self.rendered >= limit) {
                        event_loop.exit();
                    } else {
                        self.window.as_ref().unwrap().request_redraw();
                    }
                }
                _ => {}
            }
        }
    }

    impl Player {
        fn command(&mut self, value: serde_json::Value) {
            self.feedback = feedback(game::player_command(&mut self.app, &value.to_string()));
        }
        fn pointer(&mut self, action: &str) {
            let Some((x, y)) = self.cursor else {
                return;
            };
            let Some(window) = &self.window else {
                return;
            };
            let size = window.inner_size();
            if let Some((x, y)) = logical_pointer(x, y, size.width, size.height) {
                let result = game::pointer(&mut self.app, x, y, action);
                if action != "hover" {
                    self.feedback = feedback(result);
                }
            }
        }
    }

    fn feedback(result: Result<String, String>) -> String {
        let text = match result {
            Ok(text) => text,
            Err(error) => return error,
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return text;
        };
        if let Some(structure) = value.get("structure") {
            if structure.is_null() {
                if let (Some(ore), Some(plate)) = (
                    value
                        .get("discarded_ore")
                        .and_then(serde_json::Value::as_u64),
                    value
                        .get("discarded_plate")
                        .and_then(serde_json::Value::as_u64),
                ) {
                    return format!(
                        "Removed tile ({},{}); discarded {ore} ore, {plate} plate",
                        value["x"], value["y"]
                    );
                }
                return format!("Tile ({},{}) empty", value["x"], value["y"]);
            }
            return format!(
                "{} ({},{}) {} inputs:{} output:{}",
                structure["kind"].as_str().unwrap_or(""),
                value["x"],
                value["y"],
                structure["facing"].as_str().unwrap_or(""),
                structure["inputs"],
                structure["output"]
            );
        }
        text
    }

    fn construction_acceptance(app: &mut App) -> Result<(), String> {
        for (kind, x) in [("extractor", 1), ("conveyor", 2), ("processor", 3)] {
            game::player_command(
                app,
                &serde_json::json!({"op":"select","kind":kind}).to_string(),
            )?;
            let (px, py) = logical_pointer(f64::from(x * 96 + 48), 336.0, 1152, 768).unwrap();
            game::pointer(app, px, py, "place")?;
        }
        if game::pointer(app, 112.0, 112.0, "place").is_ok() {
            return Err("occupied placement unexpectedly accepted".into());
        }
        game::pointer(app, 80.0, 112.0, "rotate")?;
        let state: serde_json::Value = serde_json::from_str(&game::status(app)).unwrap();
        if !state["structures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["x"] == 2 && s["y"] == 3 && s["facing"] == "S")
        {
            return Err("rotation did not preserve tile and change east to south".into());
        }
        game::pointer(app, 80.0, 112.0, "inspect")?;
        game::pointer(app, 80.0, 112.0, "remove")?;
        if game::pointer(app, 336.0, 112.0, "remove").is_ok() {
            return Err("fixed delivery removal unexpectedly accepted".into());
        }
        game::camera(app, 16.0, 8.0, 1.5)?;
        game::player_command(app, r#"{"op":"select","kind":"conveyor"}"#)?;
        // Tile (6,4) under centered zoom and pan; simulate a resized, Retina surface.
        let screen_x = (208.0 - 192.0) * 1.5 + 192.0 + 16.0;
        let screen_y = (144.0 - 128.0) * 1.5 + 128.0 + 8.0;
        let (px, py) = logical_pointer(
            screen_x * 1440.0 / 384.0,
            screen_y * 840.0 / 256.0,
            1440,
            840,
        )
        .unwrap();
        game::pointer(app, px, py, "place")?;
        let state: serde_json::Value = serde_json::from_str(&game::status(app)).unwrap();
        if !state["structures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["x"] == 6 && s["y"] == 4 && s["kind"] == "conveyor")
        {
            return Err("camera/resize pointer placement targeted the wrong tile".into());
        }
        println!(
            "{}",
            serde_json::json!({"native_construction_acceptance":"passed", "state":state})
        );
        Ok(())
    }

    // SurfaceRenderer scales the logical framebuffer across the full physical surface.
    fn logical_pointer(x: f64, y: f64, width: u32, height: u32) -> Option<(f64, f64)> {
        if width == 0
            || height == 0
            || !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= f64::from(width)
            || y >= f64::from(height)
        {
            return None;
        }
        Some((
            x * game::WIDTH as f64 / f64::from(width),
            y * game::HEIGHT as f64 / f64::from(height),
        ))
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
        repeat: bool,
    ) -> Option<(&'static str, bool)> {
        // Held movement is sampled by fixed ticks. Repeated keydown events must
        // not resurrect a key cleared by restart or focus loss.
        if pressed && repeat {
            return None;
        }
        titan::input::update_button_alias(held, key, pressed, action_for_key)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn removal_feedback_reports_item_discards_and_distinguishes_empty_inspection() {
            let mut app = game::build_transport_fixture("disconnected").unwrap();
            for (x, y, expected) in [
                (0, 0, "Removed tile (0,0); discarded 1 ore, 0 plate"),
                (6, 5, "Removed tile (6,5); discarded 0 ore, 1 plate"),
            ] {
                let result = game::player_command(
                    &mut app,
                    &serde_json::json!({"op":"remove","x":x,"y":y}).to_string(),
                );
                assert_eq!(feedback(result), expected);
            }
            game::player_command(
                &mut app,
                r#"{"op":"place","kind":"conveyor","x":0,"y":0,"facing":"E"}"#,
            )
            .unwrap();
            assert_eq!(
                feedback(game::player_command(
                    &mut app,
                    r#"{"op":"remove","x":0,"y":0}"#
                )),
                "Removed tile (0,0); discarded 0 ore, 0 plate"
            );
            assert_eq!(
                feedback(game::player_command(
                    &mut app,
                    r#"{"op":"inspect","x":0,"y":0}"#
                )),
                "Tile (0,0) empty"
            );
        }

        #[test]
        fn pointer_mapping_handles_resize_and_retina_without_rounding_tiles() {
            assert_eq!(
                logical_pointer(192.0, 128.0, 384, 256),
                Some((192.0, 128.0))
            );
            assert_eq!(
                logical_pointer(576.0, 384.0, 1152, 768),
                Some((192.0, 128.0))
            );
            assert_eq!(logical_pointer(200.0, 150.0, 800, 600), Some((96.0, 64.0)));
            assert_eq!(logical_pointer(800.0, 0.0, 800, 600), None);
            assert_eq!(logical_pointer(0.0, 0.0, 0, 600), None);
            assert_eq!(logical_pointer(f64::NAN, 0.0, 800, 600), None);
        }

        #[test]
        fn repeat_after_restart_cannot_restore_old_movement_but_new_press_can() {
            let mut app = game::build_game();
            let mut input = game::InteractiveInput::for_app(&app);
            let mut held = HashSet::new();
            let (action, pressed) = update_key(&mut held, KeyCode::KeyD, true, false).unwrap();
            input.set_action(&app, action, pressed).unwrap();
            input.tick(&mut app);
            game::restart(&mut app);
            held.clear();
            input = game::InteractiveInput::for_app(&app);
            assert_eq!(update_key(&mut held, KeyCode::KeyD, true, true), None);
            assert!(held.is_empty());
            input.tick(&mut app);
            let state: serde_json::Value = serde_json::from_str(&game::status(&app)).unwrap();
            assert_eq!(state["camera"]["x"], 0.0);
            // Release the old physical key, then a genuinely new keydown works
            // on its first fixed tick, including the other movement alias.
            update_key(&mut held, KeyCode::KeyD, false, false);
            let (action, pressed) =
                update_key(&mut held, KeyCode::ArrowRight, true, false).unwrap();
            input.set_action(&app, action, pressed).unwrap();
            input.tick(&mut app);
            let state: serde_json::Value = serde_json::from_str(&game::status(&app)).unwrap();
            assert_eq!(state["camera"]["x"], -4.0);
        }

        #[test]
        fn releasing_one_keyboard_alias_keeps_the_other_held() {
            let mut held = HashSet::new();
            assert_eq!(
                update_key(&mut held, KeyCode::KeyW, true, false),
                Some(("up", true))
            );
            assert_eq!(
                update_key(&mut held, KeyCode::ArrowUp, true, false),
                Some(("up", true))
            );
            assert_eq!(
                update_key(&mut held, KeyCode::KeyW, false, false),
                Some(("up", true))
            );
            assert_eq!(
                update_key(&mut held, KeyCode::ArrowUp, false, false),
                Some(("up", false))
            );
            assert_eq!(update_key(&mut held, KeyCode::Space, true, false), None);
            assert!(held.is_empty());
        }
    }
}
