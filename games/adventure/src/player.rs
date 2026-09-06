//! CPU-only controller shared by native and browser players and their inspector.
use crate::game::{self, Action, Recording};
use serde::Deserialize;
use std::{collections::BTreeSet, time::Duration};
use titan::{
    App, Startup, World,
    input::{ActionValue, BufferedButtons, InputFrame, InputTracker},
    inspection::{
        Dispatch, InspectionConfig, Inspector, StepBudget, dispatch_json_with_policy,
        dispatch_with_policy,
    },
};
use titan_protocol::{
    CommandMetadata, ErrorCode, FieldMetadata, ProtocolError, QueryMetadata, Request,
    RequestEnvelope, RunMode,
};

/// Presentation and captures share this clear color and ECS extraction.
pub const CAPTURE_CLEAR: titan::render::three_d::BaseColor =
    titan::render::three_d::BaseColor::rgb(17, 28, 41);

/// Fresh owned assets, scene and UI at one application safe point.
pub struct FrozenCapture {
    pub scene: titan::render::three_d::RenderFrame3d,
    pub overlay: titan::render::RenderFrame,
    pub assets: titan::render::ImageAssets,
}
impl FrozenCapture {
    pub fn new(app: &App) -> Result<Self, ProtocolError> {
        Ok(Self {
            scene: game::extract(app.world()).map_err(|e| {
                ProtocolError::new(
                    ErrorCode::Internal,
                    format!("capture extraction failed: {e}"),
                )
            })?,
            overlay: game::extract_overlay(app.world()),
            assets: app
                .world()
                .resource::<titan::render::ImageAssets>()
                .ok_or_else(|| {
                    ProtocolError::new(ErrorCode::Internal, "missing capture image assets")
                })?
                .clone(),
        })
    }
}

#[derive(Default)]
struct Control {
    paused: bool,
    epoch: u64,
    keys: BTreeSet<String>,
    blocked: BTreeSet<String>,
    buttons: BufferedButtons<Action>,
    tracker: InputTracker<Action>,
    replay: Option<Playback>,
}
struct Playback {
    frames: Vec<InputFrame<Action>>,
    position: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Load {
    recording: Recording,
}

/// Called by the simulation before consuming its fixed-tick input. A headless
/// application has no Control resource, so its existing input path is unchanged.
pub(crate) fn prepare_tick(world: &mut World) {
    let Some(control) = world.resource_mut::<Control>() else {
        return;
    };
    let frame = if let Some(replay) = &mut control.replay {
        let frame = replay
            .frames
            .get(replay.position)
            .cloned()
            .unwrap_or_default();
        if replay.position < replay.frames.len() {
            replay.position += 1;
        }
        if replay.position == replay.frames.len() {
            control.paused = true;
        }
        frame
    } else if !control.paused {
        let mut active = control.buttons.held().clone();
        let presses = control.buttons.take_presses();
        active.extend(presses.iter().copied());
        let sampled = control
            .tracker
            .sample(active.into_iter().map(|a| (a, ActionValue::PRESSED)));
        // Preserve a release/repress between fixed ticks. The recorded pressed
        // edge is authoritative, so replay sees the same release-gate decision.
        let mut recorded =
            titan::replay::RecordedButtons::capture(&sampled, &game::SCHEMA).unwrap();
        for (action, name) in game::SCHEMA {
            if presses.contains(&action) && !recorded.pressed.iter().any(|value| value == name) {
                recorded.pressed.push(name.into());
            }
        }
        recorded.decode(&game::SCHEMA).unwrap()
    } else {
        return;
    };
    world.insert_resource(frame);
}
pub(crate) fn reset_input(world: &mut World) {
    if world.resource::<Control>().is_some() {
        clear(world);
    }
}
fn clear(world: &mut World) {
    let control = world.resource_mut::<Control>().unwrap();
    control.blocked.extend(control.keys.iter().cloned());
    control.keys.clear();
    control.buttons.clear();
    control.tracker = InputTracker::default();
    control.epoch = control.epoch.wrapping_add(1);
    world.insert_resource(InputFrame::<Action>::default());
    game::clear_scheduled_input(world);
}
fn paused(app: &mut App, value: bool) {
    clear(app.world_mut());
    let control = app.world_mut().resource_mut::<Control>().unwrap();
    control.paused = value
        || control
            .replay
            .as_ref()
            .is_some_and(|r| r.position == r.frames.len());
}
fn load(app: &mut App, recording: Recording) -> Result<(), ProtocolError> {
    if recording.format_version != 1
        || recording.fixture != game::FIXTURE
        || recording.truncated
        || recording.frames.len() > game::MAX_RECORDING_TICKS
    {
        return Err(ProtocolError::new(
            ErrorCode::InvalidValue,
            "unsupported, truncated or oversized recording",
        ));
    }
    let frames = recording
        .frames
        .iter()
        .map(|f| {
            f.decode(&game::SCHEMA).map_err(|_| {
                ProtocolError::new(ErrorCode::InvalidValue, "invalid recorded buttons")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    game::restart(app);
    paused(app, true);
    app.world_mut().resource_mut::<Control>().unwrap().replay = Some(Playback {
        frames,
        position: 0,
    });
    Ok(())
}
fn replay_status(app: &App) -> serde_json::Value {
    let control = app.world().resource::<Control>().unwrap();
    match &control.replay {
        Some(r) => {
            serde_json::json!({"active":true,"position":r.position,"total":r.frames.len(),"complete":r.position == r.frames.len(),"paused":control.paused})
        }
        None => {
            serde_json::json!({"active":false,"position":0,"total":0,"complete":false,"paused":control.paused})
        }
    }
}

pub struct PlayerSession {
    app: App,
    inspector: Inspector,
    enable_control: bool,
}
impl PlayerSession {
    /// Starts paused; hosts explicitly resume an ordinary live session.
    pub fn new(instance: &str, project: &str, run_mode: RunMode, enable_control: bool) -> Self {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        app.world_mut().insert_resource(Control {
            paused: true,
            ..Control::default()
        });
        app.refresh_extracted();
        let mut config = InspectionConfig::controlled(instance, project);
        config.run_mode = run_mode;
        // Dispatch owns the opt-in boundary; allow the game command closure to
        // follow a later browser opt-in as well as the initial native setting.
        config.mutation_enabled = true;
        let mut inspector = game::configured_inspector(config);
        inspector.set_mutation_enabled(enable_control);
        for (name, value) in [("pause", true), ("resume", false)] {
            inspector
                .register_command(
                    CommandMetadata {
                        name: name.into(),
                        description: format!(
                            "{name} this player; cancel pending keyboard and scheduled input"
                        ),
                        arguments: Default::default(),
                    },
                    move |app, _: Empty| {
                        paused(app, value);
                        Ok(())
                    },
                )
                .unwrap();
        }
        inspector
            .register_command(
                CommandMetadata {
                    name: "load_replay".into(),
                    description:
                        "Validate recording then start paused tick-by-tick playback from origin"
                            .into(),
                    arguments: [(
                        "recording".into(),
                        FieldMetadata {
                            type_name: "object".into(),
                            description: "Bounded adventure recording".into(),
                            writable: false,
                            minimum: None,
                            maximum: None,
                            unit: None,
                        },
                    )]
                    .into(),
                },
                |app, args: Load| load(app, args.recording),
            )
            .unwrap();
        inspector
            .register_query(
                QueryMetadata {
                    name: "playback".into(),
                    description: "Player pause state and consumed replay ticks".into(),
                    arguments: Default::default(),
                },
                |app, _: Empty| Ok(replay_status(app)),
            )
            .unwrap();
        Self {
            app,
            inspector,
            enable_control,
        }
    }
    /// Install an owned producer; freezing occurs synchronously at acceptance.
    pub fn register_capture(
        &mut self,
        handler: impl FnMut(
            &App,
            titan_protocol::CaptureIdentity,
            titan::inspection::CaptureCompleter,
        ) -> Result<(), ProtocolError>
        + Send
        + 'static,
    ) {
        self.inspector
            .register_async_capture_handler(960, 540, handler);
    }
    pub fn set_control_enabled(&mut self, enabled: bool) {
        self.enable_control = enabled;
        self.inspector.set_mutation_enabled(enabled);
        self.clear_input();
    }
    pub fn app(&self) -> &App {
        &self.app
    }
    pub fn paused(&self) -> bool {
        self.app.world().resource::<Control>().unwrap().paused
    }
    pub fn clock_epoch(&self) -> u64 {
        self.app.world().resource::<Control>().unwrap().epoch
    }
    pub fn replay_active(&self) -> bool {
        self.app
            .world()
            .resource::<Control>()
            .unwrap()
            .replay
            .is_some()
    }
    pub fn replay_status(&self) -> serde_json::Value {
        replay_status(&self.app)
    }
    pub fn clear_input(&mut self) {
        clear(self.app.world_mut());
    }
    pub fn pause(&mut self) {
        paused(&mut self.app, true);
        self.inspector.note_external_change();
    }
    pub fn resume(&mut self) {
        paused(&mut self.app, false);
        self.inspector.note_external_change();
    }
    pub fn restart(&mut self) {
        // Restart returns to live play and cancels every previous input source.
        game::restart(&mut self.app);
        self.inspector.reset_capture_session();
        clear(self.app.world_mut());
        let control = self.app.world_mut().resource_mut::<Control>().unwrap();
        control.replay = None;
        self.inspector.note_external_change();
    }
    pub fn stop_replay(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<Control>()
            .unwrap()
            .replay = None;
        self.restart();
        self.pause();
    }
    pub fn load_replay(&mut self, recording: Recording) -> Result<(), ProtocolError> {
        load(&mut self.app, recording)?;
        self.inspector.reset_capture_session();
        self.inspector.note_external_change();
        Ok(())
    }
    /// Physical key names match KeyboardEvent.code, making alias and repeat
    /// behavior identical on both hosts. Paused movement is deliberately ignored.
    pub fn set_key(&mut self, key: &str, pressed: bool, repeat: bool) {
        if !pressed {
            self.app
                .world_mut()
                .resource_mut::<Control>()
                .unwrap()
                .blocked
                .remove(key);
        }
        if repeat || self.paused() || self.replay_active() {
            return;
        }
        let Some(action) = key_action(key) else {
            return;
        };
        let control = self.app.world_mut().resource_mut::<Control>().unwrap();
        if control
            .blocked
            .iter()
            .any(|blocked| key_action(blocked) == Some(action))
        {
            if pressed {
                control.blocked.insert(key.into());
            }
            return;
        }
        if pressed {
            control.keys.insert(key.into());
        } else {
            control.keys.remove(key);
        }
        let held = control
            .keys
            .iter()
            .any(|key| key_action(key) == Some(action));
        control.buttons.set(action, held, true);
    }
    pub fn tick(&mut self) {
        if self.paused() {
            return;
        }
        let generation = game::status(&self.app)["session_generation"].clone();
        self.app.advance_fixed(1);
        if game::status(&self.app)["session_generation"] != generation {
            self.inspector.reset_capture_session();
        }
        self.inspector.note_external_change();
        if self.paused() {
            clear(self.app.world_mut());
        }
    }
    pub fn step(&mut self) -> Result<(), ProtocolError> {
        if !self.paused() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidValue,
                "pause before stepping",
            ));
        }
        if self.remaining() == Some(0) {
            return Err(ProtocolError::new(
                ErrorCode::InvalidValue,
                "replay is complete",
            ));
        }
        let generation = game::status(&self.app)["session_generation"].clone();
        self.app.advance_fixed(1);
        if game::status(&self.app)["session_generation"] != generation {
            self.inspector.reset_capture_session();
        }
        self.inspector.note_external_change();
        Ok(())
    }
    fn remaining(&self) -> Option<u64> {
        self.app
            .world()
            .resource::<Control>()
            .unwrap()
            .replay
            .as_ref()
            .map(|r| (r.frames.len() - r.position) as u64)
    }
    pub fn capture_timeout(&self) -> Duration {
        self.inspector.capture_timeout()
    }
    pub fn dispatch(&mut self, request: &RequestEnvelope) -> Dispatch {
        self.inspector.set_controlled(self.paused());
        self.inspector.set_step_budget(StepBudget {
            max_frames: self.remaining().unwrap_or(StepBudget::DEFAULT.max_frames),
            ..StepBudget::DEFAULT
        });
        // Immediate replay is a headless command. Interactive playback has one
        // input owner; remote injection/teleport cannot overwrite recorded ticks.
        let allowed = match &request.request {
            Request::Invoke { name, .. } => {
                name != "replay"
                    && (!self.replay_active()
                        || matches!(
                            name.as_str(),
                            "pause" | "resume" | "restart" | "load_replay"
                        ))
            }
            Request::InjectInput { .. } | Request::SetField { .. } => {
                !self.replay_active() && self.paused()
            }
            _ => true,
        };
        let generation = game::status(&self.app)["session_generation"].clone();
        let result = dispatch_with_policy(
            &mut self.app,
            &mut self.inspector,
            self.enable_control && allowed,
            request,
        );
        if game::status(&self.app)["session_generation"] != generation {
            self.inspector.reset_capture_session();
        }
        if game::status(&self.app)["session_generation"] != generation
            && !matches!(&request.request, Request::Invoke {name,..} if name == "load_replay")
        {
            clear(self.app.world_mut());
            let control = self.app.world_mut().resource_mut::<Control>().unwrap();
            control.replay = None;
        }
        self.inspector.set_controlled(self.paused());
        result
    }
    pub fn dispatch_json(&mut self, json: &str) -> Dispatch {
        match serde_json::from_str::<RequestEnvelope>(json) {
            Ok(request) => self.dispatch(&request),
            Err(_) => dispatch_json_with_policy(
                &mut self.app,
                &mut self.inspector,
                self.enable_control,
                json,
            ),
        }
    }
}
fn key_action(key: &str) -> Option<Action> {
    match key {
        "KeyW" | "ArrowUp" => Some(Action::Up),
        "KeyS" | "ArrowDown" => Some(Action::Down),
        "KeyA" | "ArrowLeft" => Some(Action::Left),
        "KeyD" | "ArrowRight" => Some(Action::Right),
        "KeyQ" => Some(Action::Switch),
        "KeyR" => Some(Action::Restart),
        _ => None,
    }
}

/// The semantic acceptance route; playback consumes these frames one at a time.
pub fn reference_recording() -> Recording {
    let mut tracker = InputTracker::default();
    let mut frames = Vec::new();
    for (action, ticks) in [(Action::Right, 8), (Action::Switch, 1), (Action::Up, 8)] {
        for _ in 0..ticks {
            frames.push(
                titan::replay::RecordedButtons::capture(
                    &tracker.sample([(action, ActionValue::PRESSED)]),
                    &game::SCHEMA,
                )
                .unwrap(),
            );
        }
    }
    Recording {
        format_version: 1,
        fixture: game::FIXTURE.into(),
        frames,
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> PlayerSession {
        PlayerSession::new("test", "test", RunMode::Interactive, true)
    }
    fn state(player: &PlayerSession) -> serde_json::Value {
        game::status(player.app())
    }
    #[test]
    fn taps_aliases_switch_and_focus_use_logical_actions() {
        let mut p = session();
        p.resume();
        p.set_key("KeyD", true, false);
        p.set_key("KeyD", false, false);
        p.tick();
        assert_eq!(state(&p)["characters"]["jumper"]["x"], 1560);
        p.set_key("KeyD", true, false);
        p.set_key("ArrowRight", true, false);
        p.tick();
        p.set_key("KeyQ", true, false);
        p.tick();
        let switched = state(&p)["characters"].clone();
        assert_eq!(state(&p)["active_character"], "strong");
        p.set_key("KeyD", false, false);
        p.tick();
        assert_eq!(state(&p)["characters"], switched);
        p.set_key("ArrowRight", false, false);
        p.tick();
        p.set_key("KeyD", true, false);
        p.tick();
        assert_eq!(state(&p)["characters"]["strong"]["x"], 3560);
        p.pause();
        p.resume();
        p.set_key("KeyD", true, true);
        p.tick();
        assert_eq!(state(&p)["characters"]["strong"]["x"], 3560);
        p.set_key("KeyD", false, false);
        p.set_key("KeyD", true, false);
        p.tick();
        assert_eq!(state(&p)["characters"]["strong"]["x"], 3620);
    }
    #[test]
    fn replay_and_live_share_the_same_game() {
        let mut player = session();
        player.load_replay(reference_recording()).unwrap();
        player.resume();
        for _ in 0..100 {
            player.tick();
        }
        let mut headless = game::build_game();
        headless.update_schedule(Startup);
        game::replay(&mut headless, reference_recording()).unwrap();
        for key in ["characters", "active_character", "session_tick"] {
            assert_eq!(state(&player)[key], game::status(&headless)[key]);
        }
        assert!(player.paused());
        assert!(player.step().is_err());
        player.restart();
        assert_eq!(state(&player)["session_tick"], 0);
        assert!(!player.replay_active());
    }
    #[test]
    fn restarting_cancels_pending_capture() {
        use std::sync::{Arc, Mutex};
        let mut player = session();
        let completion = Arc::new(Mutex::new(None));
        let observed = completion.clone();
        player.register_capture(move |_, _, done| {
            *observed.lock().unwrap() = Some(done);
            Ok(())
        });
        let Dispatch::Pending(mut pending) =
            player.dispatch(&RequestEnvelope::new("capture", Request::Capture))
        else {
            panic!("pending");
        };
        player.restart();
        assert!(matches!(
            pending.poll(Duration::ZERO).unwrap().outcome,
            titan_protocol::ResponseOutcome::Failure { .. }
        ));
        assert!(completion.lock().unwrap().as_ref().unwrap().is_cancelled());
    }
    #[test]
    fn replay_budget_and_mutation_opt_in_are_enforced() {
        use titan_protocol::ResponseOutcome;
        let mut p = session();
        p.load_replay(reference_recording()).unwrap();
        let Dispatch::Ready(response) = p.dispatch(&RequestEnvelope::new(
            "overshoot",
            Request::Step { frames: 18 },
        )) else {
            panic!("ready")
        };
        assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
        assert_eq!(state(&p)["session_tick"], 0);
        p.set_control_enabled(false);
        let Dispatch::Ready(response) = p.dispatch(&RequestEnvelope::new(
            "disabled",
            Request::Step { frames: 1 },
        )) else {
            panic!("ready")
        };
        assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
        p.set_control_enabled(true);
        let Dispatch::Ready(response) =
            p.dispatch(&RequestEnvelope::new("play", Request::Step { frames: 17 }))
        else {
            panic!("ready")
        };
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        assert_eq!(state(&p)["session_tick"], 17);
    }
    #[test]
    fn capture_freezes_fresh_overlay_and_owned_assets_without_tick() {
        let mut p = session();
        let before = FrozenCapture::new(p.app()).unwrap();
        let hud = p.app.world().iter::<titan::ui::UiText>().next().unwrap().0;
        p.app
            .world_mut()
            .get_mut::<titan::ui::UiText>(hud)
            .unwrap()
            .text = "UNREFRESHED".into();
        let after = FrozenCapture::new(p.app()).unwrap();
        assert_ne!(before.overlay, after.overlay);
        assert_eq!(state(&p)["session_tick"], 0);
        let image = before.overlay.sprites()[0].image;
        p.app
            .world_mut()
            .resource_mut::<titan::render::ImageAssets>()
            .unwrap()
            .remove(image);
        assert!(before.assets.get(image).is_some());
    }
    #[test]
    fn keyboard_restart_invalidates_capture_and_suppresses_held_movement() {
        use std::sync::{Arc, Mutex};
        let mut p = session();
        p.resume();
        p.set_key("KeyD", true, false);
        p.tick();
        let hold = Arc::new(Mutex::new(None));
        let out = hold.clone();
        p.register_capture(move |_, _, done| {
            *out.lock().unwrap() = Some(done);
            Ok(())
        });
        let Dispatch::Pending(mut pending) =
            p.dispatch(&RequestEnvelope::new("capture", Request::Capture))
        else {
            panic!("pending")
        };
        p.set_key("KeyR", true, false);
        p.tick();
        assert!(matches!(
            pending.poll(Duration::ZERO).unwrap().outcome,
            titan_protocol::ResponseOutcome::Failure { .. }
        ));
        p.tick();
        assert_eq!(state(&p)["characters"]["jumper"]["x"], 1500);
    }
    #[test]
    fn release_repress_between_ticks_is_recorded_and_replayed() {
        let mut p = session();
        p.resume();
        p.set_key("KeyD", true, false);
        p.tick();
        p.set_key("KeyQ", true, false);
        p.tick();
        p.set_key("KeyD", false, false);
        p.set_key("KeyD", true, false);
        p.tick();
        assert_eq!(state(&p)["characters"]["strong"]["x"], 3560);
        p.set_key("KeyQ", false, false);
        p.set_key("KeyQ", true, false);
        p.tick();
        assert_eq!(state(&p)["active_character"], "jumper");
        let live = state(&p);
        let recording = game::recording(p.app()).unwrap();
        let mut replay = game::build_game();
        replay.update_schedule(Startup);
        game::replay(&mut replay, recording).unwrap();
        assert_eq!(game::status(&replay)["characters"], live["characters"]);
        assert_eq!(
            game::status(&replay)["active_character"],
            live["active_character"]
        );
    }
}
