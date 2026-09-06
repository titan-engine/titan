//! Native interactive runner; the engine and game remain independent of winit.

#[cfg(not(target_arch = "wasm32"))]
use titan_factory::game;
#[cfg(not(target_arch = "wasm32"))]
mod native_ui;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{game, native_ui};
    use std::{
        collections::HashSet,
        sync::Arc,
        time::{Duration, Instant},
    };
    use titan::{App, Startup};
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
        test_production: bool,
        test_interface: bool,
        ui: native_ui::Interface,
        paused: bool,
        tool: String,
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut args = std::env::args().skip(1);
        let mut limit = None;
        let mut test_construction = false;
        let mut test_transport = false;
        let mut test_production = false;
        let mut test_interface = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--test-interface" => {
                    test_interface = true;
                    limit = Some(240);
                }
                "--test-transport" => test_transport = true,
                "--test-production" => test_production = true,
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
                        "play [--frames N] [--test-interface | --test-construction | --test-transport | --test-production] \nWASD/arrows pan; wheel over grid zooms; Tab/Enter focus/activate UI; Space pause; period step; 1/2/3 conveyor/extractor/processor; 4/5/6 inspect/rotate/remove; Q facing; E rotate; X remove; click place; right click inspect; R restart; Escape exits.\n--frames exits after N presented GPU frames."
                    );
                    return Ok(());
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }
        if [
            test_transport,
            test_construction,
            test_production,
            test_interface,
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
            > 1
        {
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
        if test_production {
            build_production_route(&mut app)?;
            limit = limit.or(Some(1500));
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
            test_production,
            test_interface,
            ui: native_ui::Interface::new(),
            paused: false,
            tool: "conveyor".into(),
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
                                .with_inner_size(LogicalSize::new(1200.0, 840.0))
                                .with_min_inner_size(LogicalSize::new(1000.0, 700.0)),
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
                if self.test_interface {
                    self.interface_acceptance()?;
                }
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
                    MouseButton::Left => self.pointer("primary"),
                    MouseButton::Right => self.pointer("inspect"),
                    _ => {}
                },
                WindowEvent::MouseWheel { delta, .. } => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                        MouseScrollDelta::PixelDelta(p) => p.y / 100.0,
                    };
                    if !self
                        .cursor
                        .and_then(|(x, y)| {
                            let z = self.window.as_ref()?.inner_size();
                            native_ui::surface(x, y, z.width, z.height)
                                .and_then(|(x, y)| native_ui::world_point(x, y))
                        })
                        .is_some()
                    {
                        return;
                    }
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
                            self.ui_action(kind);
                        }
                        match key {
                            KeyCode::Tab => self.ui.focus_next(),
                            KeyCode::Enter => {
                                if let Some(action) = self.ui.focused_action() {
                                    self.ui_action(action);
                                }
                            }
                            KeyCode::KeyQ => self.ui_action("facing"),
                            KeyCode::Digit4 => self.ui_action("inspect"),
                            KeyCode::Digit5 => self.ui_action("rotate"),
                            KeyCode::Digit6 => self.ui_action("remove"),
                            KeyCode::Space => self.ui_action("pause"),
                            KeyCode::Period => self.ui_action("step"),
                            KeyCode::KeyE => self.pointer("rotate"),
                            KeyCode::KeyX | KeyCode::Delete => self.pointer("remove"),
                            _ => {}
                        }
                    }
                    if key == KeyCode::KeyR && event.state == ElementState::Pressed && !event.repeat
                    {
                        self.ui_action("restart");
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
                    let elapsed = now
                        .duration_since(self.previous)
                        .min(Duration::from_millis(250));
                    self.accumulated += elapsed;
                    if self.paused {
                        let held =
                            |a, b| self.held_keys.contains(&a) || self.held_keys.contains(&b);
                        let dx = i32::from(held(KeyCode::KeyA, KeyCode::ArrowLeft))
                            - i32::from(held(KeyCode::KeyD, KeyCode::ArrowRight));
                        let dy = i32::from(held(KeyCode::KeyW, KeyCode::ArrowUp))
                            - i32::from(held(KeyCode::KeyS, KeyCode::ArrowDown));
                        if dx != 0 || dy != 0 {
                            let speed = elapsed.as_secs_f64() * 240.;
                            let _ = game::camera(
                                &mut self.app,
                                f64::from(dx) * speed,
                                f64::from(dy) * speed,
                                1.,
                            );
                            self.pointer("hover");
                        }
                    }
                    self.previous = now;
                    let tick = Duration::from_nanos(16_666_667);
                    if self.paused || self.test_transport || self.test_production {
                        // Deliberately hold each state for 30 presented frames so a
                        // reviewer can inspect movement around all four corners.
                        self.accumulated = Duration::ZERO;
                    }
                    while self.accumulated >= tick {
                        self.input.tick(&mut self.app);
                        self.accumulated -= tick;
                        if game::interface(&self.app)["outcome"] == "Complete" {
                            self.clear_input();
                            break;
                        }
                    }
                    if let Some(window) = &self.window {
                        window.set_title("Titan Factory - Build and diagnose");
                    }
                    match (|| {
                        let frame =
                            self.ui
                                .frame(&self.app, self.paused, &self.tool, &self.feedback)?;
                        self.renderer
                            .as_mut()
                            .unwrap()
                            .render(&frame, self.ui.assets())
                    })() {
                        Ok(true) => {
                            if self.test_production {
                                if let Err(error) = verify_production_frame(&self.app) {
                                    self.error = Some(error);
                                    event_loop.exit();
                                    return;
                                }
                                if [0, 60, 64, 124, 184, 189, 1269].contains(&self.rendered) {
                                    println!(
                                        "{}",
                                        serde_json::json!({"native_production_presented":true,"state":serde_json::from_str::<serde_json::Value>(&game::status(&self.app)).unwrap()})
                                    );
                                }
                                self.input.tick(&mut self.app);
                            }

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
        fn interface_acceptance(&mut self) -> Result<(), String> {
            self.ui.frame(&self.app, self.paused, &self.tool, "")?;
            self.ui_action("pause");
            let count = game::interface(&self.app)["structures"]
                .as_array()
                .unwrap()
                .len();
            self.pointer_at(130., 45., "primary");
            if self.tool != "extractor"
                || game::interface(&self.app)["structures"]
                    .as_array()
                    .unwrap()
                    .len()
                    != count
            {
                return Err("palette click leaked into world".into());
            }
            self.ui.focus_next();
            if self.ui.focused_action().is_none() {
                return Err("keyboard UI focus unavailable".into());
            }
            self.build_ui_route();
            self.pointer_at(12. + 5. * 32. + 16., 92. + 3. * 32. + 16., "rotate");
            self.pointer_at(12. + 4. * 32. + 16., 92. + 3. * 32. + 16., "inspect");
            let broken = game::interface(&self.app);
            if broken["inspected"]["structure"]["connection"]["code"] != "wrong_facing" {
                return Err(format!("missing wrong facing diagnosis {broken}"));
            }
            for _ in 0..3 {
                self.pointer_at(12. + 5. * 32. + 16., 92. + 3. * 32. + 16., "rotate");
            }
            for _ in 0..1269 {
                self.ui_action("step");
            }
            let complete = game::interface(&self.app);
            if complete["outcome"] != "Complete" || complete["delivered"] != 10 {
                return Err(format!("native UI repaired route failed {complete}"));
            }
            self.ui_action("step");
            if game::interface(&self.app)["tick"] != 1269 {
                return Err("completed step advanced simulation".into());
            }
            self.ui_action("restart");
            if !self.held_keys.is_empty() || game::interface(&self.app)["tick"] != 0 {
                return Err("restart did not reset input and tick".into());
            }
            self.ui_action("pause");
            self.build_ui_route();
            self.pointer_at(12. + 5. * 32. + 16., 92. + 3. * 32. + 16., "rotate");
            for _ in 0..65 {
                self.ui_action("step");
            }
            self.pointer_at(12. + 4. * 32. + 16., 92. + 3. * 32. + 16., "inspect");
            self.ui_action("inspect");
            self.feedback="Acceptance passed. Inspect belt (4,3): rotate processor (5,3) to face east to repair.".into();
            println!(
                "{}",
                serde_json::json!({"native_interface_acceptance":"passed","palette_blocks_world":true,"keyboard_focus":true,"wrong_facing_diagnosed":broken["inspected"],"repair_completed_tick":complete["tick"],"pause_step_restart":true,"state":game::interface(&self.app)})
            );
            Ok(())
        }
        fn build_ui_route(&mut self) {
            for (kind, xs) in [
                ("extractor", vec![1]),
                ("processor", vec![5]),
                ("conveyor", vec![2, 3, 4, 6, 7, 8, 9]),
            ] {
                self.ui_action(kind);
                for x in xs {
                    self.pointer_at(
                        12. + f64::from(x) * 32. + 16.,
                        92. + 3. * 32. + 16.,
                        "primary",
                    );
                }
            }
        }
        fn clear_input(&mut self) {
            self.held_keys.clear();
            self.input = game::InteractiveInput::for_app(&self.app);
            self.accumulated = Duration::ZERO;
        }
        fn ui_action(&mut self, action: &str) {
            match action {
                "conveyor" | "extractor" | "processor" => {
                    self.tool = action.into();
                    self.command(serde_json::json!({"op":"select","kind":action}));
                    let _ = game::set_preview_action(&mut self.app, "place");
                }
                "inspect" | "rotate" | "remove" => {
                    self.tool = action.into();
                    let _ = game::set_preview_action(&mut self.app, action);
                }
                "facing" => {
                    let state = game::interface(&self.app);
                    let facing = match state["selection"]["facing"].as_str() {
                        Some("N") => "E",
                        Some("E") => "S",
                        Some("S") => "W",
                        _ => "N",
                    };
                    self.command(serde_json::json!({"op":"select","facing":facing}));
                }
                "pause" => {
                    self.paused = !self.paused;
                    self.clear_input();
                }
                "step" => {
                    self.paused = true;
                    self.clear_input();
                    self.input.tick(&mut self.app);
                }
                "restart" => {
                    self.test_production = false;
                    self.test_transport = false;
                    game::restart(&mut self.app);
                    self.clear_input();
                    self.paused = false;
                    self.tool = "conveyor".into();
                    self.feedback = "Restarted. All inventories and discarded counts reset.".into();
                }
                _ => {}
            }
        }
        fn pointer(&mut self, action: &str) {
            let Some((x, y)) = self.cursor else {
                return;
            };
            let Some(window) = &self.window else {
                return;
            };
            let size = window.inner_size();
            let Some((x, y)) = native_ui::surface(x, y, size.width, size.height) else {
                return;
            };
            self.pointer_at(x, y, action);
        }
        fn pointer_at(&mut self, x: f64, y: f64, action: &str) {
            if let Some(button) = self.ui.hit(x, y) {
                if action == "primary" {
                    self.ui_action(button);
                }
                let _ = game::pointer(&mut self.app, -1., -1., "hover");
                return;
            }
            let Some((x, y)) = native_ui::world_point(x, y) else {
                let _ = game::pointer(&mut self.app, -1., -1., "hover");
                return;
            };
            let action = if action == "primary" {
                match self.tool.as_str() {
                    "inspect" => "inspect",
                    "rotate" => "rotate",
                    "remove" => "remove",
                    _ => "place",
                }
            } else {
                action
            };
            let result = game::pointer(&mut self.app, x, y, action);
            if action != "hover" {
                self.feedback = feedback(result);
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
                "Selected {} at ({},{}). Live details in panel.",
                structure["kind"].as_str().unwrap_or("structure"),
                value["x"],
                value["y"]
            );
        }
        "Tool ready. Point at the grid to preview.".into()
    }

    // This route uses the same tool selection and pointer placement as a player.
    fn build_production_route(app: &mut App) -> Result<(), String> {
        for (kind, xs) in [
            ("extractor", vec![1]),
            ("processor", vec![5]),
            ("conveyor", vec![2, 3, 4, 6, 7, 8, 9]),
        ] {
            game::player_command(
                app,
                &serde_json::json!({"op":"select","kind":kind}).to_string(),
            )?;
            for x in xs {
                let (px, py) = logical_pointer(f64::from(x * 96 + 48), 336., 1152, 768).unwrap();
                game::pointer(app, px, py, "place")?;
            }
        }
        Ok(())
    }

    fn verify_production_frame(app: &App) -> Result<(), String> {
        let state: serde_json::Value = serde_json::from_str(&game::status(app)).unwrap();
        let tick = state["tick"].as_u64().unwrap();
        let delivered = if tick < 189 {
            0
        } else {
            1 + (tick - 189) / 120
        };
        if state["delivered"] != delivered || tick > 1269 || state["seeded"] != 0 {
            return Err(format!("production delivery timing failed: {state}"));
        }
        if tick == 1269 && (state["outcome"] != "Complete" || state["completion_tick"] != 1269) {
            return Err(format!("production completion failed: {state}"));
        }
        let resident = state["structures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                s["slots"]
                    .as_object()
                    .unwrap()
                    .values()
                    .filter(|v| !v.is_null())
                    .count() as u64
            })
            .sum::<u64>();
        if state["extracted"].as_u64().unwrap() != resident + delivered {
            return Err(format!("production accounting failed: {state}"));
        }
        Ok(())
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
        fn host_controls_diagnose_repair_step_and_restart() {
            let mut app = game::build_game();
            app.update_schedule(Startup);
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
                limit: None,
                error: None,
                cursor: None,
                feedback: String::new(),
                test_transport: false,
                test_production: false,
                test_interface: false,
                ui: native_ui::Interface::new(),
                paused: false,
                tool: "conveyor".into(),
            };
            player.interface_acceptance().unwrap();
        }

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
