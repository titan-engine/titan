//! Inspection and bounded recordings of the actual playable arena instance.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use titan::{
    App, FixedTime, Startup, World,
    input::{ActionValue, InputFrame, InputTracker},
    inspection::{InspectionConfig, Inspector, handle_with_policy},
};
use titan_protocol::{
    CommandMetadata, ErrorCode, ProtocolError, QueryMetadata, Request, RequestEnvelope,
    ResponseEnvelope, ResponseOutcome, RunMode,
};

use crate::game::{self, Action};

pub const MAX_RECORDING_TICKS: usize = 3_600;
pub const MAX_RECORDING_BYTES: usize = 2 * 1024 * 1024;
const ACTION_SCHEMA: &str = "arena-buttons-v1:up,down,left,right,dash";
const ACTIONS: [(Action, &str); 5] = [
    (Action::Up, "up"),
    (Action::Down, "down"),
    (Action::Left, "left"),
    (Action::Right, "right"),
    (Action::Dash, "dash"),
];

#[derive(Default)]
struct PlaybackControl {
    paused: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedFrame {
    active: Vec<String>,
    pressed: Vec<String>,
    released: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    format_version: u32,
    game_seed: u32,
    action_schema: String,
    fixed_step_nanos: u64,
    start_host_frame: u64,
    recorded_ticks: usize,
    max_ticks: usize,
    truncated: bool,
    invalid_reason: Option<String>,
    frames: Vec<RecordedFrame>,
    final_state: serde_json::Value,
    final_checksum: String,
}

struct RecordingState {
    start_host_frame: u64,
    frames: Vec<RecordedFrame>,
    truncated: bool,
    invalid_reason: Option<String>,
    expected_positions: Vec<(u32, u32, i32, i32)>,
}

fn positions(world: &World) -> Vec<(u32, u32, i32, i32)> {
    world
        .iter::<game::Position>()
        .map(|(entity, position)| (entity.index(), entity.generation(), position.x, position.y))
        .collect()
}

fn invalid_reason(world: &World) -> Option<String> {
    let recording = world.resource::<RecordingState>().unwrap();
    recording.invalid_reason.clone().or_else(|| {
        (positions(world) != recording.expected_positions)
            .then(|| "position field changed outside consumed input".into())
    })
}

/// Record the post-simulation baseline so headless field edits are detected too.
pub(crate) fn finish_recording(world: &mut World) {
    let expected = positions(world);
    world
        .resource_mut::<RecordingState>()
        .unwrap()
        .expected_positions = expected;
}

/// Restart is the only recording origin; a truncated recording retains its prefix.
pub(crate) fn begin_recording(world: &mut World) {
    let start_host_frame = world.resource::<FixedTime>().map_or(0, |time| time.tick());
    let expected_positions = positions(world);
    world.insert_resource(RecordingState {
        start_host_frame,
        frames: Vec::new(),
        truncated: false,
        invalid_reason: None,
        expected_positions,
    });
}

/// Runs after the scheduled-input override and before the simulation consumes it.
pub(crate) fn record_consumed(world: &mut World) {
    let invalid_reason = invalid_reason(world);
    let input = world.resource::<InputFrame<Action>>().unwrap();
    let frame = RecordedFrame {
        active: ACTIONS
            .iter()
            .filter(|(action, _)| input.is_active(action))
            .map(|(_, name)| (*name).to_owned())
            .collect(),
        pressed: ACTIONS
            .iter()
            .filter(|(action, _)| input.just_pressed(action))
            .map(|(_, name)| (*name).to_owned())
            .collect(),
        released: ACTIONS
            .iter()
            .filter(|(action, _)| input.just_released(action))
            .map(|(_, name)| (*name).to_owned())
            .collect(),
    };
    let recording = world.resource_mut::<RecordingState>().unwrap();
    recording.invalid_reason = invalid_reason;
    if recording.frames.len() < MAX_RECORDING_TICKS {
        recording.frames.push(frame);
    } else {
        recording.truncated = true;
    }
}

fn comparable_state(app: &App) -> serde_json::Value {
    let mut state: serde_json::Value = serde_json::from_str(&game::status(app)).unwrap();
    state.as_object_mut().unwrap().remove("frame");
    state
}

fn recording_value(app: &App) -> Result<serde_json::Value, ProtocolError> {
    let source = app.world().resource::<RecordingState>().unwrap();
    let image = game::render_image(app.world())?;
    serde_json::to_value(Recording {
        format_version: 1,
        game_seed: game::SEED,
        action_schema: ACTION_SCHEMA.to_owned(),
        fixed_step_nanos: 16_666_667,
        start_host_frame: source.start_host_frame,
        recorded_ticks: source.frames.len(),
        max_ticks: MAX_RECORDING_TICKS,
        truncated: source.truncated,
        invalid_reason: invalid_reason(app.world()),
        frames: source.frames.clone(),
        final_state: comparable_state(app),
        final_checksum: format!("{:016x}", game::image_checksum(&image)),
    })
    .map_err(|error| ProtocolError::new(ErrorCode::Internal, error.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArguments {}

pub(crate) fn register_queries(inspector: &mut Inspector) {
    inspector
        .register_query(
            QueryMetadata {
                name: "arena_state".into(),
                description: "Read the actual arena run, player, dash and recording status.".into(),
                arguments: BTreeMap::new(),
            },
            |app, _: NoArguments| {
                let mut value: serde_json::Value =
                    serde_json::from_str(&game::status(app)).unwrap();
                value["paused"] = app
                    .world()
                    .resource::<PlaybackControl>()
                    .is_none_or(|control| control.paused)
                    .into();
                let recording = app.world().resource::<RecordingState>().unwrap();
                value["recording"] = serde_json::json!({
                    "start_host_frame": recording.start_host_frame,
                    "recorded_ticks": recording.frames.len(),
                    "max_ticks": MAX_RECORDING_TICKS,
                    "truncated": recording.truncated,
                    "invalid_reason": invalid_reason(app.world()),
                });
                Ok(value)
            },
        )
        .expect("unique arena state query");
    inspector
        .register_query(
            QueryMetadata {
                name: "recording".into(),
                description: "Export consumed fixed-tick inputs from the latest restart, bounded to 3600 ticks.".into(),
                arguments: BTreeMap::new(),
            },
            |app, _: NoArguments| recording_value(app),
        )
        .expect("unique recording query");
}

/// The same app is ticked, rendered, queried and captured by both playable hosts.
pub struct ArenaSession {
    app: App,
    input: game::InteractiveInput,
    inspector: Inspector,
    enable_control: bool,
    clock_epoch: u64,
}

impl ArenaSession {
    /// Starts paused. Native autorun hosts should call `resume` explicitly.
    pub fn new(instance_id: &str, project: &str, run_mode: RunMode, enable_control: bool) -> Self {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        app.world_mut()
            .insert_resource(PlaybackControl { paused: true });
        let mut config = InspectionConfig::controlled(instance_id, project);
        config.run_mode = run_mode;
        config.mutation_enabled = enable_control;
        let mut inspector = game::inspector_with_capture(config, |app| {
            titan_diagnostics::png_capture(&game::render_image(app.world())?)
        });
        for (name, paused) in [("pause", true), ("resume", false)] {
            inspector
                .register_command(
                    CommandMetadata {
                        name: name.into(),
                        description: format!("{name} the actual player at a fixed-tick safe point; clears pending input."),
                        arguments: BTreeMap::new(),
                    },
                    move |app, _: NoArguments| {
                        app.world_mut().resource_mut::<PlaybackControl>().unwrap().paused = paused;
                        Ok(())
                    },
                )
                .expect("unique player clock command");
        }
        Self {
            app,
            input: game::InteractiveInput::default(),
            inspector,
            enable_control,
            clock_epoch: 0,
        }
    }

    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn paused(&self) -> bool {
        self.app
            .world()
            .resource::<PlaybackControl>()
            .unwrap()
            .paused
    }

    pub const fn clock_epoch(&self) -> u64 {
        self.clock_epoch
    }

    pub const fn control_enabled(&self) -> bool {
        self.enable_control
    }

    pub fn set_control_enabled(&mut self, enabled: bool) {
        if self.enable_control != enabled {
            self.enable_control = enabled;
            self.inspector.set_mutation_enabled(enabled);
            self.inspector.note_external_change();
        }
    }

    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), String> {
        self.input.set_action(name, pressed)
    }

    pub fn cancel_action(&mut self, name: &str) -> Result<(), String> {
        self.input.cancel_action(name)
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.app
            .world_mut()
            .insert_resource(InputFrame::<Action>::default());
    }

    fn reset_timing_and_input(&mut self) {
        self.clear_input();
        game::clear_scheduled_input(&mut self.app);
        self.clock_epoch = self.clock_epoch.wrapping_add(1);
    }

    pub fn pause(&mut self) {
        self.set_paused(true);
    }

    pub fn resume(&mut self) {
        self.set_paused(false);
    }

    fn set_paused(&mut self, paused: bool) {
        if self.paused() != paused {
            self.app
                .world_mut()
                .resource_mut::<PlaybackControl>()
                .unwrap()
                .paused = paused;
            self.reset_timing_and_input();
            self.inspector.set_controlled(paused);
            self.inspector.note_external_change();
        }
    }

    pub fn restart(&mut self) {
        game::restart(&mut self.app);
        self.reset_timing_and_input();
        self.inspector.note_external_change();
    }

    pub fn tick(&mut self) {
        if !self.paused() {
            self.input.tick(&mut self.app);
            self.inspector.note_external_change();
        }
    }

    pub fn handle(&mut self, request: &RequestEnvelope) -> ResponseEnvelope {
        let was_paused = self.paused();
        self.inspector.set_controlled(was_paused);
        let response = handle_with_policy(
            &mut self.app,
            &mut self.inspector,
            self.enable_control,
            request,
        );
        if matches!(response.outcome, ResponseOutcome::Success { .. }) {
            if matches!(&request.request, Request::Invoke { name, .. } if name == "restart")
                || was_paused != self.paused()
            {
                self.reset_timing_and_input();
            }
            if matches!(request.request, Request::SetField { .. }) {
                self.app
                    .world_mut()
                    .resource_mut::<RecordingState>()
                    .unwrap()
                    .invalid_reason = Some("position field changed outside consumed input".into());
            }
            self.app.refresh_extracted();
        }
        self.inspector.set_controlled(self.paused());
        response
    }

    pub fn handle_json(&mut self, request_json: &str) -> String {
        match serde_json::from_str::<RequestEnvelope>(request_json) {
            Ok(request) => serde_json::to_string(&self.handle(&request)).unwrap(),
            Err(_) => titan::inspection::handle_json_with_policy(
                &mut self.app,
                &mut self.inspector,
                self.enable_control,
                request_json,
            ),
        }
    }
}

fn decode_actions(names: &[String]) -> Result<Vec<Action>, String> {
    if names.len() > ACTIONS.len() {
        return Err("too many actions in recorded frame".into());
    }
    let mut actions = Vec::new();
    for name in names {
        let action = ACTIONS
            .iter()
            .find(|(_, known)| name == known)
            .map(|(action, _)| *action)
            .ok_or_else(|| format!("unknown recorded action: {name}"))?;
        if actions.contains(&action) {
            return Err("duplicate recorded action".into());
        }
        actions.push(action);
    }
    Ok(actions)
}

fn decode_frame(frame: &RecordedFrame) -> Result<InputFrame<Action>, String> {
    let active = decode_actions(&frame.active)?;
    let pressed = decode_actions(&frame.pressed)?;
    let released = decode_actions(&frame.released)?;
    if pressed.iter().any(|action| !active.contains(action))
        || released.iter().any(|action| active.contains(action))
    {
        return Err("recorded edges conflict with active actions".into());
    }
    let before = active
        .iter()
        .filter(|action| !pressed.contains(action))
        .copied()
        .chain(released);
    let mut tracker = InputTracker::new();
    tracker.sample(before.map(|action| (action, ActionValue::PRESSED)));
    Ok(tracker.sample(
        active
            .into_iter()
            .map(|action| (action, ActionValue::PRESSED)),
    ))
}

/// Replay a raw recording or a saved CLI query response in a fresh headless app.
/// The captured state and software pixels must both agree before success is returned.
pub fn verify_recording(mut value: serde_json::Value) -> Result<serde_json::Value, String> {
    if value.get("response").is_some() {
        value = value
            .get_mut("response")
            .unwrap()
            .get_mut("value")
            .ok_or("query response lacks recording value")?
            .take();
    } else if value.get("value").is_some() {
        value = value.get_mut("value").unwrap().take();
    }
    let recording: Recording = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if recording.format_version != 1
        || recording.game_seed != game::SEED
        || recording.action_schema != ACTION_SCHEMA
        || recording.fixed_step_nanos != 16_666_667
        || recording.max_ticks != MAX_RECORDING_TICKS
    {
        return Err("unsupported recording header".into());
    }
    if recording.truncated || recording.invalid_reason.is_some() {
        return Err("recording is truncated or invalidated; exact replay is unavailable".into());
    }
    if recording.frames.len() > MAX_RECORDING_TICKS
        || recording.frames.len() != recording.recorded_ticks
    {
        return Err("recording frame count exceeds bounds or differs from header".into());
    }
    let frames = recording
        .frames
        .iter()
        .map(decode_frame)
        .collect::<Result<Vec<_>, _>>()?;
    let mut app = game::build_game();
    app.update_schedule(Startup);
    for frame in frames {
        app.world_mut().insert_resource(frame);
        app.advance_fixed(1);
    }
    let state = comparable_state(&app);
    let image = game::render_image(app.world()).map_err(|error| error.message)?;
    let checksum = format!("{:016x}", game::image_checksum(&image));
    if state != recording.final_state || checksum != recording.final_checksum {
        return Err(format!(
            "replay mismatch: state or image differs (actual checksum {checksum})"
        ));
    }
    Ok(serde_json::json!({
        "verified": true,
        "ticks": recording.recorded_ticks,
        "source_start_host_frame": recording.start_host_frame,
        "replay_frame": app.world().resource::<FixedTime>().unwrap().tick(),
        "state": state,
        "checksum": checksum,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::{EntityId, InputValue, Response};

    fn session(control: bool) -> ArenaSession {
        ArenaSession::new("live-test", "arena", RunMode::Interactive, control)
    }

    fn request(session: &mut ArenaSession, request: Request) -> ResponseEnvelope {
        session.handle(&RequestEnvelope::new("test", request))
    }

    fn success(response: ResponseEnvelope) -> Response {
        match response.outcome {
            ResponseOutcome::Success { response } => response,
            other => panic!("expected success: {other:?}"),
        }
    }

    fn query(session: &mut ArenaSession, name: &str) -> serde_json::Value {
        match success(request(
            session,
            Request::Query {
                name: name.into(),
                arguments: BTreeMap::new(),
            },
        )) {
            Response::QueryResult { value } => value,
            other => panic!("expected query result: {other:?}"),
        }
    }

    #[test]
    fn actual_player_inspection_pause_step_resume_and_revision() {
        let mut live = session(true);
        live.resume();
        live.set_action("right", true).unwrap();
        live.set_action("dash", true).unwrap();
        live.tick();
        let state = query(&mut live, "arena_state");
        assert_eq!(state["position"]["x"], 84);
        assert_eq!(state["run"]["dash_cooldown"], 120);
        let paused = request(
            &mut live,
            Request::Invoke {
                name: "pause".into(),
                arguments: BTreeMap::new(),
            },
        );
        let frame = paused.observed_frame;
        let revision = paused.state_revision;
        live.tick();
        let status = request(&mut live, Request::Status);
        assert_eq!(
            (status.observed_frame, status.state_revision),
            (frame, revision)
        );
        assert!(matches!(success(status), Response::Status(status) if status.paused));
        success(request(&mut live, Request::Step { frames: 5 }));
        assert_eq!(query(&mut live, "arena_state")["position"]["x"], 104);
        // Resume clears scheduled control input and physical aliases via the epoch.
        success(request(
            &mut live,
            Request::InjectInput {
                frame: frame + 6,
                actions: BTreeMap::from([("left".into(), InputValue::Button(true))]),
            },
        ));
        live.resume();
        live.tick();
        assert_eq!(query(&mut live, "arena_state")["position"]["x"], 104);
        let before = request(&mut live, Request::Status);
        live.restart();
        let after = request(&mut live, Request::Status);
        assert_eq!(before.observed_frame, after.observed_frame);
        assert!(after.state_revision > before.state_revision);
        assert_eq!(query(&mut live, "recording")["recorded_ticks"], 0);
    }

    #[test]
    fn read_only_queries_and_capture_do_not_enable_mutations() {
        let mut live = session(false);
        live.resume();
        live.tick();
        let before = request(&mut live, Request::Status);
        assert_eq!(query(&mut live, "arena_state")["frame"], 1);
        assert_eq!(query(&mut live, "recording")["recorded_ticks"], 1);
        success(request(&mut live, Request::Capture));
        for operation in [
            Request::Invoke {
                name: "pause".into(),
                arguments: BTreeMap::new(),
            },
            Request::Step { frames: 1 },
            Request::InjectInput {
                frame: 2,
                actions: BTreeMap::new(),
            },
            Request::SetField {
                entity: EntityId {
                    index: 0,
                    generation: 0,
                },
                component: "titan_game::game::Position".into(),
                field: "x".into(),
                value: 10.into(),
            },
        ] {
            let result = request(&mut live, operation);
            assert_eq!(
                (result.observed_frame, result.state_revision),
                (before.observed_frame, before.state_revision)
            );
            assert!(
                matches!(result.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
            );
        }
    }

    #[test]
    fn recording_replays_short_taps_repress_edges_and_reset_segments() {
        let mut live = session(true);
        live.resume();
        live.set_action("dash", true).unwrap();
        live.set_action("dash", false).unwrap();
        live.tick();
        assert_eq!(query(&mut live, "arena_state")["position"]["x"], 84);
        // A fresh physical edge while still active on the previous tick must survive export.
        live.set_action("dash", true).unwrap();
        live.tick();
        let recording = query(&mut live, "recording");
        assert_eq!(
            recording["frames"][0]["pressed"],
            serde_json::json!(["dash"])
        );
        assert_eq!(
            recording["frames"][1]["pressed"],
            serde_json::json!(["dash"])
        );
        assert!(
            verify_recording(recording).unwrap()["verified"]
                .as_bool()
                .unwrap()
        );
        for _ in 0..119 {
            live.tick();
        }
        live.set_action("dash", false).unwrap();
        live.set_action("left", true).unwrap();
        live.tick();
        live.set_action("dash", true).unwrap();
        live.tick();
        live.pause();
        let recording = query(&mut live, "recording");
        assert_eq!(recording["recorded_ticks"], 123);
        assert!(verify_recording(recording).is_ok());
        live.restart();
        live.resume();
        live.tick();
        let recording = query(&mut live, "recording");
        assert_eq!(recording["start_host_frame"], 123);
        assert_eq!(verify_recording(recording).unwrap()["replay_frame"], 1);
    }

    #[test]
    fn recording_bounds_validation_and_external_mutation_are_explicit() {
        let mut live = session(true);
        let recording = query(&mut live, "recording");
        let mut invalid = recording.clone();
        invalid["game_seed"] = 123.into();
        assert!(verify_recording(invalid).unwrap_err().contains("header"));
        let mut invalid = recording;
        invalid["recorded_ticks"] = 1.into();
        assert!(
            verify_recording(invalid)
                .unwrap_err()
                .contains("frame count")
        );
        let entity = live.app().world().entities().next().unwrap();
        success(request(
            &mut live,
            Request::SetField {
                entity: EntityId {
                    index: entity.index(),
                    generation: entity.generation(),
                },
                component: std::any::type_name::<game::Position>().into(),
                field: "x".into(),
                value: 20.into(),
            },
        ));
        assert!(
            verify_recording(query(&mut live, "recording"))
                .unwrap_err()
                .contains("invalidated")
        );
        live.restart();
        live.resume();
        for _ in 0..MAX_RECORDING_TICKS + 1 {
            live.tick();
        }
        let recording = query(&mut live, "recording");
        assert_eq!(recording["recorded_ticks"], MAX_RECORDING_TICKS);
        assert_eq!(recording["truncated"], true);
        assert!(
            verify_recording(recording)
                .unwrap_err()
                .contains("truncated")
        );
    }
    #[test]
    fn recording_preserves_ready_tick_repress_and_rejects_tampering() {
        let mut live = session(true);
        live.resume();
        live.set_action("dash", true).unwrap();
        for _ in 0..120 {
            live.tick();
        }
        live.set_action("dash", false).unwrap();
        live.set_action("dash", true).unwrap();
        live.tick();
        let recording = query(&mut live, "recording");
        assert_eq!(
            recording["frames"][119]["active"],
            serde_json::json!(["dash"])
        );
        assert_eq!(
            recording["frames"][120]["pressed"],
            serde_json::json!(["dash"])
        );
        assert_eq!(recording["final_state"]["position"]["x"], 108);
        assert!(verify_recording(recording.clone()).is_ok());
        let mut corrupted = recording.clone();
        corrupted["frames"][120]["pressed"] = serde_json::json!([]);
        assert!(
            verify_recording(corrupted)
                .unwrap_err()
                .contains("mismatch")
        );
        let mut malformed = recording.clone();
        malformed["frames"][0]["active"] = serde_json::json!(["teleport"]);
        assert!(verify_recording(malformed).unwrap_err().contains("unknown"));
        let mut malformed = recording.clone();
        malformed["frames"][0]["released"] = serde_json::json!(["dash"]);
        assert!(
            verify_recording(malformed)
                .unwrap_err()
                .contains("conflict")
        );
        let mut oversized = recording;
        let frame = oversized["frames"][0].clone();
        oversized["frames"] = serde_json::Value::Array(vec![frame; MAX_RECORDING_TICKS + 1]);
        oversized["recorded_ticks"] = (MAX_RECORDING_TICKS + 1).into();
        assert!(verify_recording(oversized).unwrap_err().contains("bounds"));
    }

    #[test]
    fn contact_loss_replays_and_headless_field_edits_invalidate() {
        let mut live = session(true);
        live.resume();
        for _ in 0..310 {
            live.tick();
        }
        live.pause();
        let recording = query(&mut live, "recording");
        assert_eq!(recording["final_state"]["run"]["health"], 0);
        assert_eq!(recording["final_state"]["run"]["outcome"], "Lost");
        assert!(verify_recording(recording).is_ok());
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let mut inspector = game::inspector_with_capture(
            InspectionConfig::controlled("headless", "arena"),
            |app| titan_diagnostics::png_capture(&game::render_image(app.world())?),
        );
        inspector.set_mutation_enabled(true);
        let entity = app.world().entities().next().unwrap();
        success(inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "edit",
                Request::SetField {
                    entity: EntityId {
                        index: entity.index(),
                        generation: entity.generation(),
                    },
                    component: std::any::type_name::<game::Position>().into(),
                    field: "x".into(),
                    value: 20.into(),
                },
            ),
        ));
        assert!(recording_value(&app).unwrap()["invalid_reason"].is_string());
        app.advance_fixed(1);
        assert!(recording_value(&app).unwrap()["invalid_reason"].is_string());
        game::restart(&mut app);
        assert!(verify_recording(recording_value(&app).unwrap()).is_ok());
    }
}
