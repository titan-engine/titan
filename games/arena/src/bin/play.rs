//! Native interactive runner; the engine and game remain independent of winit.

#[cfg(not(target_arch = "wasm32"))]
use titan_game::game;
#[cfg(not(target_arch = "wasm32"))]
#[path = "../surface.rs"]
mod gpu_surface;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{game, gpu_surface::SurfaceRenderer};
    use std::{
        collections::HashSet,
        sync::Arc,
        time::{Duration, Instant},
    };
    use titan::{App, Startup};
    use titan_render_wgpu::wgpu;
    use winit::{
        application::ApplicationHandler,
        dpi::LogicalSize,
        event::{ElementState, WindowEvent},
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
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut args = std::env::args().skip(1);
        let mut limit = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
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
                        "play [--frames N] \nMove with arrow keys or WASD; R restarts; Escape exits.\n--frames exits after N presented GPU frames."
                    );
                    return Ok(());
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let mut player = Player {
            app,
            input: game::InteractiveInput::default(),
            held_keys: HashSet::new(),
            window: None,
            renderer: None,
            previous: Instant::now(),
            accumulated: Duration::ZERO,
            rendered: 0,
            limit,
            error: None,
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
                                .with_title("Titan — Arena Survival")
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
                WindowEvent::Focused(false) => {
                    self.held_keys.clear();
                    self.input = game::InteractiveInput::default();
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let PhysicalKey::Code(key) = event.physical_key else {
                        return;
                    };
                    if key == KeyCode::Escape && event.state == ElementState::Pressed {
                        event_loop.exit();
                        return;
                    }
                    if key == KeyCode::KeyR && event.state == ElementState::Pressed {
                        game::restart(&mut self.app);
                        self.held_keys.clear();
                        self.input = game::InteractiveInput::default();
                    }
                    if let Some((action, pressed)) = update_key(
                        &mut self.held_keys,
                        key,
                        event.state == ElementState::Pressed,
                    ) {
                        self.input
                            .set_action(action, pressed)
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
                    while self.accumulated >= tick {
                        self.input.tick(&mut self.app);
                        self.accumulated -= tick;
                    }
                    match self.renderer.as_mut().unwrap().render(&self.app) {
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
                    } else {
                        self.window.as_ref().unwrap().request_redraw();
                    }
                }
                _ => {}
            }
        }
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
        let action = action_for_key(key)?;
        if pressed {
            held.insert(key);
        } else {
            held.remove(&key);
        }
        Some((
            action,
            held.iter().any(|key| action_for_key(*key) == Some(action)),
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

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
            assert_eq!(update_key(&mut held, KeyCode::Space, true), None);
            assert!(held.is_empty());
        }
    }
}
