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

#[derive(Default)]
struct Control {
    paused: bool,
    epoch: u64,
    keys: BTreeSet<String>,
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
        active.extend(control.buttons.take_presses());
        control
            .tracker
            .sample(active.into_iter().map(|a| (a, ActionValue::PRESSED)))
    } else {
        return;
    };
    world.insert_resource(frame);
}
fn clear(world: &mut World) {
    let control = world.resource_mut::<Control>().unwrap();
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
                            description: "Bounded collection room recording".into(),
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
        self.inspector.note_external_change();
        Ok(())
    }
    /// Physical key names match KeyboardEvent.code, making alias and repeat
    /// behavior identical on both hosts. Paused movement is deliberately ignored.
    pub fn set_key(&mut self, key: &str, pressed: bool, repeat: bool) {
        if repeat || self.paused() || self.replay_active() {
            return;
        }
        let Some(action) = key_action(key) else {
            return;
        };
        let control = self.app.world_mut().resource_mut::<Control>().unwrap();
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
        self.app.advance_fixed(1);
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
        self.app.advance_fixed(1);
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
        _ => None,
    }
}

/// The semantic acceptance route; playback consumes these frames one at a time.
pub fn reference_recording() -> Recording {
    let mut tracker = InputTracker::default();
    let mut frames = Vec::new();
    for (action, ticks) in [(Action::Right, 8), (Action::Up, 20), (Action::Right, 16)] {
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
    use titan_protocol::{Response, ResponseOutcome};
    fn session() -> PlayerSession {
        PlayerSession::new("test", "test", RunMode::Interactive, true)
    }
    fn request(session: &mut PlayerSession, request: Request) -> titan_protocol::ResponseEnvelope {
        match session.dispatch(&RequestEnvelope::new("test", request)) {
            Dispatch::Ready(r) => r,
            Dispatch::Pending(_) => panic!("capture is unregistered"),
        }
    }
    fn state(session: &PlayerSession) -> serde_json::Value {
        game::status(session.app())
    }
    fn query(session: &mut PlayerSession) -> serde_json::Value {
        let response = request(
            session,
            Request::Query {
                name: "state".into(),
                arguments: Default::default(),
            },
        );
        match response.outcome {
            ResponseOutcome::Success {
                response: Response::QueryResult { value },
            } => value,
            _ => panic!("query failed: {response:?}"),
        }
    }
    #[test]
    fn taps_aliases_release_focus_and_repeat_share_one_sampler() {
        let mut player = session();
        player.resume();
        player.set_key("KeyD", true, false);
        player.set_key("KeyD", false, false);
        player.tick();
        assert_eq!(state(&player)["position"]["x"], -2750);
        player.tick();
        assert_eq!(state(&player)["position"]["x"], -2750);
        player.set_key("KeyD", true, false);
        player.set_key("ArrowRight", true, false);
        player.set_key("KeyD", false, false);
        player.tick();
        assert_eq!(state(&player)["position"]["x"], -2500);
        player.clear_input();
        player.set_key("ArrowRight", true, true);
        player.tick();
        assert_eq!(state(&player)["position"]["x"], -2500);
        player.set_key("KeyD", true, false);
        player.pause();
        player.resume();
        player.tick();
        assert_eq!(state(&player)["position"]["x"], -2500);
    }
    #[test]
    fn replay_runs_exactly_44_ticks_and_inspector_observes_played_app() {
        let mut player = session();
        player.load_replay(reference_recording()).unwrap();
        player.step().unwrap();
        assert_eq!(query(&mut player)["position"]["x"], -2750);
        player.resume();
        for _ in 0..100 {
            player.tick();
        }
        let live = query(&mut player);
        assert_eq!(live["session_tick"], 44);
        assert_eq!(live["completed"], true);
        assert_eq!(live["position"], serde_json::json!({"x":3000,"z":-2000}));
        let mut headless = game::build_game();
        headless.update_schedule(Startup);
        game::replay(&mut headless, reference_recording()).unwrap();
        for key in [
            "position",
            "collected",
            "completed",
            "remaining",
            "session_tick",
        ] {
            assert_eq!(live[key], game::status(&headless)[key]);
        }
        assert!(player.paused());
        assert!(player.step().is_err());
        player.restart();
        assert_eq!(state(&player)["session_tick"], 0);
        assert_eq!(player.replay_status()["position"], 0);
        assert!(!player.replay_active());
        player.step().unwrap();
        assert_eq!(state(&player)["session_tick"], 1);
    }
    #[test]
    fn remote_step_budget_rejects_overshoot_and_replay_input_is_exclusive() {
        let mut player = session();
        player.load_replay(reference_recording()).unwrap();
        let response = request(&mut player, Request::Step { frames: 45 });
        assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
        assert_eq!(state(&player)["session_tick"], 0);
        let response = request(&mut player, Request::Step { frames: 44 });
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        assert_eq!(state(&player)["session_tick"], 44);
        assert_eq!(state(&player)["completed"], true);
        let response = request(&mut player, Request::Step { frames: 1 });
        assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
    }
    #[test]
    fn invalid_replay_preserves_state_and_control_opt_in_can_change() {
        let mut player = session();
        player.resume();
        player.set_key("KeyD", true, false);
        player.tick();
        let before = state(&player);
        let mut recording = reference_recording();
        recording.fixture = "unknown".into();
        assert!(player.load_replay(recording).is_err());
        assert_eq!(before, state(&player));
        player.pause();
        player.set_control_enabled(false);
        let response = request(&mut player, Request::Step { frames: 1 });
        assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
        player.set_control_enabled(true);
        let response = request(&mut player, Request::Step { frames: 1 });
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
    }
    #[test]
    fn remote_scheduled_input_is_canceled_when_live_resumes() {
        let mut player = session();
        let response = request(
            &mut player,
            Request::InjectInput {
                frame: 1,
                actions: [("right".into(), titan_protocol::InputValue::Button(true))].into(),
            },
        );
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        player.resume();
        player.set_key("KeyW", true, false);
        player.tick();
        assert_eq!(
            state(&player)["position"],
            serde_json::json!({"x":-3000,"z":2750})
        );
    }
}
