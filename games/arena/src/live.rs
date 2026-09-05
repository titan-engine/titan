//! Inspection and bounded recordings of the actual playable arena instance.
use std::collections::BTreeMap;

use serde::Deserialize;
use titan::{
    App, FixedTime, Startup, World,
    input::InputFrame,
    inspection::{InspectionConfig, Inspector, StepBudget},
    replay::{
        Playback as ReplayState, RecordedButtons as RecordedFrame, RecordingIdentity,
        SnapshotRecorder, SnapshotRecording as Recording,
    },
};
use titan_protocol::{
    CommandMetadata, ErrorCode, FieldMetadata, ProtocolError, QueryMetadata, Request,
    RequestEnvelope, ResponseEnvelope, ResponseOutcome, RunMode,
};

use crate::game::{self, Action};

pub const MAX_SEEK_TICKS_PER_UPDATE: usize = 120;
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

fn recording_identity() -> RecordingIdentity<'static> {
    RecordingIdentity {
        game_seed: u64::from(game::SEED),
        action_schema: ACTION_SCHEMA,
        fixed_step_nanos: 16_666_667,
        max_ticks: MAX_RECORDING_TICKS,
    }
}

struct RecordingState {
    recorder: SnapshotRecorder,
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
    recording
        .recorder
        .invalid_reason()
        .map(str::to_owned)
        .or_else(|| {
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
    finish_replay(world);
}

/// Each fresh run or restored save begins a snapshot-backed recording segment.
pub(crate) fn begin_recording(world: &mut World) {
    world.remove_resource::<ReplayState>();
    let buttons: Vec<_> = world
        .iter::<titan::ui::UiButton>()
        .map(|(entity, _)| entity)
        .collect();
    for entity in buttons {
        world
            .get_mut::<titan::ui::UiButton>(entity)
            .unwrap()
            .enabled = true;
    }
    let initial_snapshot = game::export_save_world(world).expect("initialized arena snapshot");
    let start_host_frame = world.resource::<FixedTime>().map_or(0, |time| time.tick());
    let expected_positions = positions(world);
    world.insert_resource(RecordingState {
        recorder: SnapshotRecorder::new(initial_snapshot, start_host_frame, MAX_RECORDING_TICKS),
        expected_positions,
    });
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadSaveArgs {
    save: serde_json::Value,
}

/// Runs after the scheduled-input override and before the simulation consumes it.
pub(crate) fn record_consumed(world: &mut World) {
    if let Some(replay) = world.resource_mut::<ReplayState>() {
        if let Some(frame) = replay.next_frame() {
            let input = decode_frame(frame).expect("validated replay frame");
            world.insert_resource(input);
        }
        return;
    }
    let invalid_reason = invalid_reason(world);
    let frame = RecordedFrame::capture(world.resource::<InputFrame<Action>>().unwrap(), &ACTIONS);
    let recording = &mut world.resource_mut::<RecordingState>().unwrap().recorder;
    if let Some(reason) = invalid_reason {
        recording.invalidate(reason);
    }
    match frame {
        Ok(frame) => recording.push(frame),
        Err(reason) => recording.invalidate(reason),
    }
}

fn comparable_state(app: &App) -> serde_json::Value {
    let mut state: serde_json::Value = serde_json::from_str(&game::status(app)).unwrap();
    state.as_object_mut().unwrap().remove("frame");
    state
}

fn recording_value(app: &App) -> Result<serde_json::Value, ProtocolError> {
    if let Some(replay) = app.world().resource::<ReplayState>() {
        return serde_json::to_value(replay.recording())
            .map_err(|error| invalid(error.to_string()));
    }
    let source = app.world().resource::<RecordingState>().unwrap();
    let image = game::render_image(app.world())?;
    let mut recording = source
        .recorder
        .export(
            recording_identity(),
            comparable_state(app),
            game::export_save(app).ok(),
            format!("{:016x}", game::image_checksum(&image)),
        )
        .map_err(invalid)?;
    recording.invalid_reason = invalid_reason(app.world());
    serde_json::to_value(recording)
        .map_err(|error| ProtocolError::new(ErrorCode::Internal, error.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadReplayArgs {
    recording: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeekReplayArgs {
    position: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaySpeedArgs {
    speed: f64,
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
                value["replay"] = replay_status(app.world());
                value["recording"] = if let Some(replay) = app.world().resource::<ReplayState>() {
                    let recording = replay.recording();
                    serde_json::json!({"start_host_frame":recording.start_host_frame,"recorded_ticks":recording.frames.len(),"max_ticks":MAX_RECORDING_TICKS,"truncated":recording.truncated,"invalid_reason":recording.invalid_reason})
                } else {
                    let recording = &app.world().resource::<RecordingState>().unwrap().recorder;
                    serde_json::json!({"start_host_frame":recording.start_host_frame,"recorded_ticks":recording.len(),"max_ticks":MAX_RECORDING_TICKS,"truncated":recording.truncated(),"invalid_reason":invalid_reason(app.world())})
                };
                Ok(value)
            },
        )
        .expect("unique arena state query");
    inspector
        .register_query(
            QueryMetadata {
                name: "recording".into(),
                description: "Export consumed fixed-tick inputs from the latest restart or save snapshot, bounded to 3600 ticks.".into(),
                arguments: BTreeMap::new(),
            },
            |app, _: NoArguments| recording_value(app),
        )
        .expect("unique recording query");
    inspector
        .register_query(
            QueryMetadata {
                name: "save".into(),
                description: "Export a bounded, versioned arena gameplay save; excludes host, UI and transient input state.".into(),
                arguments: BTreeMap::new(),
            },
            |app, _: NoArguments| game::export_save(app),
        )
        .expect("unique save query");
    inspector
        .register_command(
            CommandMetadata {
                name: "load_save".into(),
                description: "Restore a validated arena save at a safe point. Live players must be paused with controls enabled; host clock stays monotonic and a new snapshot recording begins.".into(),
                arguments: [("save".into(), FieldMetadata {
                    type_name: "ArenaSaveV1".into(),
                    description: "Raw versioned gameplay object returned by the save query, at most 64 KiB.".into(),
                    writable: false,
                    minimum: None,
                    maximum: None,
                    unit: None,
                })].into(),
            },
            |app, args: LoadSaveArgs| {
                if app.world().resource::<PlaybackControl>().is_some_and(|control| !control.paused) {
                    return Err(ProtocolError::new(ErrorCode::NotControlled, "pause the live player before loading a save"));
                }
                game::load_save(app, args.save)
            },
        )
        .expect("unique load command");
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
        inspector
            .register_command(
                CommandMetadata {
                    name: "load_replay".into(),
                    description: "Load a validated bounded recording while paused.".into(),
                    arguments: [(
                        "recording".into(),
                        FieldMetadata {
                            type_name: "ArenaRecording".into(),
                            description: "Raw recording or query response, at most 2 MiB.".into(),
                            writable: false,
                            minimum: None,
                            maximum: None,
                            unit: None,
                        },
                    )]
                    .into(),
                },
                |app, args: LoadReplayArgs| load_replay(app, args.recording),
            )
            .expect("unique replay load");
        inspector.register_command(
            CommandMetadata {
                name: "seek_replay".into(),
                description: "Seek a paused replay to a consumed-tick position; advances at most 120 ticks per host update.".into(),
                arguments: [("position".into(), FieldMetadata {
                    type_name: "usize".into(), description: "Consumed ticks from 0 through replay total.".into(),
                    writable: false, minimum: Some(0.0), maximum: Some(MAX_RECORDING_TICKS as f64), unit: Some("ticks".into()),
                })].into(),
            }, |app, args: SeekReplayArgs| seek_replay(app, args.position),
        ).expect("unique replay seek");
        inspector.register_command(
            CommandMetadata {
                name: "replay_speed".into(),
                description: "Set paused replay wall-clock speed; fixed ticks and manual stepping stay unchanged.".into(),
                arguments: [("speed".into(), FieldMetadata {
                    type_name: "f64".into(), description: "One of 0.25, 0.5, 1, 2 or 4.".into(),
                    writable: false, minimum: Some(0.25), maximum: Some(4.0), unit: None,
                })].into(),
            }, |app, args: ReplaySpeedArgs| set_replay_speed(app, args.speed),
        ).expect("unique replay speed");

        for (name, callback) in [
            (
                "restart_replay",
                restart_replay as fn(&mut App) -> Result<(), ProtocolError>,
            ),
            ("stop_replay", stop_replay),
        ] {
            inspector
                .register_command(
                    CommandMetadata {
                        name: name.into(),
                        description: format!(
                            "{name} while paused; stop returns to a fresh live run."
                        ),
                        arguments: BTreeMap::new(),
                    },
                    move |app, _: NoArguments| callback(app),
                )
                .expect("unique replay command");
        }

        for (name, paused) in [("pause", true), ("resume", false)] {
            inspector
                .register_command(
                    CommandMetadata {
                        name: name.into(),
                        description: format!("{name} the actual player at a fixed-tick safe point; clears pending input."),
                        arguments: BTreeMap::new(),
                    },
                    move |app, _: NoArguments| {
                        if !paused && (replay_complete(app.world()) || replay_seeking(app.world())) { return Ok(()); }
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

    pub fn replay_active(&self) -> bool {
        self.app.world().resource::<ReplayState>().is_some()
    }
    pub fn replay_status(&self) -> serde_json::Value {
        replay_status(self.app.world())
    }
    /// Validate and dry-run a bounded recording before replacing paused gameplay.
    /// Physical host controls do not require inspection mutation opt-in.
    pub fn load_replay(&mut self, recording: serde_json::Value) -> Result<(), ProtocolError> {
        load_replay(&mut self.app, recording)?;
        self.inspector.reset_capture_session();
        self.reset_timing_and_input();
        self.inspector.note_external_change();
        Ok(())
    }
    /// Restore the loaded origin without rewinding host time. Requires pause.
    pub fn restart_replay(&mut self) -> Result<(), ProtocolError> {
        restart_replay(&mut self.app)?;
        self.inspector.reset_capture_session();
        self.reset_timing_and_input();
        self.inspector.note_external_change();
        Ok(())
    }
    /// Leave playback for a fresh live run. Requires pause.
    pub fn stop_replay(&mut self) -> Result<(), ProtocolError> {
        stop_replay(&mut self.app)?;
        self.inspector.reset_capture_session();
        self.reset_timing_and_input();
        self.inspector.note_external_change();
        Ok(())
    }
    pub fn replay_seeking(&self) -> bool {
        replay_seeking(self.app.world())
    }
    pub fn replay_speed(&self) -> f64 {
        self.app
            .world()
            .resource::<ReplayState>()
            .map_or(1.0, ReplayState::speed)
    }
    pub fn set_replay_speed(&mut self, speed: f64) -> Result<(), ProtocolError> {
        set_replay_speed(&mut self.app, speed)?;
        self.reset_timing_and_input();
        self.inspector.note_external_change();
        Ok(())
    }
    /// Queue a bounded seek while paused. Targets include both origin and EOF.
    pub fn seek_replay(&mut self, position: usize) -> Result<(), ProtocolError> {
        seek_replay(&mut self.app, position)?;
        self.inspector.reset_capture_session();
        self.reset_timing_and_input();
        self.inspector.note_external_change();
        Ok(())
    }
    /// Hosts call once per update, including while paused, before servicing controls.
    pub fn update_replay_seek(&mut self) -> usize {
        let ticks = self
            .app
            .world()
            .resource::<ReplayState>()
            .map_or(0, |replay| replay.seek_budget(MAX_SEEK_TICKS_PER_UPDATE));
        if ticks > 0 {
            self.app.advance_fixed(ticks as u64);
            self.inspector.note_external_change();
            if !self.replay_seeking() {
                self.reset_timing_and_input();
            }
        }
        ticks
    }
    /// Consume exactly one recorded frame while paused; rejects completion.
    pub fn step_replay(&mut self) -> Result<(), ProtocolError> {
        require_paused(&self.app)?;
        if !self.replay_active() || replay_complete(self.app.world()) || self.replay_seeking() {
            return Err(invalid("no remaining replay frames"));
        }
        self.app.advance_fixed(1);
        self.inspector.note_external_change();
        Ok(())
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
        if self.replay_active() {
            return Ok(());
        }
        self.input.set_action(name, pressed)
    }

    pub fn cancel_action(&mut self, name: &str) -> Result<(), String> {
        self.input.cancel_action(name)
    }

    pub fn clear_input(&mut self) {
        self.cancel_pointer();
        self.input.clear();
        self.app
            .world_mut()
            .insert_resource(InputFrame::<Action>::default());
    }

    /// Returns whether the in-game UI consumed this primary-pointer update.
    pub fn pointer(&mut self, position: Option<(i32, i32)>, pressed: bool) -> bool {
        if self.replay_active() {
            return true;
        }
        let before = game::restart_epoch(&self.app);
        let result = game::handle_ui_pointer(&mut self.app, position, pressed);
        if game::restart_epoch(&self.app) != before {
            self.inspector.reset_capture_session();
            self.reset_timing_and_input();
            self.inspector.note_external_change();
        }
        result.consumed
    }

    pub fn cancel_pointer(&mut self) {
        game::cancel_ui_pointer(&mut self.app);
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
        if !paused && (replay_complete(self.app.world()) || self.replay_seeking()) {
            return;
        }
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
        self.inspector.reset_capture_session();
        self.reset_timing_and_input();
        self.inspector.note_external_change();
    }

    pub fn tick(&mut self) {
        if !self.paused() {
            self.input.tick(&mut self.app);
            if self.paused() {
                self.reset_timing_and_input();
            }
            self.inspector.note_external_change();
        }
    }

    pub fn handle(&mut self, request: &RequestEnvelope) -> ResponseEnvelope {
        self.dispatch(request).into_ready()
    }

    pub fn dispatch(&mut self, request: &RequestEnvelope) -> titan::inspection::Dispatch {
        let was_paused = self.paused();
        let reset_epoch = game::restart_epoch(&self.app);
        self.inspector.set_controlled(was_paused);
        self.inspector.set_step_budget(StepBudget {
            max_frames: self.app.world().resource::<ReplayState>().map_or(
                StepBudget::DEFAULT.max_frames,
                |r| {
                    if r.seeking() { 0 } else { r.remaining() as u64 }
                },
            ),
            ..StepBudget::DEFAULT
        });
        let allowed = !self.replay_active()
            || match &request.request {
                Request::SetField { .. } | Request::InjectInput { .. } => false,
                Request::Invoke { name, .. } => matches!(
                    name.as_str(),
                    "pause"
                        | "resume"
                        | "restart"
                        | "load_replay"
                        | "restart_replay"
                        | "stop_replay"
                        | "seek_replay"
                        | "replay_speed"
                ),
                _ => true,
            };
        let response = titan::inspection::dispatch_with_policy(
            &mut self.app,
            &mut self.inspector,
            self.enable_control && allowed,
            request,
        );
        let response = match response {
            titan::inspection::Dispatch::Ready(response) => response,
            pending @ titan::inspection::Dispatch::Pending(_) => return pending,
        };
        let replay_control_changed = matches!(&request.request, Request::Invoke { name, .. }
            if matches!(name.as_str(), "seek_replay" | "replay_speed"))
            && matches!(response.outcome, ResponseOutcome::Success { .. });
        if game::restart_epoch(&self.app) != reset_epoch
            || was_paused != self.paused()
            || replay_control_changed
        {
            self.reset_timing_and_input();
        }
        if matches!(response.outcome, ResponseOutcome::Success { .. }) {
            if matches!(request.request, Request::SetField { .. }) {
                self.app
                    .world_mut()
                    .resource_mut::<RecordingState>()
                    .unwrap()
                    .recorder
                    .invalidate("position field changed outside consumed input");
            }
            self.app.refresh_extracted();
        }
        if game::restart_epoch(&self.app) != reset_epoch {
            self.inspector.reset_capture_session();
        }
        self.inspector.set_controlled(self.paused());
        titan::inspection::Dispatch::Ready(response)
    }

    pub fn capture_timeout(&self) -> std::time::Duration {
        self.inspector.capture_timeout()
    }

    pub fn dispatch_json(&mut self, json: &str) -> titan::inspection::Dispatch {
        match serde_json::from_str(json) {
            Ok(request) => self.dispatch(&request),
            Err(_) => titan::inspection::dispatch_json_with_policy(
                &mut self.app,
                &mut self.inspector,
                self.enable_control,
                json,
            ),
        }
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

fn decode_frame(frame: &RecordedFrame) -> Result<InputFrame<Action>, String> {
    frame.decode(&ACTIONS)
}

/// Replay a raw recording or a saved CLI query response in a fresh headless app.
/// The captured state and software pixels must both agree before success is returned.
pub fn verify_recording(value: serde_json::Value) -> Result<serde_json::Value, String> {
    let recording = parse_recording(value)?;
    verify_parsed(&recording)
}

fn parse_recording(value: serde_json::Value) -> Result<Recording, String> {
    let recording = Recording::parse(value, recording_identity(), MAX_RECORDING_BYTES, true)?;
    for frame in &recording.frames {
        decode_frame(frame)?;
    }
    Ok(recording)
}

fn restore_origin(app: &mut App, recording: &Recording) -> Result<(), ProtocolError> {
    if let Some(snapshot) = &recording.initial_snapshot {
        game::load_save(app, snapshot.clone())
    } else {
        game::restart(app);
        Ok(())
    }
}

fn verify_parsed(recording: &Recording) -> Result<serde_json::Value, String> {
    let mut app = game::build_game();
    app.update_schedule(Startup);
    restore_origin(&mut app, recording).map_err(|e| e.message)?;
    for frame in &recording.frames {
        app.world_mut().insert_resource(decode_frame(frame)?);
        app.advance_fixed(1);
    }
    let state = comparable_state(&app);
    let save = game::export_save(&app).map_err(|e| e.message)?;
    let image = game::render_image(app.world()).map_err(|e| e.message)?;
    let checksum = format!("{:016x}", game::image_checksum(&image));
    if state != recording.final_state
        || checksum != recording.final_checksum
        || recording
            .final_snapshot
            .as_ref()
            .is_some_and(|expected| *expected != save)
    {
        return Err(format!(
            "replay mismatch: state or image differs (actual checksum {checksum})"
        ));
    }
    Ok(
        serde_json::json!({"verified":true,"ticks":recording.recorded_ticks,"source_start_host_frame":recording.start_host_frame,"replay_frame":app.world().resource::<FixedTime>().unwrap().tick(),"state":state,"save":save,"checksum":checksum}),
    )
}

fn invalid(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidValue, message)
}
fn require_paused(app: &App) -> Result<(), ProtocolError> {
    if app
        .world()
        .resource::<PlaybackControl>()
        .is_none_or(|c| !c.paused)
    {
        return Err(ProtocolError::new(
            ErrorCode::NotControlled,
            "replay playback requires a paused interactive session",
        ));
    }
    Ok(())
}
fn replay_seeking(world: &World) -> bool {
    world
        .resource::<ReplayState>()
        .is_some_and(ReplayState::seeking)
}
fn set_replay_speed(app: &mut App, speed: f64) -> Result<(), ProtocolError> {
    require_paused(app)?;
    app.world_mut()
        .resource_mut::<ReplayState>()
        .ok_or_else(|| invalid("no replay loaded"))?
        .set_speed(speed)
        .map_err(invalid)
}
fn seek_replay(app: &mut App, position: usize) -> Result<(), ProtocolError> {
    require_paused(app)?;
    let replay = app
        .world()
        .resource::<ReplayState>()
        .ok_or_else(|| invalid("no replay loaded"))?;
    if position > replay.recording().frames.len() {
        return Err(invalid("seek position exceeds replay total"));
    }
    if position < replay.position() {
        let mut replay = app.world_mut().remove_resource::<ReplayState>().unwrap();
        restore_origin(app, replay.recording())?;
        replay.rewind();
        app.world_mut().insert_resource(replay);
        disable_replay_buttons(app);
    }
    app.world_mut()
        .resource_mut::<ReplayState>()
        .unwrap()
        .seek(position)
        .map_err(invalid)?;
    finish_replay(app.world_mut());
    Ok(())
}
fn replay_complete(world: &World) -> bool {
    world
        .resource::<ReplayState>()
        .is_some_and(ReplayState::complete)
}
fn replay_status(world: &World) -> serde_json::Value {
    world
        .resource::<ReplayState>()
        .map_or_else(ReplayState::inactive_status, ReplayState::status)
}

fn load_replay(app: &mut App, value: serde_json::Value) -> Result<(), ProtocolError> {
    require_paused(app)?;
    let recording = parse_recording(value).map_err(invalid)?;
    let verified = verify_parsed(&recording).map_err(invalid)?;
    install_replay(app, recording, verified["save"].clone())
}
fn install_replay(
    app: &mut App,
    recording: Recording,
    expected_save: serde_json::Value,
) -> Result<(), ProtocolError> {
    restore_origin(app, &recording)?;
    app.world_mut()
        .insert_resource(ReplayState::new(recording, expected_save));
    disable_replay_buttons(app);
    finish_replay(app.world_mut());
    Ok(())
}
fn disable_replay_buttons(app: &mut App) {
    let buttons: Vec<_> = app
        .world()
        .iter::<titan::ui::UiButton>()
        .map(|(entity, _)| entity)
        .collect();
    for entity in buttons {
        app.world_mut()
            .get_mut::<titan::ui::UiButton>(entity)
            .unwrap()
            .enabled = false;
    }
}
fn restart_replay(app: &mut App) -> Result<(), ProtocolError> {
    require_paused(app)?;
    let replay = app
        .world()
        .resource::<ReplayState>()
        .ok_or_else(|| invalid("no replay loaded"))?;
    let recording = replay.recording().clone();
    let expected_save = replay.expected_snapshot().clone();
    install_replay(app, recording, expected_save)
}
fn stop_replay(app: &mut App) -> Result<(), ProtocolError> {
    require_paused(app)?;
    if app.world().resource::<ReplayState>().is_none() {
        return Err(invalid("no replay loaded"));
    }
    game::restart(app);
    Ok(())
}
fn finish_replay(world: &mut World) {
    if !replay_complete(world)
        || world
            .resource::<ReplayState>()
            .unwrap()
            .verified()
            .is_some()
    {
        return;
    }
    let result = game::render_image(world).and_then(|image| {
        let r = world.resource::<ReplayState>().unwrap();
        let save = game::export_save_world(world)?;
        let matched = format!("{:016x}", game::image_checksum(&image))
            == r.recording().final_checksum
            && *r.expected_snapshot() == save;
        if matched {
            Ok(())
        } else {
            Err(invalid("playback final state or pixels differ"))
        }
    });
    let r = world.resource_mut::<ReplayState>().unwrap();
    r.finish(result.map_err(|e| e.message))
        .expect("completed unverified replay");
    if let Some(control) = world.resource_mut::<PlaybackControl>() {
        control.paused = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::{EntityId, InputValue, Response};

    #[test]
    fn pending_capture_survives_pause_but_is_invalidated_by_world_replacement() {
        let mut session = session(true);
        let work = std::sync::Arc::new(std::sync::Mutex::new(None));
        let sink = work.clone();
        session
            .inspector
            .register_async_capture_handler(2, 2, move |_, _, completion| {
                *sink.lock().unwrap() = Some(completion);
                Ok(())
            });
        let titan::inspection::Dispatch::Pending(mut pending) =
            session.dispatch(&RequestEnvelope::new("delayed", Request::Capture))
        else {
            panic!("capture must defer");
        };
        session.pause();
        assert!(pending.poll(std::time::Duration::ZERO).is_none());
        let restarted = session.handle(&RequestEnvelope::new(
            "reset",
            Request::Invoke {
                name: "restart".into(),
                arguments: Default::default(),
            },
        ));
        assert!(matches!(restarted.outcome, ResponseOutcome::Success { .. }));
        let canceled = pending.poll(std::time::Duration::ZERO).unwrap();
        assert_eq!(canceled.request_id, "delayed");
        assert!(
            matches!(canceled.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::Cancelled)
        );
        let producer = work.lock().unwrap().take().unwrap();
        producer.complete(Err(ProtocolError::new(ErrorCode::Internal, "late result")));
        assert!(pending.poll(std::time::Duration::ZERO).is_none());
    }

    #[test]
    fn bounded_seek_retargets_and_matches_sequential_snapshots_and_pixels() {
        let mut source = session(false);
        source.resume();
        source.set_action("right", true).unwrap();
        let mut snapshots = Vec::new();
        let mut checksums = Vec::new();
        for _ in 0..301 {
            snapshots.push(game::export_save(source.app()).unwrap());
            checksums.push(game::image_checksum(
                &game::render_image(source.app().world()).unwrap(),
            ));
            source.tick();
        }
        source.pause();
        let recording = recording_value(source.app()).unwrap();
        let mut target = session(true);
        target.load_replay(recording.clone()).unwrap();
        target.set_replay_speed(4.0).unwrap();
        let initial_epoch = target.clock_epoch();
        target.seek_replay(300).unwrap();
        assert!(target.clock_epoch() > initial_epoch);
        assert_eq!(target.update_replay_seek(), 120);
        assert_eq!(target.replay_status()["target"], 300);
        assert!(target.step_replay().is_err());
        target.resume();
        assert!(target.paused());
        assert!(matches!(
            request(&mut target, Request::Step { frames: 1 }).outcome,
            ResponseOutcome::Failure { .. }
        ));
        target.set_action("left", true).unwrap();
        target.seek_replay(140).unwrap();
        assert_eq!(target.update_replay_seek(), 20);
        assert!(!target.replay_seeking());
        assert_eq!(game::export_save(target.app()).unwrap(), snapshots[140]);
        assert_eq!(
            game::image_checksum(&game::render_image(target.app().world()).unwrap()),
            checksums[140]
        );
        let host_frame = target.app.world().resource::<FixedTime>().unwrap().tick();
        target.seek_replay(60).unwrap();
        assert_eq!(
            target.app.world().resource::<FixedTime>().unwrap().tick(),
            host_frame
        );
        assert_eq!(target.replay_speed(), 4.0);
        assert_eq!(target.update_replay_seek(), 60);
        assert_eq!(game::export_save(target.app()).unwrap(), snapshots[60]);
        target.seek_replay(0).unwrap();
        assert_eq!(target.update_replay_seek(), 0);
        assert_eq!(game::export_save(target.app()).unwrap(), snapshots[0]);
        target.seek_replay(301).unwrap();
        assert_eq!(target.update_replay_seek(), 120);
        assert_eq!(target.update_replay_seek(), 120);
        assert_eq!(target.update_replay_seek(), 61);
        assert_eq!(target.update_replay_seek(), 0);
        assert_eq!(target.replay_status()["verified"], true);
        assert_eq!(
            game::export_save(target.app()).unwrap(),
            recording["final_snapshot"]
        );
        assert_eq!(recording_value(target.app()).unwrap(), recording);
        target.seek_replay(300).unwrap();
        assert_eq!(target.replay_status()["verified"], serde_json::Value::Null);
        while target.replay_seeking() {
            target.update_replay_seek();
        }
        target.step_replay().unwrap();
        assert_eq!(target.replay_status()["verified"], true);
        target.restart_replay().unwrap();
        assert_eq!(target.replay_status()["verified"], serde_json::Value::Null);
        assert_eq!(target.replay_speed(), 1.0);
        target.seek_replay(250).unwrap();
        target.stop_replay().unwrap();
        assert_eq!(target.update_replay_seek(), 0);
        assert!(!target.replay_seeking());
    }

    #[test]
    fn empty_replay_seek_is_immediate_and_current_position_cancels_pending_seek() {
        let mut target = session(true);
        let empty = recording_value(target.app()).unwrap();
        target.load_replay(empty).unwrap();
        target.seek_replay(0).unwrap();
        assert_eq!(target.update_replay_seek(), 0);
        assert_eq!(target.replay_status()["verified"], true);
        target.stop_replay().unwrap();
        target.resume();
        for _ in 0..121 {
            target.tick();
        }
        target.pause();
        target
            .load_replay(recording_value(target.app()).unwrap())
            .unwrap();
        target.seek_replay(121).unwrap();
        assert_eq!(target.update_replay_seek(), 120);
        target.seek_replay(120).unwrap();
        assert_eq!(target.update_replay_seek(), 0);
        assert!(!target.replay_seeking());
        assert_eq!(target.replay_status()["position"], 120);
        target.step_replay().unwrap();
        assert_eq!(target.replay_status()["verified"], true);
    }

    #[test]
    fn seek_and_speed_commands_validate_without_mutating_and_reset_clock_epoch() {
        let mut source = session(false);
        source.resume();
        source.tick();
        source.pause();
        let mut target = session(true);
        target
            .load_replay(recording_value(source.app()).unwrap())
            .unwrap();
        let invoke = |name: &str, key: &str, value| Request::Invoke {
            name: name.into(),
            arguments: [(key.into(), value)].into(),
        };
        let before = request(&mut target, Request::Status);
        let epoch = target.clock_epoch();
        for action in [
            invoke("seek_replay", "position", 2.into()),
            invoke("seek_replay", "position", (-1).into()),
            invoke("seek_replay", "position", 0.5.into()),
            invoke("replay_speed", "speed", 3.into()),
            invoke("replay_speed", "speed", "fast".into()),
        ] {
            let response = request(&mut target, action);
            assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
            assert_eq!(response.state_revision, before.state_revision);
            assert_eq!(response.observed_frame, before.observed_frame);
            assert_eq!(target.clock_epoch(), epoch);
        }
        for speed in [0.25, 0.5, 1.0, 2.0, 4.0] {
            let epoch = target.clock_epoch();
            success(request(
                &mut target,
                invoke("replay_speed", "speed", speed.into()),
            ));
            assert_eq!(target.replay_speed(), speed);
            assert!(target.clock_epoch() > epoch);
        }
        target.resume();
        assert!(target.seek_replay(1).is_err());
        assert!(target.set_replay_speed(1.0).is_err());
        target.pause();
        target.set_control_enabled(false);
        assert!(
            matches!(request(&mut target, invoke("seek_replay", "position", 1.into())).outcome,
            ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
        );
        target.seek_replay(1).unwrap();
        assert_eq!(target.update_replay_seek(), 1);
        assert_eq!(target.replay_status()["verified"], true);
    }

    #[test]
    fn snapshot_origin_playback_matches_complete_state_and_pixels() {
        for origin_ticks in [1, 194] {
            let mut source = session(false);
            source.resume();
            if origin_ticks == 1 {
                source.set_action("dash", true).unwrap();
            }
            for _ in 0..origin_ticks {
                source.tick();
            }
            source.pause();
            let origin = game::export_save(source.app()).unwrap();
            game::load_save(&mut source.app, origin.clone()).unwrap();
            source.resume();
            source.set_action("right", true).unwrap();
            source.set_action("dash", true).unwrap();
            for _ in 0..8 {
                source.tick();
            }
            source.pause();
            let recording = recording_value(source.app()).unwrap();
            assert_eq!(recording["format_version"], 2);
            assert_eq!(recording["initial_snapshot"], origin);
            let verified = verify_recording(recording.clone()).unwrap();
            let mut target = session(false);
            target.resume();
            target.tick();
            target.pause();
            let frame = target.app.world().resource::<FixedTime>().unwrap().tick();
            target.load_replay(recording.clone()).unwrap();
            assert_eq!(game::export_save(target.app()).unwrap(), origin);
            assert_eq!(recording_value(target.app()).unwrap(), recording);
            target.set_action("left", true).unwrap();
            assert!(target.pointer(Some((8, 12)), true));
            assert!(target.pointer(Some((8, 12)), false));
            target.step_replay().unwrap();
            target.resume();
            for _ in 0..20 {
                target.tick();
            }
            assert!(target.paused());
            assert_eq!(target.replay_status()["position"], 8);
            assert_eq!(target.replay_status()["verified"], true);
            assert_eq!(
                target.app.world().resource::<FixedTime>().unwrap().tick(),
                frame + 8
            );
            assert_eq!(game::export_save(target.app()).unwrap(), verified["save"]);
            assert_eq!(recording_value(target.app()).unwrap(), recording);
            assert!(target.step_replay().is_err());
            target.resume();
            assert!(target.paused());
            target.restart_replay().unwrap();
            assert_eq!(game::export_save(target.app()).unwrap(), origin);
            assert_eq!(
                target.app.world().resource::<FixedTime>().unwrap().tick(),
                frame + 8
            );
            target.stop_replay().unwrap();
            assert!(!target.replay_active());
            assert!(verify_recording(recording_value(target.app()).unwrap()).is_ok());
        }
    }

    #[test]
    fn replay_protocol_locks_live_mutation_and_rejects_overshoot_before_advancing() {
        let mut source = session(false);
        source.resume();
        source.tick();
        source.tick();
        source.pause();
        let recording = recording_value(source.app()).unwrap();
        let mut target = session(true);
        target.load_replay(recording.clone()).unwrap();
        let before = request(&mut target, Request::Status);
        for action in [
            Request::Step { frames: 3 },
            Request::InjectInput {
                frame: 1,
                actions: [("dash".into(), InputValue::Button(true))].into(),
            },
            Request::Invoke {
                name: "load_save".into(),
                arguments: [("save".into(), recording["initial_snapshot"].clone())].into(),
            },
            Request::Invoke {
                name: "ui_pointer".into(),
                arguments: [
                    ("x".into(), 8.into()),
                    ("y".into(), 12.into()),
                    ("pressed".into(), true.into()),
                ]
                .into(),
            },
        ] {
            let response = request(&mut target, action);
            assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
            assert_eq!(response.observed_frame, before.observed_frame);
            assert_eq!(response.state_revision, before.state_revision);
            assert_eq!(target.replay_status()["position"], 0);
        }
        let mut bad = recording.clone();
        bad["final_snapshot"]["run"]["random"] = 0.into();
        assert!(target.load_replay(bad).is_err());
        assert_eq!(recording_value(target.app()).unwrap(), recording);
        success(request(&mut target, Request::Step { frames: 2 }));
        assert_eq!(target.replay_status()["verified"], true);
        let completed = request(&mut target, Request::Status);
        assert!(matches!(
            request(&mut target, Request::Step { frames: 1 }).outcome,
            ResponseOutcome::Failure { .. }
        ));
        assert_eq!(
            request(&mut target, Request::Status).observed_frame,
            completed.observed_frame
        );
    }

    #[test]
    fn legacy_recording_verifies_and_plays_without_remote_opt_in() {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/recording-v1.json")).unwrap();
        let verified = verify_recording(value.clone()).unwrap();
        let mut live = session(false);
        live.load_replay(value).unwrap();
        live.resume();
        for _ in 0..MAX_RECORDING_TICKS {
            live.tick();
        }
        assert_eq!(live.replay_status()["verified"], true);
        assert_eq!(game::export_save(live.app()).unwrap(), verified["save"]);
    }

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
    #[test]
    fn ui_inspection_and_local_or_command_restart_share_session_reset() {
        let mut live = session(false);
        let button = live
            .app()
            .world()
            .iter::<titan::Name>()
            .find(|(_, name)| name.as_str() == "ui/restart")
            .unwrap()
            .0;
        let entity = EntityId {
            index: button.index(),
            generation: button.generation(),
        };
        let Response::Entity(details) = success(request(&mut live, Request::Entity { entity }))
        else {
            panic!("entity details")
        };
        assert_eq!(
            details.components[std::any::type_name::<titan::ui::UiText>()]["text"],
            "R RESTART"
        );
        assert!(
            !details.component_fields[std::any::type_name::<titan::ui::UiButton>()]["enabled"]
                .writable
        );
        live.resume();
        live.set_action("right", true).unwrap();
        live.tick();
        live.pause();
        let before = request(&mut live, Request::Status);
        let pointer_request = |pressed| Request::Invoke {
            name: "ui_pointer".into(),
            arguments: [
                ("x".into(), 8.into()),
                ("y".into(), 12.into()),
                ("pressed".into(), serde_json::Value::Bool(pressed)),
            ]
            .into(),
        };
        assert!(
            matches!(request(&mut live, pointer_request(true)).outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
        );
        assert!(live.pointer(Some((8, 12)), true));
        assert!(live.pointer(Some((8, 12)), false));
        assert!(
            live.paused(),
            "local button works while paused without inspector opt-in"
        );
        let after = request(&mut live, Request::Status);
        assert_eq!(after.observed_frame, before.observed_frame);
        assert!(after.state_revision > before.state_revision);
        assert_eq!(query(&mut live, "recording")["recorded_ticks"], 0);
        live.resume();
        live.tick();
        assert_eq!(
            query(&mut live, "arena_state")["position"]["x"],
            80,
            "button restart clears held movement"
        );
        live.pause();
        live.set_control_enabled(true);
        let clock = request(&mut live, Request::Status).observed_frame;
        success(request(
            &mut live,
            Request::InjectInput {
                frame: clock + 1,
                actions: [("dash".into(), InputValue::Button(true))].into(),
            },
        ));
        let epoch = live.clock_epoch();
        success(request(&mut live, pointer_request(true)));
        success(request(&mut live, pointer_request(false)));
        assert!(
            live.clock_epoch() > epoch,
            "command activation uses game reset epoch, not a restart command-name check"
        );
        assert!(live.paused());
        success(request(&mut live, Request::Step { frames: 1 }));
        assert_eq!(
            query(&mut live, "arena_state")["position"]["x"],
            80,
            "command activation clears pending injected input"
        );
        assert!(verify_recording(query(&mut live, "recording")).is_ok());
    }
    #[test]
    fn save_query_and_paused_load_restore_state_ui_and_preserve_session_identity() {
        let mut live = session(true);
        live.resume();
        live.set_action("right", true).unwrap();
        live.set_action("dash", true).unwrap();
        live.tick();
        let before_query = request(&mut live, Request::Status);
        let save = query(&mut live, "save");
        let after_query = request(&mut live, Request::Status);
        assert_eq!(before_query.state_revision, after_query.state_revision);
        let expected_state = comparable_state(live.app());
        let expected_image = game::image_checksum(&game::render_image(live.app().world()).unwrap());
        let load = |save| Request::Invoke {
            name: "load_save".into(),
            arguments: [("save".into(), save)].into(),
        };
        let denied = request(&mut live, load(save.clone()));
        assert!(
            matches!(denied.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::NotControlled)
        );
        assert_eq!(denied.state_revision, after_query.state_revision);
        live.pause();
        success(request(&mut live, Request::Step { frames: 20 }));
        let before_load = request(&mut live, Request::Status);
        let _recording_before = query(&mut live, "recording");
        let epoch = live.clock_epoch();
        let loaded = request(&mut live, load(save));
        success(loaded.clone());
        assert_eq!(loaded.observed_frame, before_load.observed_frame);
        assert!(loaded.state_revision > before_load.state_revision);
        assert!(live.clock_epoch() > epoch);
        assert!(live.paused());
        assert_eq!(comparable_state(live.app()), expected_state);
        assert_eq!(
            game::image_checksum(&game::render_image(live.app().world()).unwrap()),
            expected_image
        );
        let dash_label = live
            .app()
            .world()
            .iter::<titan::Name>()
            .find(|(_, name)| name.as_str() == "ui/dash")
            .unwrap()
            .0;
        assert_eq!(
            live.app()
                .world()
                .get::<titan::ui::UiText>(dash_label)
                .unwrap()
                .text,
            "DASH 2.0S"
        );
        let recording_after = query(&mut live, "recording");
        assert_eq!(
            recording_after["start_host_frame"],
            before_load.observed_frame
        );
        assert_eq!(recording_after["recorded_ticks"], 0);
        assert!(recording_after["invalid_reason"].is_null());
        assert!(verify_recording(recording_after).is_ok());
        live.restart();
        assert!(query(&mut live, "recording")["invalid_reason"].is_null());
    }

    #[test]
    fn invalid_or_disabled_load_does_not_mutate_and_valid_load_clears_stale_input() {
        let mut live = session(false);
        let save = query(&mut live, "save");
        let load = |save| Request::Invoke {
            name: "load_save".into(),
            arguments: [("save".into(), save)].into(),
        };
        let before = request(&mut live, Request::Status);
        let disabled = request(&mut live, load(save.clone()));
        assert!(
            matches!(disabled.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
        );
        assert_eq!(disabled.state_revision, before.state_revision);
        live.set_control_enabled(true);
        let before = request(&mut live, Request::Status);
        let state = query(&mut live, "save");
        let epoch = live.clock_epoch();
        for bad in [
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!({"padding":"x".repeat(game::MAX_SAVE_BYTES + 1)}),
        ] {
            let rejected = request(&mut live, load(bad));
            assert!(
                matches!(rejected.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
            );
            assert_eq!(
                (rejected.observed_frame, rejected.state_revision),
                (before.observed_frame, before.state_revision)
            );
            assert_eq!(live.clock_epoch(), epoch);
            assert_eq!(query(&mut live, "save"), state);
            assert!(query(&mut live, "recording")["invalid_reason"].is_null());
        }
        live.set_action("right", true).unwrap();
        live.set_action("dash", true).unwrap();
        live.pointer(Some((8, 12)), true);
        success(request(
            &mut live,
            Request::Invoke {
                name: "ui_pointer".into(),
                arguments: [
                    ("x".into(), 8.into()),
                    ("y".into(), 12.into()),
                    ("pressed".into(), true.into()),
                ]
                .into(),
            },
        ));
        success(request(
            &mut live,
            Request::InjectInput {
                frame: before.observed_frame + 1,
                actions: [("left".into(), InputValue::Button(true))].into(),
            },
        ));
        success(request(&mut live, load(save)));
        let epoch = live.clock_epoch();
        live.pointer(Some((8, 12)), false);
        success(request(
            &mut live,
            Request::Invoke {
                name: "ui_pointer".into(),
                arguments: [
                    ("x".into(), 8.into()),
                    ("y".into(), 12.into()),
                    ("pressed".into(), false.into()),
                ]
                .into(),
            },
        ));
        assert_eq!(
            live.clock_epoch(),
            epoch,
            "orphaned pre-load pointer releases must not restart"
        );
        success(request(&mut live, Request::Step { frames: 1 }));
        assert_eq!(
            query(&mut live, "arena_state")["position"]["x"],
            80,
            "load clears injected input"
        );
        live.resume();
        live.tick();
        assert_eq!(
            query(&mut live, "arena_state")["position"]["x"],
            80,
            "load clears buffered physical input"
        );
        assert!(query(&mut live, "recording")["invalid_reason"].is_null());
    }
}
